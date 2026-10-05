//! `drmod-tas` — командный клиент HTTP API мода: запуск и чтение `.tas`-скриптов,
//! состояние игры и выгрузка кольцевого буфера логов в скрипт.
//!
//! Клиент свой (не из редактора): CLI не должен линковать GUI. Тела скриптов
//! уходят gzip'ом — лимит тела мода (64 КБ) измеряется по сжатым байтам.
//!
//! ```text
//! drmod-tas run  <file.tas|-> [--watch]
//! drmod-tas get  [id|last]
//! drmod-tas state
//! drmod-tas export [--from ms] [--to ms] [--script-id N] [--human] [--limit N]
//!                   [--name NAME] [-o FILE]
//! ```

use std::collections::HashMap;
use std::io::{Read, Write};
use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Deserialize;

use drmod_replay_types::InputUnit;

mod game_window;

/// Куда стучится CLI по умолчанию — тот же адрес, что у редактора (`src/api.rs` мода).
const DEFAULT_URL: &str = "http://127.0.0.1:5223";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        usage();
        std::process::exit(2);
    };

    let result = match command.as_str() {
        "-h" | "--help" | "help" => {
            usage();
            Ok(())
        }
        "run" => run(&args[1..]),
        "get" => get(&args[1..]),
        "state" => state(&args[1..]),
        "export" => export(&args[1..]),
        other => Err(format!("unknown command '{other}'")),
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn usage() {
    println!(
        "drmod-tas — клиент HTTP API мода MGR:R\n\n\
         USAGE:\n  \
         drmod-tas run  <file.tas|-> [--watch] [--no-focus] [--url URL]\n  \
         drmod-tas get  [id|last] [--url URL]\n  \
         drmod-tas state [--url URL]\n  \
         drmod-tas export [--from ms] [--to ms] [--script-id N] [--human]\n  \
         \x20                [--limit N] [--name NAME] [-o FILE] [--url URL]\n\n\
         The mod listens on {DEFAULT_URL} by default."
    );
}

// ── the commands ─────────────────────────────────────────────────────────────

/// `run <file.tas|->` — the text is parsed (and refused with its line) before it is sent, then
/// posted to `POST /script/run.tas`.
///
/// Before the post the run does what the editor does: brings the game window to the foreground
/// (a script's own `restart` plays menu keys through DirectInput, which the game only reads while
/// it owns the focus) and settles any open pause/fail menu out of the way. `--no-focus` skips both.
/// `--watch` follows the run to its end.
fn run(rest: &[String]) -> Result<(), String> {
    let parsed = Parsed::of(rest)?;
    let source = parsed
        .positional
        .first()
        .ok_or("run needs a .tas file (or -)")?;

    let text = if source == "-" {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| format!("stdin: {e}"))?;
        buffer
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("{source}: {e}"))?
    };

    // Parsed here so a typo names its line without a round trip to the game.
    drmod_script::dsl::parse(&text).map_err(|refused| refused.frame_aware())?;

    let client = Client::new(&parsed.url());

    // The mod has to answer before the window is grabbed — no point stealing focus for a run
    // that will not go out.
    get_state(&client).map_err(|_| {
        "the mod is not answering — is the game running with the mod injected?".to_owned()
    })?;

    if !parsed.flag("no-focus") {
        if !game_window::focus_and_settle(350) {
            eprintln!(
                "warning: the game window did not take the foreground — menu input may be lost"
            );
        }
        ensure_gameplay(&client)?;
    }

    let answer = post_script(&client, &text)?;
    println!(
        "script {} '{}' {} ({} frames)",
        answer.script_id, answer.name, answer.status, answer.total_frames
    );

    if parsed.flag("watch") {
        watch(&client, answer.script_id, Duration::from_secs(600))?;
    }
    Ok(())
}

/// `GET /state` — the subset the CLI reads.
fn get_state(client: &Client) -> Result<StateSnapshot, String> {
    let (status, body) = client.get("/state")?;
    if !(200..300).contains(&status) {
        return Err(from_error(&body, status));
    }
    serde_json::from_str(&body).map_err(|e| format!("state response: {e}"))
}

