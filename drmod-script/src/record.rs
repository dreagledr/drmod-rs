//! A recorded run of frames back into a script document — the way a `.tas` is grown from play
//! instead of typed.
//!
//! Two recorders feed this: the mod's log ring (`GET /logs`, one `InputUnit` per render frame) and
//! the replay SQLite database (`drmod-dbdump`). Both hold the same fact — what the game was fed —
//! so the conversion lives here once instead of in each tool.
//!
//! The conversion is lossy by construction, and the losses are the format's own:
//!
//! * `buttons_released`/`buttons_alternated` are not reproduced — a script feeds down/pressed, and
//!   a release is the implicit end of a command;
//! * keybind actions that never reach `InputUnit` (`ripper`, `lock_on`, `item`, `codec`, `dodge`,
//!   `camera_reset`, `zandatsu`) can only be captured if the recorder kept them separately (the
//!   replay database does; the log ring does not);
//! * the D-pad bits are shared between gameplay and menus (`0x1` weapon-select vs menu-left, `0x8`
//!   augment vs menu-up, `0x10` jump vs confirm). The context is not guessed: it is read from the
//!   frame's menu status, which the recorder has. `menu_status_raw == 1` (`InGame`) is gameplay;
//!   every other status is read as a menu — the same call the mod makes when it feeds a script
//!   (`drmod-core/src/api.rs`, `in_weapon_menu`).

use drmod_replay_types::{InputUnit, input_bits};

use crate::error::{ScriptFormatException, ScriptResult};
use crate::model::{DEFAULT_NAME, ScriptCommand, ScriptDocument, ScriptInput, ScriptTrigger};

/// `GameMenuStatus::InGame` (`drmod-core/src/game/mod.rs`). Only this status is gameplay; every
/// other one is read with the menu meaning of the shared D-pad bits.
pub const MENU_IN_GAME: i32 = 1;

/// One frame of a recording, as the converter needs it.
///
/// `frame_index` is the frame the command's `t` is written from; the log ring renumbers from zero
/// before calling, so a gap in the recorder's own counter does not become a gap in the script.
/// `pos` is the player's position on that frame — the first one arms the script's start trigger.
#[derive(Clone, Copy, Debug)]
pub struct RecordFrame {
    pub frame_index: u32,
    pub input: InputUnit,
    /// The keybind edge the game's `InputUnit` does not carry (the replay database records it,
    /// the log ring leaves it `false`).
    pub ripper: bool,
    /// The keybinds the game was seen reading on this frame, as bitmasks indexed by
    /// [`keybind`]: `down` is held, `pressed` is the edge. These actions (`lock_on`, `item`,
    /// `codec`, `camera_reset`, `zandatsu`, `dodge`) never reach `InputUnit`, so this is the only
    /// way the log ring can carry them back into a script. The replay database leaves both `0`.
    pub keybind_down: u32,
    pub keybind_pressed: u32,
    pub pos: [f32; 3],
    /// The raw `GameMenuStatus` of the frame; [`MENU_IN_GAME`] means gameplay.
    pub menu_status_raw: i32,
}

/// The game's keybind indices (`eSaveKeybind`, `drmod-core/src/tas/addresses.rs`), shared so the
/// mod's per-frame keybind bitmasks and this converter agree on which bit is which action.
pub mod keybind {
    pub const RIPPERMODE: u32 = 11;
    pub const SWITCH_LOCK_ON: u32 = 12;
    pub const USE_SUBWEAPON: u32 = 13;
    pub const USE_ITEM: u32 = 14;
    pub const CODEC_SCREEN: u32 = 17;
    pub const CAMERA_RESET: u32 = 19;
    pub const EXECUTION: u32 = 20;
    pub const DEFFENSIVE_OFFENSIVE: u32 = 21;

    /// The bit for one keybind index.
    pub const fn bit(index: u32) -> u32 {
        1 << index
    }
}

/// The frames as a document: commands merged from runs of identical input, armed on the first
/// frame's position. The name is the format's default — the caller names it.
pub fn document(frames: &[RecordFrame]) -> ScriptResult<ScriptDocument> {
    let trigger_pos = frames
        .first()
        .map(|frame| frame.pos)
        .ok_or_else(|| ScriptFormatException::new("no frames to convert"))?;

    let commands = commands(frames);
    if commands.is_empty() {
        return Err(ScriptFormatException::new("the recording holds no input"));
    }

    let document = ScriptDocument {
        name: DEFAULT_NAME.to_owned(),
        trigger: Some(ScriptTrigger {
            pos: Some(trigger_pos),
            ticks: None,
        }),
        restart: None,
        commands,
    };

    // The same limits the mod enforces, so a recording that would be refused as JSON is refused
    // here first.
    crate::json::validate(&document)?;
    Ok(document)
}

