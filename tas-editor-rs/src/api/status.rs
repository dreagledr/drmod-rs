//! What the editor knows about the game right now: the state it polls, not the state it sends.
//!
//! This is the reader's half of the API — [`super::rules::PlaybackRules`] is the writer's. It
//! carries only what the status line and the buttons are decided by: the snapshot the mod keeps
//! answering with is much larger (player, camera, frame ring), and none of that belongs in a
//! control panel.

use super::json::StateResponse;

/// What the editor knows about the game right now.
#[derive(Clone, Debug, PartialEq)]
pub struct GameStatus {
    pub online: bool,
    pub menu_status: String,
    pub mission_id: i32,
    pub mission_name: String,
    pub script: Option<GameScript>,
    pub fps: f32,
    pub fixed_tick: bool,
    pub cap_limit: Option<u32>,
    pub rng_pin: String,
    pub rng_seed: u32,
    pub headless: bool,
}

impl GameStatus {
    /// The menu status that means the game is being played — the one state `Run` requires, and the
    /// string the mod's own `GameMenuStatus::name()` produces.
    pub const IN_GAME: &'static str = "In Game";

    /// No answer from the API: nothing is injected, or the game is not running at all. The editor
    /// keeps working (the workspace is on disk) — only the run controls go quiet.
    pub fn offline() -> Self {
        Self {
            online: false,
            menu_status: "—".to_owned(),
            mission_id: 0,
            mission_name: String::new(),
            script: None,
            fps: 0.0,
            fixed_tick: false,
            cap_limit: None,
            rng_pin: "off".to_owned(),
            rng_seed: 0,
            headless: false,
        }
    }

    /// The mod's `/state` as the panel's model. A snapshot that misses a sub-object is answered
    /// for with the mod's own defaults rather than refused: half a status line beats a blank panel.
    pub fn of(state: &StateResponse) -> Self {
        Self {
            online: true,
            menu_status: state.menu_status.clone().unwrap_or_else(|| "—".to_owned()),
            mission_id: state.mission_id,
            mission_name: state.mission_name.clone().unwrap_or_default(),
            script: state.script.as_ref().map(GameScript::of),
            fps: state.fps,
            fixed_tick: state.dt.is_some_and(|dt| dt.fixed),
            cap_limit: state.fps_cap.as_ref().and_then(|cap| cap.limit),
            rng_pin: state.rng_pin.clone().unwrap_or_else(|| "off".to_owned()),
            rng_seed: state.rng_seed,
            headless: state.render.is_some_and(|render| render.headless),
        }
    }

    /// Whether the game is in gameplay, which is what a run needs: the script's own restart plays
    /// the pause menu, and a menu already open would swallow those keys.
    pub fn in_gameplay(&self) -> bool {
        self.online && self.menu_status == Self::IN_GAME
    }

    /// Whether a script holds the mod's one slot — the same set the mod refuses a second run with
    /// (`is_active` in `src/api.rs`). Cancel is live exactly while this is true, whoever started it.
    pub fn script_active(&self) -> bool {
        self.script.as_ref().is_some_and(|script| script.active())
    }
}

/// The script slot, as the status line reads it. `active` is the mod's own `is_active` set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameScript {
    pub id: u32,
    pub name: String,
    pub phase: GameScriptPhase,
    pub frame: u32,
    pub total_frames: u32,
}

impl GameScript {
    pub fn active(&self) -> bool {
        matches!(
            self.phase,
            GameScriptPhase::Restarting | GameScriptPhase::Armed | GameScriptPhase::Running
        )
    }

    pub fn of(script: &super::json::ScriptResponse) -> Self {
        Self {
            id: script.id,
            name: script.name.clone().unwrap_or_default(),
            phase: GameScriptPhase::read(script.status.as_deref()),
            frame: script.frame,
            total_frames: script.total_frames,
        }
    }
}

/// The phase of the mod's one script slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameScriptPhase {
    Unknown,
    Restarting,
    Armed,
    Running,
    Done,
    Stopped,
}

impl GameScriptPhase {
    /// The word the mod sent, as a value. An unknown word is [`Self::Unknown`] rather than a
    /// failure: a new status on the mod's side must not blind the whole panel.
    pub fn read(status: Option<&str>) -> Self {
        match status {
            Some("restarting") => Self::Restarting,
            Some("armed") => Self::Armed,
            Some("running") => Self::Running,
            Some("done") => Self::Done,
            Some("stopped") => Self::Stopped,
            _ => Self::Unknown,
        }
    }

    /// The lower-case word the mod spells it with.
    pub fn word(self) -> &'static str {
        match self {
            Self::Restarting => "restarting",
            Self::Armed => "armed",
            Self::Running => "running",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }
}