/// The mod's one script slot, by id.
fn get_script(client: &Client, id: u32) -> Result<ScriptStatus, String> {
    let (status, body) = client.get(&format!("/script/{id}"))?;
    if !(200..300).contains(&status) {
        return Err(from_error(&body, status));
    }
    serde_json::from_str(&body).map_err(|e| format!("script response: {e}"))
}

/// Posts a `.tas` body, taking the mod's one script slot if it is held (the editor's `409` retry).
fn post_script(client: &Client, text: &str) -> Result<RunResponse, String> {
    post_body(client, "/script/run.tas", text)
}

/// Posts a JSON script body (`POST /script/run`) — the menu steps need it, because the DSL cannot
/// spell the raw `dik_key`/`raw_key` a menu screen reads.
fn post_json(client: &Client, json: &str) -> Result<RunResponse, String> {
    post_body(client, "/script/run", json)
}

/// Posts a script to `path`, taking the mod's one script slot if it is held (the editor's `409`
/// retry): stop whatever holds it, then retry once.
fn post_body(client: &Client, path: &str, body: &str) -> Result<RunResponse, String> {
    let (mut status, mut response) = client.post_gzip(path, body.as_bytes())?;
    if status == 409 {
        let _ = client.post_empty("/script/stop");
        (status, response) = client.post_gzip(path, body.as_bytes())?;
    }
    if !(200..300).contains(&status) {
        return Err(from_error(&response, status));
    }
    serde_json::from_str(&response).map_err(|e| format!("run response: {e}"))
}

/// Menus the player is dead in: the game does not leave them on its own, and their preselected
/// entry is Retry, so a single confirm is the whole way out.
const FAIL_MENUS: [&str; 3] = ["Mission Fail", "Mission Failed", "Game Over"];

/// The statuses a mission load spends time in — a run cannot start here and does not need to be
/// forced out of here either.
const LOADING_MENUS: [&str; 3] = [
    "Loading Into Mission",
    "Loading Into Boss Mission",
    "Main Menu Load",
];

/// DirectInput Escape (`drmod-core/src/tas/addresses.rs`), the key the codec screen reads to close.
const DIK_ESCAPE: u32 = 0x01;

/// Gets the game into gameplay before a run: a menu that is already open would swallow the keys a
/// script's own `restart` plays. The editor's `menu_settler`, ported.
fn ensure_gameplay(client: &Client) -> Result<(), String> {
    let menu = get_state(client)?
        .menu_status
        .unwrap_or_else(|| "unknown".to_owned());

    if menu.eq_ignore_ascii_case("In Game") {
        return Ok(());
    }

    if FAIL_MENUS.iter().any(|known| known.eq_ignore_ascii_case(&menu)) {
        play_menu(client, "fail-retry", |input| drmod_script::model::ScriptInput {
            confirm: true,
            ..input
        })?;
        return wait_menu(client, "In Game", Duration::from_secs(10))
            .ok_or_else(|| format!("the fail menu did not clear (still \"{menu}\")"));
    }

    if menu.eq_ignore_ascii_case("Pause Menu") {
        play_menu(client, "close-menu", |input| drmod_script::model::ScriptInput {
            pause: true,
            ..input
        })?;
        return wait_menu(client, "In Game", Duration::from_secs(3))
            .ok_or_else(|| "the pause menu did not close".to_owned());
    }

    // The codec is closed with Esc as a **raw DIK key** (DirectInput), not the `pause` bit: the
    // codec screen reads the device key, and `pause` does nothing there (measured).
    if menu.eq_ignore_ascii_case("Codec") {
        play_menu(client, "close-codec", |input| drmod_script::model::ScriptInput {
            dik_key: Some(DIK_ESCAPE),
            ..input
        })?;
        return wait_menu(client, "In Game", Duration::from_secs(3))
            .ok_or_else(|| "the codec did not close".to_owned());
    }

    if LOADING_MENUS.iter().any(|known| known.eq_ignore_ascii_case(&menu)) {
        return Err(format!("the game is loading (\"{menu}\") — a run needs gameplay"));
    }

    Err(format!(
        "the game is in \"{menu}\", and this tool only knows how to leave the pause, codec and fail menus"
    ))
}

