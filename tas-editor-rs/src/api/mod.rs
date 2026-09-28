//! The mod's HTTP API as the editor uses it: the state it reads, the script it starts, and the
//! four run levers it sets.
//!
//! A request comes back as a value, never as an error — the same rule the workspace follows, and
//! for the same reason: the only thing a click handler or a poll tick can do with a failure is
//! paint it. The three ways a call can end are therefore spelled out in [`ApiResult`]: an answer,
//! a refusal by the mod (with the mod's own wording, which already names the field), and no answer
//! at all.
//!
//! ⚠️ The mod's server is single-threaded and lives in the game's render loop: it answers one
//! request at a time. Polling it is cheap but not free, which is why the panel asks a couple of
//! times a second and never per frame.

pub mod json;
pub mod rules;
pub mod status;

use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write as _;

pub use json::ApiJson;
pub use rules::{FpsCapMode, PlaybackRules};
pub use status::{GameScript, GameScriptPhase, GameStatus};

/// Where the mod listens (`src/api.rs`, both builds).
pub const DEFAULT_URL: &str = "http://127.0.0.1:5223";

/// How a call to the mod ended: an answer, a refusal, or no answer at all.
///
/// `T` is the parsed answer; the raw payload is what a lever's call gets, since the editor only
/// needs to know it went through.
#[derive(Clone, Debug)]
pub struct ApiResult<T> {
    pub value: Option<T>,
    pub error: Option<String>,
    /// No answer: nothing is injected, the game is not running, or the mod dropped the socket
    /// twice.
    pub offline: bool,
    /// The mod refused because its one script slot is taken (`409`) — an answer the caller can do
    /// something about, rather than a plain failure.
    pub conflict: bool,
}

impl<T> ApiResult<T> {
    pub fn good(value: T) -> Self {
        Self {
            value: Some(value),
            error: None,
            offline: false,
            conflict: false,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            value: None,
            error: Some(error.into()),
            offline: false,
            conflict: false,
        }
    }

    pub fn taken(error: impl Into<String>) -> Self {
        Self {
            value: None,
            error: Some(error.into()),
            offline: false,
            conflict: true,
        }
    }

    pub fn no_answer(error: impl Into<String>) -> Self {
        Self {
            value: None,
            error: Some(error.into()),
            offline: true,
            conflict: false,
        }
    }

    /// Whether the mod answered and the body parsed.
    pub fn ok(&self) -> bool {
        self.value.is_some() && self.error.is_none()
    }

    /// What to show the user: the mod's own wording when it refused, and otherwise why there was
    /// no answer at all.
    pub fn message(&self) -> String {
        self.error
            .clone()
            .unwrap_or_else(|| "the mod did not answer".to_owned())
    }
}

/// The mod's HTTP API client.
///
/// Every request carries `Connection: close`: the mod's server answers and then closes the socket
/// (`respond` in `src/api.rs`), so the connection is not reused after the server has already
/// dropped it. A dropped connection under load is retried once.
pub struct ModApi {
    base: String,
    agent: ureq::Agent,
}

