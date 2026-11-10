//! The JSON of the mod's HTTP API — the shapes the editor reads and the request bodies it sends.
//!
//! Unlike the script JSON, unknown keys are *ignored* here: the mod's `/state` carries a player, a
//! camera and a frame ring the editor has no use for, and a new field on the mod's side must not
//! break the editor.
//!
//! Request bodies leave unset fields out: the mod treats an absent key as its own default, and
//! writing `"ms": null` would be a field it has to guess about.

use serde::{Deserialize, Serialize};

/// The request bodies and the response readers of the mod's API.
pub struct ApiJson;

impl ApiJson {
    /// `POST /dt` — the fixed tick. `ticks` rides along only when the tick is being turned on:
    /// with `fixed: false` the mod restores its own measured delta either way.
    pub fn dt(on: bool) -> String {
        write(&DtRequest {
            fixed: on,
            ms: None,
            ticks: on.then_some(true),
        })
    }

    /// `POST /fps` — `"game"` is the cap the game itself keeps, `"off"` lifts it.
    pub fn cap(cap: &'static str) -> String {
        write(&FpsRequest {
            cap: Some(cap),
            fps: None,
        })
    }

    pub fn fps(fps: u32) -> String {
        write(&FpsRequest {
            cap: None,
            fps: Some(fps),
        })
    }

    /// `POST /rng` — `"freeze"` with a seed is the reproducible pin; `"off"` hands the decision
    /// back to the game.
    pub fn seed(pin: &str, seed: Option<u32>) -> String {
        write(&RngRequest {
            pin: pin.to_owned(),
            seed: if pin == "freeze" { seed } else { None },
        })
    }

    /// `POST /render` — a headless run, and the way back to a rendered frame.
    pub fn headless(on: bool) -> String {
        write(&RenderRequest {
            headless: Some(on),
            hold: None,
            reset: None,
        })
    }

    pub fn reset() -> String {
        write(&RenderRequest {
            headless: None,
            hold: None,
            reset: Some(true),
        })
    }
}

fn write<T: Serialize>(request: &T) -> String {
    serde_json::to_string(request).unwrap_or_else(|_| "{}".to_owned())
}

/// `GET /state` — the whole snapshot. Only the fields the editor paints are modelled; everything
/// else the mod sends is ignored.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct StateResponse {
    #[serde(default)]
    pub mission_id: i32,

    #[serde(default)]
    pub mission_name: Option<String>,

    #[serde(default)]
    pub menu_status: Option<String>,

    #[serde(default)]
    pub fps: f32,

    #[serde(default)]
    pub script: Option<ScriptResponse>,

    #[serde(default)]
    pub dt: Option<DtSnapshot>,

    #[serde(default)]
    pub rng_pin: Option<String>,

    #[serde(default)]
    pub rng_seed: u32,

    #[serde(default)]
    pub fps_cap: Option<FpsCapSnapshot>,

    #[serde(default)]
    pub render: Option<RenderSnapshot>,
}

/// The script the mod is about: the running one, or the last one it ran.
///
/// `status` is a word, not a number (`ScriptStatus` is `#[serde(rename_all = "lowercase")]`), so
/// it is read as a string and mapped — a value the editor does not know must not make the whole
/// snapshot unreadable.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ScriptResponse {
    #[serde(default)]
    pub id: u32,

    #[serde(default)]
    pub name: Option<String>,

    #[serde(default)]
    pub status: Option<String>,

    #[serde(default)]
    pub frame: u32,

    #[serde(default)]
    pub total_frames: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct DtSnapshot {
    #[serde(default)]
    pub fixed: bool,

    #[serde(default)]
    pub fixed_ms: f32,

    #[serde(default)]
    pub frame_ms: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FpsCapSnapshot {
    #[serde(default)]
    pub cap: Option<String>,

    /// `None` means there is no cap — a real state, not a missing field.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct RenderSnapshot {
    #[serde(default)]
    pub headless: bool,

    #[serde(default)]
    pub hold: bool,
}

/// `POST /script/run` — what the mod answered with: the id the status is read by, and whether it
/// is already running or armed on its trigger.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RunResponse {
    #[serde(default)]
    pub script_id: u32,

    #[serde(default)]
    pub name: Option<String>,

    #[serde(default)]
    pub total_frames: u32,

    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ErrorResponse {
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Serialize)]
struct DtRequest {
    fixed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ms: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticks: Option<bool>,
}

#[derive(Serialize)]
struct FpsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    cap: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fps: Option<u32>,
}

#[derive(Serialize)]
struct RngRequest {
    pin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u32>,
}

#[derive(Serialize)]
struct RenderRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    headless: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reset: Option<bool>,
}

/// `GET /state`.
pub fn read_state(payload: &str) -> Option<StateResponse> {
    serde_json::from_str(payload).ok()
}

/// `POST /script/run`.
pub fn read_run(payload: &str) -> Option<RunResponse> {
    serde_json::from_str(payload).ok()
}

/// `{ "error": "..." }` — the mod's own wording for a refusal, which already names the field or
/// the reason, so the editor paints it rather than inventing one.
///
/// `None` when the body is not an error at all (or is not JSON) — the caller falls back to the
/// HTTP code.
pub fn read_error(payload: &str) -> Option<String> {
    serde_json::from_str::<ErrorResponse>(payload)
        .ok()
        .and_then(|error| error.error)
}