/// The frames as the canonical `.tas` text.
pub fn to_text(frames: &[RecordFrame]) -> ScriptResult<String> {
    crate::dsl::write(&document(frames)?)
}

/// The frames as the mod's API JSON (`POST /script/run` body).
pub fn to_json(frames: &[RecordFrame]) -> ScriptResult<String> {
    crate::json::write(&document(frames)?)
}

/// The stick the movement flags stand for: forward→(0,−1000), backward→(0,1000), left→(−1000,0),
/// right→(1000,0), diagonals summed — the same assembly the mod's own script runner does.
fn implied_stick(input: &ScriptInput) -> [f32; 2] {
    let mut stick = [0.0f32, 0.0];
    if input.forward {
        stick[1] -= 1000.0;
    }
    if input.backward {
        stick[1] += 1000.0;
    }
    if input.left {
        stick[0] -= 1000.0;
    }
    if input.right {
        stick[0] += 1000.0;
    }
    stick
}

/// Decodes one frame's `InputUnit` into a script input. `None` for a frame with no input — such
/// frames are skipped.
///
/// The left stick is written explicitly only when it differs from the one the movement flags imply
/// (in a recording `FORWARD|RIGHT` often carries `(0,−1000)`, not the `(1000,−1000)` the flags
/// would rebuild), so the movement survives the round trip.
pub fn decode(frame: &RecordFrame) -> Option<ScriptInput> {
    let down = frame.input.buttons_down;
    let gameplay = frame.menu_status_raw == MENU_IN_GAME;

    let mut input = ScriptInput {
        forward: down & input_bits::FORWARD != 0,
        backward: down & input_bits::BACK != 0,
        left: down & input_bits::LEFT != 0,
        right: down & input_bits::RIGHT != 0,
        light_attack: down & input_bits::LIGHT_ATTACK != 0,
        heavy_attack: down & input_bits::HEAVY_ATTACK != 0,
        ninja_run: down & input_bits::NINJA_RUN != 0,
        blade: down & input_bits::BLADE != 0,
        subweapon: down & input_bits::SUBWEAPON != 0,
        pause: down & input_bits::PAUSE != 0,
        ripper: frame.ripper,
        ..Default::default()
    };

    // The shared D-pad bits, read in the context the frame's menu status puts them in.
    if gameplay {
        input.jump = down & input_bits::JUMP != 0;
        input.ar_mode = down & input_bits::AR_MODE != 0;
        input.weapon_select = down & input_bits::WEAPON_SELECT != 0;
    } else {
        input.menu_left = down & input_bits::MENU_LEFT != 0;
        input.menu_right = down & input_bits::MENU_RIGHT != 0;
        input.menu_down = down & input_bits::MENU_DOWN != 0;
        input.menu_up = down & input_bits::MENU_UP != 0;
        input.confirm = down & input_bits::CONFIRM != 0;
    }

    // The keybind actions `InputUnit` does not carry. `ripper` is a toggle edge; the rest are the
    // mod's hold actions, so they are read from the held bit (a tap held across a run becomes one
    // command with its own duration, which is what the mod's own runner feeds).
    let held = frame.keybind_down;
    let pressed = frame.keybind_pressed;
    input.ripper |= pressed & keybind::bit(keybind::RIPPERMODE) != 0;
    input.lock_on = held & keybind::bit(keybind::SWITCH_LOCK_ON) != 0;
    input.item = held & keybind::bit(keybind::USE_ITEM) != 0;
    input.camera_reset = held & keybind::bit(keybind::CAMERA_RESET) != 0;
    input.zandatsu = held & keybind::bit(keybind::EXECUTION) != 0;
    input.dodge = held & keybind::bit(keybind::DEFFENSIVE_OFFENSIVE) != 0;
    input.codec = (held & keybind::bit(keybind::CODEC_SCREEN) != 0)
        || (pressed & keybind::bit(keybind::CODEC_SCREEN) != 0);
    // `subweapon` has an `InputUnit` bit as well; the keybind is the same action, so it is OR-ed in.
    input.subweapon |= held & keybind::bit(keybind::USE_SUBWEAPON) != 0;

    if frame.input.left_stick != implied_stick(&input) {
        input.left_stick = Some(frame.input.left_stick);
    }
    if frame.input.right_stick != [0.0, 0.0] {
        input.camera = Some(frame.input.right_stick);
    }

    if input.is_empty() { None } else { Some(input) }
}