/// Plays a one-command menu script through the mod — the same three-frame bit the editor sends.
fn play_menu(
    client: &Client,
    name: &str,
    set: fn(drmod_script::model::ScriptInput) -> drmod_script::model::ScriptInput,
) -> Result<(), String> {
    let document = drmod_script::model::ScriptDocument {
        name: name.to_owned(),
        trigger: None,
        restart: None,
        commands: vec![drmod_script::model::ScriptCommand {
            t: 0,
            duration: 3,
            input: set(drmod_script::model::ScriptInput::default()),
            when_enemy: None,
        }],
    };

    // A menu step goes out as JSON, not `.tas`: it may need a raw `dik_key`, which the DSL cannot
    // spell (`docs/SCRIPT_DSL.md` §6).
    let json = drmod_script::json::write(&document).map_err(|e| e.message().to_owned())?;
    let answer = post_json(client, &json)?;

    // Wait for the menu step to finish before the next one (or the real script) goes out.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match get_script(client, answer.script_id) {
            Ok(script) => match script.status.as_deref() {
                Some("done") | Some("stopped") => return Ok(()),
                _ => {}
            },
            // The slot is gone — whoever held it finished.
            Err(_) => return Ok(()),
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    Err(format!("menu step '{name}' did not finish"))
}

/// Waits until the menu status reads `menu`, or the timeout runs out.
fn wait_menu(client: &Client, menu: &str, timeout: Duration) -> Option<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(state) = get_state(client)
            && state
                .menu_status
                .is_some_and(|current| current.eq_ignore_ascii_case(menu))
        {
            return Some(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// `get [id|last]` — the script's `.tas`, as the mod holds it.
fn get(rest: &[String]) -> Result<(), String> {
    let parsed = Parsed::of(rest)?;
    let id = parsed.positional.first().map(String::as_str).unwrap_or("last");
    let client = Client::new(&parsed.url());

    let (status, body) = client.get(&format!("/script/{id}.tas"))?;
    if !(200..300).contains(&status) {
        return Err(from_error(&body, status));
    }
    print!("{body}");
    if !body.ends_with('\n') {
        println!();
    }
    Ok(())
}

/// `state` — the handful of facts a terminal wants, read off the full `/state`.
fn state(rest: &[String]) -> Result<(), String> {
    let parsed = Parsed::of(rest)?;
    let client = Client::new(&parsed.url());

    let (status, body) = client.get("/state")?;
    if !(200..300).contains(&status) {
        return Err(from_error(&body, status));
    }
    let state: StateSnapshot =
        serde_json::from_str(&body).map_err(|e| format!("state response: {e}"))?;

    println!(
        "mission: {} ({})",
        state.mission_name.unwrap_or_default(),
        state.mission_id
    );
    println!("menu:    {}", state.menu_status.unwrap_or_default());
    if let Some(player) = state.player {
        println!(
            "player:  found={} hp={} pos=({:.2}, {:.2}, {:.2})",
            player.found, player.hp, player.pos[0], player.pos[1], player.pos[2]
        );
    }
    match state.script {
        Some(script) => println!(
            "script:  id={} '{}' {} {}/{}",
            script.id,
            script.name.unwrap_or_default(),
            script.status.unwrap_or_default(),
            script.frame,
            script.total_frames
        ),
        None => println!("script:  none"),
    }
    println!("fps:     {:.1}", state.fps);
    Ok(())
}

/// `export` — the log ring (or a window of it) written back out as a `.tas`.
///
/// The whole ring is fetched by default (`limit=5000`): `/logs` answers with the *oldest* `limit`
/// frames of the window, so the server's own default of 1000 would export a run's first ~16 s and
/// drop the recent input the caller is looking at. `--limit` narrows it deliberately.
///
/// `--human` keeps only the frames no script was running on, which is the player's own input
/// rather than a script's override. It is filtered here: `/logs` does not know the flag (the
/// mod's `GET /logs.tas` does).
fn export(rest: &[String]) -> Result<(), String> {
    let parsed = Parsed::of(rest)?;
    let client = Client::new(&parsed.url());

    let limit = parsed.value("limit").unwrap_or("5000");
    let mut query = vec![format!("limit={limit}")];
    for (name, value) in [
        ("from_ms", parsed.value("from")),
        ("to_ms", parsed.value("to")),
        ("script_id", parsed.value("script-id")),
    ] {
        if let Some(value) = value {
            query.push(format!("{name}={value}"));
        }
    }
    let path = format!("/logs?{}", query.join("&"));

    let (status, body) = client.get(&path)?;
    if !(200..300).contains(&status) {
        return Err(from_error(&body, status));
    }
    let logs: LogsResponse = serde_json::from_str(&body).map_err(|e| format!("logs: {e}"))?;

    let human = parsed.flag("human");
    let records: Vec<_> = logs
        .frames
        .into_iter()
        .filter(|frame| !human || frame.script_id.is_none())
        .enumerate()
        .map(|(index, frame)| drmod_script::record::RecordFrame {
            frame_index: index as u32,
            input: frame.input.into_unit(),
            ripper: false,
            keybind_down: frame.keybind_down_bits,
            keybind_pressed: frame.keybind_pressed_bits,
            pos: frame.pos,
            menu_status_raw: frame.menu_status_raw,
        })
        .collect();

    let mut document = drmod_script::record::document(&records).map_err(|e| e.message().to_owned())?;
    if let Some(name) = parsed.value("name") {
        document.name = name.to_string();
    }
    let text = drmod_script::dsl::write(&document).map_err(|e| e.message().to_owned())?;

    match parsed.value("out") {
        Some(path) => {
            std::fs::write(&path, text.as_bytes()).map_err(|e| format!("{path}: {e}"))?;
            eprintln!(
                "{} frames -> {} ({} commands)",
                records.len(),
                path,
                document.commands.len()
            );
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// Follows a run to its end, printing the phase changes.
fn watch(client: &Client, id: u32, timeout: Duration) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut last = String::new();
    loop {
        let (status, body) = client.get(&format!("/script/{id}"))?;
        if status == 200
            && let Ok(script) = serde_json::from_str::<ScriptStatus>(&body)
        {
            let word = script.status.clone().unwrap_or_default();
            if word != last {
                println!("  {} {}/{}", word, script.frame, script.total_frames);
            }
            let finished = matches!(word.as_str(), "done" | "stopped");
            last = word;
            if finished {
                return Ok(());
            }
        }

        if started.elapsed() > timeout {
            return Err(format!("timed out waiting for script {id}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

// ── the client ───────────────────────────────────────────────────────────────

/// The mod's API, one request at a time (`Connection: close`, like the mod's own server).
struct Client {
    base: String,
    agent: ureq::Agent,
}

impl Client {
    fn new(url: &str) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_millis(1500)))
            .timeout_global(Some(Duration::from_secs(30)))
            // A 4xx is an answer, not a transport failure: the mod's `{ "error": ... }` body is
            // what the caller wants to read.
            .http_status_as_error(false)
            .build();

        Self {
            base: url.trim_end_matches('/').to_owned(),
            agent: config.into(),
        }
    }

    fn get(&self, path: &str) -> Result<(u16, String), String> {
        self.call(false, path, None)
    }

    /// A script body, gzipped: the mod's body limit is measured on the compressed bytes.
    fn post_gzip(&self, path: &str, plain: &[u8]) -> Result<(u16, String), String> {
        self.call(true, path, Some(gzipped(plain)))
    }

    /// `POST` with no body (`/script/stop`).
    fn post_empty(&self, path: &str) -> Result<(u16, String), String> {
        let url = format!("{}{path}", self.base);
        let call = self
            .agent
            .post(&url)
            .header("Connection", "close")
            .send_empty();
        read_call(call)
    }

    fn call(&self, post: bool, path: &str, body: Option<Vec<u8>>) -> Result<(u16, String), String> {
        let url = format!("{}{path}", self.base);
        let call = if post {
            let request = self
                .agent
                .post(&url)
                .header("Connection", "close")
                .header("Content-Type", "text/plain; charset=utf-8")
                .header("Content-Encoding", "gzip");
            match body {
                Some(bytes) => request.send(bytes),
                None => request.send_empty(),
            }
        } else {
            self.agent
                .get(&url)
                .header("Connection", "close")
                .call()
        };

        read_call(call)
    }
}

/// The common ending of a call: the status and the body, or the transport error as a string.
fn read_call(
    call: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<(u16, String), String> {
    match call {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let text = response.body_mut().read_to_string().unwrap_or_default();
            Ok((status, text))
        }
        Err(error) => Err(error.to_string()),
    }
}

/// The `{ "error": "..." }` the mod answers a refusal with, or the bare code.
fn from_error(body: &str, status: u16) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: Option<String>,
    }

    serde_json::from_str::<ErrorBody>(body)
        .ok()
        .and_then(|error| error.error)
        .unwrap_or_else(|| format!("HTTP {status}"))
}

/// The request body, gzipped (the mod's `Content-Encoding: gzip`).
fn gzipped(plain: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    if encoder.write_all(plain).is_err() {
        return plain.to_vec();
    }
    encoder.finish().unwrap_or_else(|_| plain.to_vec())
}

// ── args ─────────────────────────────────────────────────────────────────────

/// A flat `--key value` / `--bool` / positional parse — the CLI is small enough that a parser
/// crate would cost more than it saves.
#[derive(Default)]
struct Parsed {
    positional: Vec<String>,
    values: HashMap<String, String>,
    flags: HashMap<String, bool>,
}

impl Parsed {
    fn of(args: &[String]) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut i = 0;
        while i < args.len() {
            let argument = &args[i];
            if let Some(key) = argument.strip_prefix("--") {
                if matches!(key, "human" | "watch" | "no-focus") {
                    parsed.flags.insert(key.to_owned(), true);
                } else if let Some((key, value)) = key.split_once('=') {
                    parsed.values.insert(key.to_owned(), value.to_owned());
                } else {
                    let value = args
                        .get(i + 1)
                        .ok_or_else(|| format!("--{key} needs a value"))?;
                    parsed.values.insert(key.to_owned(), value.clone());
                    i += 1;
                }
            } else if argument == "-o" {
                let value = args.get(i + 1).ok_or("-o needs a value")?;
                parsed.values.insert("out".to_owned(), value.clone());
                i += 1;
            } else {
                parsed.positional.push(argument.clone());
            }
            i += 1;
        }
        Ok(parsed)
    }

    fn url(&self) -> String {
        self.values
            .get("url")
            .cloned()
            .unwrap_or_else(|| DEFAULT_URL.to_owned())
    }

    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn flag(&self, key: &str) -> bool {
        self.flags.get(key).copied().unwrap_or(false)
    }
}

// ── the API's JSON, as far as the CLI reads it ───────────────────────────────

#[derive(Deserialize)]
struct RunResponse {
    #[serde(default)]
    script_id: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    total_frames: u32,
}

#[derive(Deserialize)]
struct ScriptStatus {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    frame: u32,
    #[serde(default)]
    total_frames: u32,
}

#[derive(Deserialize, Default)]
struct StateSnapshot {
    #[serde(default)]
    mission_id: i32,
    #[serde(default)]
    mission_name: Option<String>,
    #[serde(default)]
    menu_status: Option<String>,
    #[serde(default)]
    player: Option<StatePlayer>,
    #[serde(default)]
    script: Option<StateScript>,
    #[serde(default)]
    fps: f32,
}

#[derive(Deserialize)]
struct StatePlayer {
    #[serde(default)]
    found: bool,
    #[serde(default)]
    pos: [f32; 3],
    #[serde(default)]
    hp: i32,
}

#[derive(Deserialize)]
struct StateScript {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    frame: u32,
    #[serde(default)]
    total_frames: u32,
}

#[derive(Deserialize)]
struct LogsResponse {
    #[serde(default)]
    frames: Vec<LogFrame>,
}

#[derive(Deserialize)]
struct LogFrame {
    #[serde(default)]
    script_id: Option<u32>,
    #[serde(default)]
    menu_status_raw: i32,
    #[serde(default)]
    keybind_down_bits: u32,
    #[serde(default)]
    keybind_pressed_bits: u32,
    #[serde(default)]
    pos: [f32; 3],
    input: LogInput,
}

/// The `input` object of a `/logs` frame, decoded back into the mod's `InputUnit`.
#[derive(Deserialize)]
struct LogInput {
    down_bits: u32,
    left_stick: [f32; 2],
    right_stick: [f32; 2],
}

impl LogInput {
    fn into_unit(self) -> InputUnit {
        InputUnit {
            buttons_down: self.down_bits,
            left_stick: self.left_stick,
            right_stick: self.right_stick,
            ..Default::default()
        }
    }
}