impl ModApi {
    pub fn new(url: &str) -> Self {
        // A request is answered in milliseconds; the wait is for the game's own stalls (a level
        // loading blocks the render loop the server runs in).
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_millis(1500)))
            .timeout_global(Some(Duration::from_millis(2500)))
            .build();

        Self {
            base: url.trim_end_matches('/').to_owned(),
            agent: config.into(),
        }
    }

    /// `GET /state` — the whole snapshot, as the panel's model.
    pub fn state(&self) -> ApiResult<GameStatus> {
        let answer = self.get("/state");
        if !answer.ok() {
            return ApiResult {
                value: None,
                error: answer.error,
                offline: answer.offline,
                conflict: answer.conflict,
            };
        }

        match json::read_state(answer.value.as_deref().unwrap_or("")) {
            Some(state) => ApiResult::good(GameStatus::of(&state)),
            None => ApiResult::failed("the mod's /state did not read as JSON"),
        }
    }

    /// `POST /script/run` — the script body is the editor's own JSON, which is the same shape the
    /// fixtures carry, so the mod's own parser is the only authority on it.
    ///
    /// A `409` means the mod's one script slot is taken: it is reported as its own kind of answer,
    /// because the caller can do something about it (stop that script and run this one).
    pub fn run(&self, script: &str) -> ApiResult<json::RunResponse> {
        let answer = self.post("/script/run", Some(script));
        if !answer.ok() {
            return ApiResult {
                value: None,
                error: answer.error,
                offline: answer.offline,
                conflict: answer.conflict,
            };
        }

        match json::read_run(answer.value.as_deref().unwrap_or("")) {
            Some(run) => ApiResult::good(run),
            None => ApiResult::failed("the mod's /script/run did not read as JSON"),
        }
    }

    /// `POST /script/stop` — clears the override and frees the slot. The mod restores the render
    /// and the frame cap by itself when a headless run ends, cancelled or not.
    pub fn stop(&self) -> ApiResult<String> {
        self.post("/script/stop", None)
    }

    /// `POST /dt`, `/fps`, `/rng`, `/render` — the levers. The bodies are built by
    /// [`PlaybackRules`]; what comes back is only told to be an answer or not.
    pub fn post(&self, path: &str, body: Option<&str>) -> ApiResult<String> {
        self.send(true, path, body)
    }

    pub fn get(&self, path: &str) -> ApiResult<String> {
        self.send(false, path, None)
    }

    fn send(&self, post: bool, path: &str, body: Option<&str>) -> ApiResult<String> {
        let url = format!("{}{path}", self.base);

        // One retry, not a loop: the second failure is the answer.
        for attempt in 0..=1 {
            let result = if post {
                self.post_once(&url, body)
            } else {
                self.get_once(&url)
            };

            match result {
                Ok(answer) => return answer,
                Err(error) => {
                    if attempt == 1 {
                        return ApiResult::no_answer(error);
                    }

                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }

        unreachable!("the loop returns on its last attempt")
    }

    fn get_once(&self, url: &str) -> Result<ApiResult<String>, String> {
        let request = self.agent.get(url).header("Connection", "close");

        self.finish(request.call())
    }

    fn post_once(&self, url: &str, body: Option<&str>) -> Result<ApiResult<String>, String> {
        let request = self
            .agent
            .post(url)
            .header("Connection", "close")
            .header("Content-Type", "application/json; charset=utf-8");

        let call = match body {
            Some(text) => {
                // ⚠️ The mod's body limit is 64 KiB measured on the *compressed* bytes, so a
                // script long enough to be worth storing is only accepted gzipped. The body is
                // sent as bytes rather than as a string: `Content-Length` has to be the compressed
                // length.
                let compressed = gzipped(text.as_bytes());
                request
                    .header("Content-Encoding", "gzip")
                    .send(compressed)
            }
            None => request.send_empty(),
        };

        self.finish(call)
    }

    /// Reads a response into an [`ApiResult`], or tells the caller the transport itself failed —
    /// which is the only reason to retry.
    fn finish(
        &self,
        result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<ApiResult<String>, String> {
        let mut response = match result {
            Ok(response) => response,
            Err(error) => {
                // A transport failure is worth one retry; a status code is not.
                return match error {
                    ureq::Error::StatusCode(code) => Ok(self.refusal(code, String::new())),
                    other => Err(other.to_string()),
                };
            }
        };

        let status = response.status().as_u16();
        let payload = response.body_mut().read_to_string().unwrap_or_default();

        if (200..300).contains(&status) {
            Ok(ApiResult::good(payload))
        } else {
            Ok(self.refusal(status, payload))
        }
    }

    fn refusal(&self, status: u16, payload: String) -> ApiResult<String> {
        let refused = json::read_error(&payload).unwrap_or_else(|| format!("HTTP {status}"));

        if status == 409 {
            ApiResult::taken(refused)
        } else {
            ApiResult::failed(refused)
        }
    }
}

/// The request body, gzipped, as the mod reads it (`Content-Encoding: gzip`): a script that fits
/// a long run is far past 64 KiB of JSON — the mod's body limit is on the *compressed* bytes.
///
/// Compression is unconditional: every body this client sends is a small JSON object, so the
/// branch that measured compressibility would cost more than the bytes it saves.
pub fn gzipped(plain: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    if encoder.write_all(plain).is_err() {
        // A `Vec` sink does not fail; unreachable in practice, and returning the input keeps the
        // caller working rather than panicking a UI thread.
        return plain.to_vec();
    }

    encoder.finish().unwrap_or_else(|_| plain.to_vec())
}
