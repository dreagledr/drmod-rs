//! The script model: a script as the editor holds it — the whole body of `POST /script/run`.
//!
//! This is the hub between the three representations: the DSL text (`dsl`), the API JSON
//! (`json`) and the command table's frames (`projection`). Only the JSON carries the whole
//! format — `raw_key`, `dik_key` and `when_enemy` have no text spelling and no table column.
//!
//! Field names, order and defaults mirror the mod's own DTO (`replay-types/src/script.rs`),
//! because that is what the editor hands to the game: a value the mod would refuse must be
//! refused here first.

use serde::{Deserialize, Serialize};

/// The `name` the mod falls back to when the JSON omits it (`default_script_name` in
/// `src/api.rs`).
pub const DEFAULT_NAME: &str = "script";

/// Mirrors `MAX_SCRIPT_FRAMES` in the mod — an upper guard (~4.6 h at 60 FPS), not the
/// practical limit: what really bounds a script is the request body size.
pub const MAX_SCRIPT_FRAMES: u32 = 1_000_000;

/// Mirrors the mod's `name too long (max 64)` check.
pub const MAX_NAME_LENGTH: usize = 64;

/// A script as the editor holds it.
///
/// `deny_unknown_fields` mirrors the mod: a typo like `light_attackk` must not pass silently
/// here either.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ScriptDocument {
    /// The mod's own fallback applies when the field is absent.
    #[serde(default = "default_name")]
    pub name: String,

    /// Arms the script (starts it on a position or a tick count) instead of running it at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<ScriptTrigger>,

    /// Plays the pause menu and restarts the mission before arming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<RestartSpec>,

    #[serde(default)]
    pub commands: Vec<ScriptCommand>,
}

fn default_name() -> String {
    DEFAULT_NAME.to_owned()
}

/// `trigger`: where — or after how many simulation ticks — an armed script starts.
///
/// At least one of the two is required; that cross-field limit is checked by `json::validate`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ScriptTrigger {
    /// The player's spawn area: ±0.1 m on X/Z and ±1.0 m on Y — the tolerance record/playback
    /// uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 3]>,

    /// Simulation ticks of gameplay after arming — the reproducible start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks: Option<u64>,
}

/// One `commands[]` entry: its inputs are held from frame `t` for `duration` frames.
///
/// The frame is a **simulation tick** — the mod feeds the inputs from its `updateInputUnit`
/// detour — so timings do not drift with the frame rate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptCommand {
    pub t: u32,
    pub duration: u32,
    pub input: ScriptInput,

    /// The command fires on the first tick its enemy condition holds, with `t` as the fallback.
    ///
    /// Only the JSON carries it: the DSL has no spelling for a condition and the command table
    /// has no column for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_enemy: Option<EnemyCondition>,
}

/// The `input` object: the flags of the script format, the two sticks and the two raw key codes.
///
/// Every field is `skip_serializing_if` its own default: an unset flag and a `false` one mean
/// the same to the mod as an absent key, so writing them would bury the one line that matters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ScriptInput {
    #[serde(default, skip_serializing_if = "is_false")]
    pub forward: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub backward: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub left: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub right: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub jump: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub light_attack: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub heavy_attack: bool,
    /// Camera turn of the command, fed as the right stick each frame (mouse deltas, hundreds to
    /// thousands).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ripper: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub blade: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ninja_run: bool,
    /// Walking: the game encodes it by stick magnitude, so this halves the implied stick
    /// instead of pressing anything.
    #[serde(default, skip_serializing_if = "is_false")]
    pub walk: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dodge: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub lock_on: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub subweapon: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub item: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ar_mode: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub weapon_select: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub codec: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub zandatsu: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub camera_reset: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pause: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub confirm: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_up: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_down: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_left: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_right: bool,
    /// Raw game key code, delivered through the `isKeyDown`/`isKeyPressed` detours — the channel
    /// the pause menu reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_key: Option<u32>,
    /// Raw DirectInput DIK code, mixed into the device state after polling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dik_key: Option<u32>,
    /// An explicit stick position, overriding the one implied by the movement flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_stick: Option<[f32; 2]>,
}

impl ScriptInput {
    /// Whether any input is set at all. The mod rejects an empty `input`, so the editor has to
    /// as well.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// `when_enemy`: the enemy state that fires a command.
///
/// Every bound is optional and a missing one means "no limit".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EnemyCondition {
    /// Enemy animations (`Behavior + 0x618`) the command may fire in. Empty means any; known
    /// ids: 19 lunge, 65545 jump, 24 hit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anim: Vec<i32>,

    /// Enemy animation frame (`+0x8B4`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_min: Option<i32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_max: Option<i32>,

    /// Distance to the enemy in metres — the hit has to reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dist_max: Option<f32>,

    /// How far the enemy's **blade** is above the player.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blade_dy_min: Option<f32>,

    /// Player height: the hit must be a jump — only that launches — but a low one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_y_min: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_y_max: Option<f32>,

    /// Vertical speed: 0 fires only while falling — a hit on the way up does not launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_vy_max: Option<f32>,

    /// Fire again every `duration` frames while the condition holds.
    #[serde(default, skip_serializing_if = "is_false")]
    pub repeat: bool,
}

/// `restart`: the pause-menu sequence the mod plays before arming.
///
/// A parameter is optional on purpose, and `None` means "the mod's default" — which is also what
/// `Default` carries. The distinction is not cosmetic: it is what decides whether the text has
/// to spell a parameter out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartSpec {
    /// Menu steps up — in the pause menu Restart is the bottom entry.
    #[serde(default = "one", skip_serializing_if = "Option::is_none")]
    pub ups: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downs: Option<u32>,

    /// Frames an arrow key is held.
    #[serde(default = "six")]
    pub hold: Option<u32>,

    /// Pause after `pause`: the menu has to open first, or the arrow is lost in the animation.
    #[serde(default = "twenty")]
    pub open_gap: Option<u32>,

    /// Pause between the arrows and the confirmation.
    #[serde(default = "ten")]
    pub gap: Option<u32>,

    /// Confirmations in a row: the Restart entry, then the dialog's preselected YES.
    #[serde(default = "two")]
    pub confirms: Option<u32>,

    /// Pause between confirmations — the dialog has to appear.
    #[serde(default = "twenty_five")]
    pub confirm_gap: Option<u32>,

    /// Frames after the last confirmation.
    #[serde(default = "fifteen")]
    pub tail: Option<u32>,
}

impl Default for RestartSpec {
    fn default() -> Self {
        Self {
            ups: Some(1),
            downs: None,
            hold: Some(6),
            open_gap: Some(20),
            gap: Some(10),
            confirms: Some(2),
            confirm_gap: Some(25),
            tail: Some(15),
        }
    }
}

/// The serde fallbacks: a key the JSON leaves out takes the mod's own default, which is what
/// these answer with. They return `Option` because `None` is what the *text* means by "the mod's
/// default" — the two spellings of the same idea have to agree.
fn one() -> Option<u32> {
    Some(1)
}

fn two() -> Option<u32> {
    Some(2)
}

fn six() -> Option<u32> {
    Some(6)
}

fn ten() -> Option<u32> {
    Some(10)
}

fn fifteen() -> Option<u32> {
    Some(15)
}

fn twenty() -> Option<u32> {
    Some(20)
}

fn twenty_five() -> Option<u32> {
    Some(25)
}
