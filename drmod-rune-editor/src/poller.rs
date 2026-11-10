//! The mod's status poll, on a worker thread.
//!
//! The mod's HTTP server is single-threaded inside the game's render loop: it answers one
//! request at a time and stops answering while a level loads. The panel therefore asks a couple
//! of times a second and never from inside a frame.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::api::{GameStatus, ModApi};

/// How often the game is asked what it is doing.
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long an answer may be in flight before another poll is started. The mod's own timeout is
/// shorter; this is the wedge guard.
pub const POLL_PATIENCE: Duration = Duration::from_millis(2500);

pub struct Poller {
    receiver: Option<Receiver<GameStatus>>,
    started: Option<Instant>,
    next: Instant,
    url: String,
}

impl Poller {
    pub fn new(url: &str) -> Self {
        Self {
            receiver: None,
            started: None,
            next: Instant::now(),
            url: url.to_owned(),
        }
    }

    /// The newest answer, if one has arrived. Never blocks.
    pub fn take(&mut self) -> Option<GameStatus> {
        let receiver = self.receiver.as_ref()?;
        match receiver.try_recv() {
            Ok(state) => {
                self.receiver = None;
                self.started = None;
                Some(state)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                self.started = None;
                None
            }
        }
    }

    pub fn due(&self) -> bool {
        if self.receiver.is_some() {
            return self
                .started
                .is_some_and(|started| started.elapsed() > POLL_PATIENCE);
        }

        Instant::now() >= self.next
    }

    pub fn start(&mut self) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let url = self.url.clone();
        std::thread::spawn(move || {
            let api = ModApi::new(&url);
            let status = api.state().value.unwrap_or_else(GameStatus::offline);
            let _ = sender.send(status);
        });

        self.receiver = Some(receiver);
        self.started = Some(Instant::now());
        self.next = Instant::now() + POLL_INTERVAL;
    }

    /// Ask again on the next frame, after an action changed the game's state.
    pub fn kick(&mut self) {
        self.next = Instant::now();
    }
}
