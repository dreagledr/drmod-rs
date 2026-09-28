//! The text converter's view of a document: one [`CommandRow`] per frame, in the
//! angle-plus-deflection shape the converter collapses back into commands.
//!
//! The command table does not use this — it shows [`super::projection::ScriptFrame`], the DSL's
//! own terms. This is the intermediate the text writer goes through.
//!
//! Only frame-level inputs project: `raw_key`, `dik_key` and `when_enemy` have no column, so
//! commands carrying them are left out — and a document collapsed back from the frames loses
//! them. The tests pin that down instead of hiding it.
//!
//! The projection is lossy by nature, which is why the JSON stays the source of truth: a
//! documented stick arrives as an angle plus a deflection, and the axes come back from the
//! trigonometry within a few thousandths.

use super::model::{ScriptCommand, ScriptDocument, ScriptInput};
use super::keys::{CommandKeys, command_mask};

/// Raw axis units per deflection unit, as in the DSL: `1.0` is a full axis, `√2` the corner —
/// the JSON's own `[-1000, 1000]`.
const AXIS_UNIT: f64 = 1000.0;

/// Where a stick that is at rest points: a released stick keeps its last direction, so a run of
/// frames does not snap to zero between two pushes. A full push forward is the angle 270 in this
/// shape (0 in the DSL's compass).
const REST_ANGLE: f64 = 270.0;

/// Axes closer than this are the same value: `cos 270°` is 6·10⁻¹⁷, which is a direction only to
/// a mathematician.
const AXIS_EPSILON: f64 = 1e-6;

/// One frame of a script in the converter's own shape: each stick is a direction in degrees plus
/// a deflection magnitude, and every other input is a boolean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandRow {
    pub frame: i32,
    pub left_stick_angle: f64,
    pub right_stick_angle: f64,
    pub left_stick_amount: f64,
    pub right_stick_amount: f64,
    pub buttons: u32,
}

impl CommandRow {
    /// Whether the input at `bit` — an index into [`CommandKeys::ALL`] — is held on this frame.
    pub fn holds(&self, bit: usize) -> bool {
        self.buttons & (1u32 << bit) != 0
    }
}

/// The frames of a document: 0 up to the end of its last projectable command.
///
/// Commands keep the order they appear in, so a stick held by two of them is the later one — the
/// document's own last-wins rule.
pub fn expand(document: &ScriptDocument) -> Vec<CommandRow> {
    let commands: Vec<&ScriptCommand> = document
        .commands
        .iter()
        .filter(|command| {
            command.when_enemy.is_none()
                && command.input.raw_key.is_none()
                && command.input.dik_key.is_none()
        })
        .collect();

    let total = commands
        .iter()
        .map(|command| command.t + command.duration)
        .max()
        .unwrap_or(0);

    let mut rows = Vec::with_capacity(total as usize);
    let mut left_angle = REST_ANGLE;
    let mut right_angle = 0.0;

    for frame in 0..total {
        let mut buttons = 0u32;
        let mut left: Option<[f64; 2]> = None;
        let mut right: Option<[f64; 2]> = None;

        for command in &commands {
            if frame < command.t || frame >= command.t + command.duration {
                continue;
            }

            buttons |= bits(&command.input);

            // The last command to set the stick is what the game sees: the mod assigns it per
            // command, so a later command's directions replace an earlier command's explicit
            // stick, and back.
            if let Some(commanded) = commanded_stick(&command.input) {
                left = Some(commanded);
            }

            if let Some(camera) = command.input.camera {
                right = Some([f64::from(camera[0]), f64::from(camera[1])]);
            }
        }

        // Walking is a magnitude the game reads rather than a key: the mod halves the stick it
        // ended up with, however it got there.
        if buttons & command_mask(&["walk"]) != 0
            && let Some(axes) = left
        {
            left = Some([axes[0] / 2.0, axes[1] / 2.0]);
        }

        let (next_left, left_amount) = polar(left.unwrap_or([0.0, 0.0]));
        let (next_right, right_amount) = polar(right.unwrap_or([0.0, 0.0]));
        if left_amount > 0.0 {
            left_angle = next_left;
        }

        if right_amount > 0.0 {
            right_angle = next_right;
        }

        rows.push(CommandRow {
            frame: frame as i32,
            left_stick_angle: left_angle,
            right_stick_angle: right_angle,
            left_stick_amount: left_amount,
            right_stick_amount: right_amount,
            buttons,
        });
    }

    rows
}

/// The frames back to commands: one command per run of identical frames, and no command for a
/// run where nothing is held — the mod's own reading of a frame without input.
pub fn collapse(frames: &[CommandRow]) -> Vec<ScriptCommand> {
    let mut commands = Vec::new();
    let mut start = 0usize;

    while start < frames.len() {
        if empty(&frames[start]) {
            start += 1;
            continue;
        }

        let mut end = start;
        while end + 1 < frames.len()
            && !empty(&frames[end + 1])
            && same_row(&frames[end], &frames[end + 1])
        {
            end += 1;
        }

        commands.push(command(
            &frames[start],
            frames[start].frame as u32,
            (frames[end].frame - frames[start].frame + 1) as u32,
        ));
        start = end + 1;
    }

    commands
}