/// The frames as commands: a run of identical inputs becomes one command, `ripper` stays a
/// one-frame command of its own (it is a keybind edge), and empty frames are skipped.
pub fn commands(frames: &[RecordFrame]) -> Vec<ScriptCommand> {
    let mut commands = Vec::new();
    let mut i = 0;
    while i < frames.len() {
        let Some(input) = decode(&frames[i]) else {
            i += 1;
            continue;
        };

        if input.ripper {
            commands.push(ScriptCommand {
                t: frames[i].frame_index,
                duration: 1,
                input,
                when_enemy: None,
            });
            i += 1;
            continue;
        }

        let start = i;
        while i + 1 < frames.len() && decode(&frames[i + 1]) == Some(input) {
            i += 1;
        }
        commands.push(ScriptCommand {
            t: frames[start].frame_index,
            duration: frames[i].frame_index - frames[start].frame_index + 1,
            input,
            when_enemy: None,
        });
        i += 1;
    }

    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(down: u32, stick: [f32; 2]) -> InputUnit {
        InputUnit {
            buttons_down: down,
            left_stick: stick,
            ..Default::default()
        }
    }

    fn frame(index: u32, input: InputUnit, menu: i32) -> RecordFrame {
        RecordFrame {
            frame_index: index,
            input,
            ripper: false,
            keybind_down: 0,
            keybind_pressed: 0,
            pos: [0.0, 0.0, 0.0],
            menu_status_raw: menu,
        }
    }

    #[test]
    fn keybind_actions_reach_the_input() {
        let held = keybind::bit(keybind::SWITCH_LOCK_ON)
            | keybind::bit(keybind::USE_ITEM)
            | keybind::bit(keybind::CAMERA_RESET)
            | keybind::bit(keybind::EXECUTION)
            | keybind::bit(keybind::DEFFENSIVE_OFFENSIVE)
            | keybind::bit(keybind::USE_SUBWEAPON);
        let pressed = keybind::bit(keybind::RIPPERMODE) | keybind::bit(keybind::CODEC_SCREEN);

        let frames = [RecordFrame {
            frame_index: 0,
            input: InputUnit::default(),
            ripper: false,
            keybind_down: held,
            keybind_pressed: pressed,
            pos: [0.0, 0.0, 0.0],
            menu_status_raw: MENU_IN_GAME,
        }];
        let input = decode(&frames[0]).expect("input");
        assert!(input.ripper && input.lock_on && input.item && input.camera_reset);
        assert!(input.zandatsu && input.dodge && input.codec && input.subweapon);
    }

    #[test]
    fn gameplay_bits_stay_gameplay() {
        let frames = [frame(
            0,
            unit(input_bits::WEAPON_SELECT | input_bits::AR_MODE | input_bits::JUMP, [0.0, 0.0]),
            MENU_IN_GAME,
        )];
        let input = decode(&frames[0]).expect("input");
        assert!(input.weapon_select && input.ar_mode && input.jump);
        assert!(!input.menu_left && !input.menu_up && !input.confirm);
    }

    #[test]
    fn menu_bits_are_read_as_menu() {
        let frames = [frame(
            0,
            unit(
                input_bits::MENU_LEFT | input_bits::MENU_UP | input_bits::CONFIRM,
                [0.0, 0.0],
            ),
            9, // Select Weapon Menu
        )];
        let input = decode(&frames[0]).expect("input");
        assert!(input.menu_left && input.menu_up && input.confirm);
        assert!(!input.weapon_select && !input.ar_mode && !input.jump);
    }

    #[test]
    fn direction_flags_build_the_stick_only_when_it_differs() {
        // A plain forward run: the flag implies (0,−1000), so no explicit stick is written.
        let plain = frame(
            0,
            unit(input_bits::FORWARD, [0.0, -1000.0]),
            MENU_IN_GAME,
        );
        assert_eq!(decode(&plain).expect("input").left_stick, None);

        // Forward+right with a straight forward stick: the flags would imply the diagonal, so the
        // recorded stick is kept.
        let diagonal = frame(
            0,
            unit(input_bits::FORWARD | input_bits::RIGHT, [0.0, -1000.0]),
            MENU_IN_GAME,
        );
        assert_eq!(
            decode(&diagonal).expect("input").left_stick,
            Some([0.0, -1000.0])
        );
    }

    #[test]
    fn identical_runs_merge_and_empty_frames_drop() {
        let forward = unit(input_bits::FORWARD, [0.0, -1000.0]);
        let frames = [
            frame(0, InputUnit::default(), MENU_IN_GAME),
            frame(1, forward, MENU_IN_GAME),
            frame(2, forward, MENU_IN_GAME),
            frame(3, InputUnit::default(), MENU_IN_GAME),
            frame(4, unit(input_bits::JUMP, [0.0, 0.0]), MENU_IN_GAME),
        ];
        let commands = commands(&frames);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].t, 1);
        assert_eq!(commands[0].duration, 2);
        assert_eq!(commands[1].t, 4);
    }
}
