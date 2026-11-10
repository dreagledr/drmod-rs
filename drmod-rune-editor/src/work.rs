//! The actions that talk to the mod, off the UI thread.
//!
//! Every one of them can block for seconds — the game's render loop owns the HTTP server, and a
//! level load stops it answering — so a click starts a worker and the frame keeps drawing. One
//! action is in flight at a time; the panel gates its buttons on [`Worker::busy`].

use std::sync::mpsc::{Receiver, TryRecvError};

use drmod_script::{ScriptDocument, json};

use crate::api::{DEFAULT_URL, ModApi, PlaybackRules};
use crate::menu_settler;
use crate::platform;

/// What a finished worker reports.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// The three levers, set without starting a script.
    Levers { error: Option<String> },
    /// A run: a refusal, or what to say when it went out.
    Run {
        warning: Option<String>,
        error: Option<String>,
        message: Option<String>,
    },
    /// A cancel.
    Stop { error: Option<String> },
}

#[derive(Default)]
pub struct Worker {
    receiver: Option<Receiver<Outcome>>,
}

impl Worker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }

    /// The finished worker's answer, if one arrived. Called once a frame; never blocks.
    pub fn take(&mut self) -> Option<Outcome> {
        let receiver = self.receiver.as_ref()?;
        match receiver.try_recv() {
            Ok(outcome) => {
                self.receiver = None;
                Some(outcome)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                None
            }
        }
    }

    fn start(&mut self, work: impl FnOnce() -> Outcome + Send + 'static) {
        if self.receiver.is_some() {
            return;
        }

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(work());
        });
        self.receiver = Some(receiver);
    }

    pub fn apply(&mut self, rules: PlaybackRules) {
        self.start(move || {
            let api = ModApi::new(DEFAULT_URL);
            match rules.apply(&api) {
                Some(error) => Outcome::Levers { error: Some(error) },
                None => Outcome::Levers { error: None },
            }
        });
    }

    pub fn stop(&mut self) {
        self.start(|| {
            let api = ModApi::new(DEFAULT_URL);
            let stopped = api.stop();
            Outcome::Stop {
                error: (!stopped.ok()).then(|| format!("остановка: {}", stopped.message())),
            }
        });
    }

    pub fn run(&mut self, document: ScriptDocument, rules: PlaybackRules) {
        self.start(move || run_script_at(DEFAULT_URL, &document, &rules));
    }
}

/// The whole of a run, off the UI thread: the window, the menu, the rules, the script.
///
/// The order is the one the Python tools established: the game window first (the menu keys a
/// `restart` plays arrive only while the game owns the input focus), then a menu settled out of
/// the way, then the rules, and the seed **last** — the mod freezes the LCG on the first tick of
/// the next script, and the script it has to land on is the one sent right after.
fn run_script_at(url: &str, document: &ScriptDocument, rules: &PlaybackRules) -> Outcome {
    let api = ModApi::new(url);

    // A fresh read before anything is sent: the poll can be half a second old, and the menu is
    // what decides whether the script's own restart can be played at all.
    let Some(snapshot) = api.state().value else {
        return Outcome::Run {
            warning: None,
            error: Some("Мод не отвечает — игра запущена, а мод внедрён?".to_owned()),
            message: None,
        };
    };

    // A warning, not a stop: the script runs either way, and what may be lost is the menu input
    // the script's own restart plays. `None` means there is no window to focus on this platform.
    let warning = match platform::focus_and_settle(350) {
        Some(false) => Some("Окно игры не вышло вперёд — ввод меню может не дойти".to_owned()),
        _ => None,
    };

    // Out of the pause menu, or out of a fail menu, before the script arms — otherwise the menu
    // swallows the keys the script's own restart plays.
    if let Some(stuck) = menu_settler::ensure_gameplay(&api) {
        return Outcome::Run {
            warning,
            error: Some(stuck),
            message: None,
        };
    }

    if let Some(lever) = rules.apply(&api) {
        return Outcome::Run {
            warning,
            error: Some(lever),
            message: None,
        };
    }

    let Ok(body) = json::write(document) else {
        return Outcome::Run {
            warning,
            error: Some("Скрипт не записывается как JSON".to_owned()),
            message: None,
        };
    };

    let mut started = api.run(&body);
    if started.conflict {
        // The mod holds one script slot and answers a second run with 409. A script that ended
        // between the poll and this click is a race the panel cannot see, so the slot is taken
        // once, by stopping whatever holds it.
        let _ = api.stop();
        started = api.run(&body);
    }

    let outcome = if let Some(run) = &started.value {
        format!(
            "«{}» отправлен — {} кадров",
            run.name.clone().unwrap_or_else(|| document.name.clone()),
            run.total_frames
        )
    } else {
        format!("«{}» отправлен", document.name)
    };

    if !started.ok() {
        return Outcome::Run {
            warning,
            error: Some(format!("запуск: {}", started.message())),
            message: None,
        };
    }

    let _ = snapshot;
    Outcome::Run {
        warning,
        error: None,
        message: Some(outcome),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// One request the worker sent, as `(path, body)`.
    type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    fn mock_mod() -> (String, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    break;
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();

                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line == "\r\n" || line == "\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }

                let mut body = vec![0u8; length];
                if length > 0 {
                    let _ = reader.read_exact(&mut body);
                }
                log.lock().unwrap().push((path.clone(), body));

                let answer = match path.as_str() {
                    "/state" => r#"{"menu_status":"In Game","fps":60,"script":null}"#,
                    "/script/run" => {
                        r#"{"script_id":1,"name":"test","total_frames":10,"status":"running"}"#
                    }
                    _ => "{}",
                };
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    answer.len(),
                    answer
                );
            }
        });

        (url, seen)
    }

    #[test]
    fn a_run_posts_the_levers_and_the_script_in_order() {
        let (url, seen) = mock_mod();
        let document = drmod_script::dsl::parse("! name=test\n0 a").unwrap();
        let rules = PlaybackRules::default();

        let outcome = run_script_at(&url, &document, &rules);
        let Outcome::Run { error, message, .. } = outcome else {
            panic!("a run was expected");
        };
        assert_eq!(error, None);
        assert!(message.unwrap().contains("test"));

        let paths: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.clone())
            .collect();
        // `/state` twice: the fresh snapshot at the start, and the menu check just before the
        // levers. Both are reads; the writes are one each and in order.
        assert_eq!(
            paths,
            ["/state", "/state", "/dt", "/fps", "/rng", "/script/run"]
        );

        // The seed goes last of the levers, immediately before the script: the mod freezes the LCG
        // on the first tick of the next script.
        assert!(seen.lock().unwrap()[4].1.len() > 10);
    }

    #[test]
    fn no_answer_names_the_game_rather_than_hanging() {
        let document = drmod_script::dsl::parse("! name=test\n0 a").unwrap();
        let outcome = run_script_at("http://127.0.0.1:1", &document, &PlaybackRules::default());
        let Outcome::Run { error, .. } = outcome else {
            panic!("a run was expected");
        };
        assert!(error.unwrap().contains("Мод не отвечает"));
    }
}