/// One run of frames as a command: its booleans, a stick only where the directions do not imply
/// it, and a camera only while the stick is off centre.
fn command(row: &CommandRow, t: u32, duration: u32) -> ScriptCommand {
    let mut input = ScriptInput::default();
    for (bit, key) in CommandKeys::ALL.iter().enumerate() {
        if row.holds(bit) {
            input = (key.set)(input);
        }
    }

    let left = vector(row.left_stick_angle, row.left_stick_amount);
    if !same_axes(left, implied(&input)) {
        input.left_stick = Some(rounded(left));
    }

    if row.right_stick_amount > 0.0 {
        input.camera = Some(rounded(vector(row.right_stick_angle, row.right_stick_amount)));
    }

    ScriptCommand {
        t,
        duration,
        input,
        when_enemy: None,
    }
}

/// The bit field of an input, one bit per command-table column.
fn bits(input: &ScriptInput) -> u32 {
    let mut buttons = 0u32;
    for (bit, key) in CommandKeys::ALL.iter().enumerate() {
        if (key.get)(input) {
            buttons |= 1u32 << bit;
        }
    }

    buttons
}

/// The stick a command feeds the game **before** walking halves it: the explicit one, or the one
/// its movement flags stand for.
///
/// `None` when the command says nothing about the stick — which is not the same as a stick at
/// rest, since a command that says nothing leaves the previous value standing.
pub fn commanded_stick(input: &ScriptInput) -> Option<[f64; 2]> {
    match input.left_stick {
        Some(stick) => Some([f64::from(stick[0]), f64::from(stick[1])]),
        None => directions(input),
    }
}

/// The stick one command's movement flags stand for, as the mod assembles it: each direction
/// adds a full axis (`forward` is `(0, −1000)`) and diagonals add up.
///
/// `None` when the command moves nowhere; `walk` is not part of it — the mod halves the finished
/// stick instead.
fn directions(input: &ScriptInput) -> Option<[f64; 2]> {
    let mut x = 0.0;
    let mut y = 0.0;
    if input.forward {
        y -= AXIS_UNIT;
    }

    if input.backward {
        y += AXIS_UNIT;
    }

    if input.left {
        x -= AXIS_UNIT;
    }

    if input.right {
        x += AXIS_UNIT;
    }

    if x == 0.0 && y == 0.0 {
        None
    } else {
        Some([x, y])
    }
}

/// The stick a command ends up with when it carries nothing explicit — its own directions,
/// halved by `walk` as the mod halves the finished stick.
fn implied(input: &ScriptInput) -> [f64; 2] {
    let axes = directions(input).unwrap_or([0.0, 0.0]);
    if input.walk {
        [axes[0] / 2.0, axes[1] / 2.0]
    } else {
        axes
    }
}

/// Axis values as the table's pair: an angle in degrees from +X and a deflection, where 1 is a
/// full axis.
fn polar(axes: [f64; 2]) -> (f64, f64) {
    let mut angle = axes[1].atan2(axes[0]) * 180.0 / std::f64::consts::PI;
    if angle < 0.0 {
        angle += 360.0;
    }

    let length = (axes[0] * axes[0] + axes[1] * axes[1]).sqrt() / AXIS_UNIT;
    ((angle * 1000.0).round() / 1000.0, length)
}

/// The table's pair back to axis values.
fn vector(angle: f64, deflection: f64) -> [f64; 2] {
    let radians = angle * std::f64::consts::PI / 180.0;
    [
        AXIS_UNIT * deflection * radians.cos(),
        AXIS_UNIT * deflection * radians.sin(),
    ]
}

/// Axis values rounded for a document, with the `-0` of a 90° cosine snapped away — it would
/// otherwise read as a direction.
fn rounded(axes: [f64; 2]) -> [f32; 2] {
    [
        ((axes[0] * 1000.0).round() / 1000.0 + 0.0) as f32,
        ((axes[1] * 1000.0).round() / 1000.0 + 0.0) as f32,
    ]
}

/// Whether two frames hold the same thing — what makes a run a run. Axes compare with a
/// tolerance: an angle and a deflection that describe the same direction must not split a run
/// over 10⁻¹³.
fn same_row(a: &CommandRow, b: &CommandRow) -> bool {
    a.buttons == b.buttons
        && same_axes(
            vector(a.left_stick_angle, a.left_stick_amount),
            vector(b.left_stick_angle, b.left_stick_amount),
        )
        && same_axes(
            vector(a.right_stick_angle, a.right_stick_amount),
            vector(b.right_stick_angle, b.right_stick_amount),
        )
}

fn same_axes(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < AXIS_EPSILON && (a[1] - b[1]).abs() < AXIS_EPSILON
}

fn empty(row: &CommandRow) -> bool {
    row.buttons == 0 && row.left_stick_amount == 0.0 && row.right_stick_amount == 0.0
}
