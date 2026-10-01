//! The script DSL: one line per frame, the whole file is the script.
//!
//! Tokens are the console pad's names (`a` jump, `x` light attack, `lt` blade, `du` augment …)
//! plus `ls`/`rs` for the sticks, and **movement lives in the stick**: `ls:<angle>` is a full
//! press on the compass (0 forward, 90 right, 180 back, 270 left), `lsx`/`lsy`/`rsx`/`rsy` give
//! exact axis values, and `wk` halves the left stick. A stick or a direction flag both reach the
//! same place — the stick alone drives the character — so a direction flag that a JSON script
//! carries is written out as the stick that flag stands for.
//!
//! [`write`] is canonical — one line per frame a command touches, tokens in a fixed order,
//! invariant numbers, a one-frame duration left out — which makes `write(parse(text))` the same
//! text again; the golden tests rely on that. [`parse`] is strict: an unknown token or attribute
//! is an error naming its 1-based line, never a silently skipped word — the same typo protection
//! the mod gets from `deny_unknown_fields`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::error::{ScriptFormatException, ScriptResult};
use super::frames::commanded_stick;
use super::json;
use super::model::{DEFAULT_NAME, RestartSpec, ScriptCommand, ScriptDocument, ScriptInput, ScriptTrigger};

/// The extension of a script text in the workspace.
pub const EXTENSION: &str = ".tas";

/// The rules line starts here and is the file's first non-comment line.
const RULES_MARK: char = '!';

/// A comment runs to the end of the line, alone or after a rule or a frame.
const COMMENT_MARK: char = '#';

/// The stick tokens: full press by angle, exact value by axis.
const LEFT_STICK: &str = "ls";
const LEFT_STICK_X: &str = "lsx";
const LEFT_STICK_Y: &str = "lsy";
const RIGHT_STICK: &str = "rs";
const RIGHT_STICK_X: &str = "rsx";
const RIGHT_STICK_Y: &str = "rsy";

/// One axis unit: a full press reaches ±1000, the pad's own range — so `ls:<angle>` and
/// `rs:<angle>` carry the same magnitude, and a faster camera (a mouse delta of a few thousand)
/// is spelled with `rsx`/`rsy`.
const AXIS_UNIT: f64 = 1000.0;

/// Axis values closer than half of the last decimal count as zero — `sin 0°` is not exactly 0,
/// and a `-0` would read as a direction.
const AXIS_STEP: f64 = 0.001;

/// How close to ±1000 a stick has to be to be written as an angle instead of two axes.
const FULL_PRESS_TOLERANCE: f64 = AXIS_STEP;

/// One `input` field of the format: how to read it, how to set it, and the token that spells it
/// in the text — `None` for the four movement flags, which have no token because the DSL moves
/// with the stick.
struct Input {
    key: &'static str,
    get: fn(&ScriptInput) -> bool,
    set: fn(ScriptInput) -> ScriptInput,
    token: Option<&'static str>,
}

/// The inputs in the order the command table's columns have them, which is also the order a
/// frame line writes its tokens in.
///
/// The tokens are not a rename of those columns: a column can share its token with another
/// (`dr` is the inventory switch the column calls `dl`) and one token can stand for two columns
/// (`by` is Y+B, the game's Execute prompt).
const INPUTS: [Input; 26] = [
    Input { key: "forward", get: |i| i.forward, set: |i| ScriptInput { forward: true, ..i }, token: None },
    Input { key: "backward", get: |i| i.backward, set: |i| ScriptInput { backward: true, ..i }, token: None },
    Input { key: "left", get: |i| i.left, set: |i| ScriptInput { left: true, ..i }, token: None },
    Input { key: "right", get: |i| i.right, set: |i| ScriptInput { right: true, ..i }, token: None },
    Input { key: "jump", get: |i| i.jump, set: |i| ScriptInput { jump: true, ..i }, token: Some("a") },
    Input { key: "light_attack", get: |i| i.light_attack, set: |i| ScriptInput { light_attack: true, ..i }, token: Some("x") },
    Input { key: "heavy_attack", get: |i| i.heavy_attack, set: |i| ScriptInput { heavy_attack: true, ..i }, token: Some("y") },
    Input { key: "ripper", get: |i| i.ripper, set: |i| ScriptInput { ripper: true, ..i }, token: Some("lr") },
    Input { key: "blade", get: |i| i.blade, set: |i| ScriptInput { blade: true, ..i }, token: Some("lt") },
    Input { key: "ninja_run", get: |i| i.ninja_run, set: |i| ScriptInput { ninja_run: true, ..i }, token: Some("rt") },
    Input { key: "walk", get: |i| i.walk, set: |i| ScriptInput { walk: true, ..i }, token: Some("wk") },
    Input { key: "dodge", get: |i| i.dodge, set: |i| ScriptInput { dodge: true, ..i }, token: Some("ax") },
    Input { key: "lock_on", get: |i| i.lock_on, set: |i| ScriptInput { lock_on: true, ..i }, token: Some("rb") },
    Input { key: "subweapon", get: |i| i.subweapon, set: |i| ScriptInput { subweapon: true, ..i }, token: Some("lb") },
    Input { key: "item", get: |i| i.item, set: |i| ScriptInput { item: true, ..i }, token: Some("dd") },
    Input { key: "ar_mode", get: |i| i.ar_mode, set: |i| ScriptInput { ar_mode: true, ..i }, token: Some("du") },
    Input { key: "weapon_select", get: |i| i.weapon_select, set: |i| ScriptInput { weapon_select: true, ..i }, token: Some("dl") },
    Input { key: "codec", get: |i| i.codec, set: |i| ScriptInput { codec: true, ..i }, token: Some("cd") },
    Input { key: "zandatsu", get: |i| i.zandatsu, set: |i| ScriptInput { zandatsu: true, ..i }, token: Some("b") },
    Input { key: "camera_reset", get: |i| i.camera_reset, set: |i| ScriptInput { camera_reset: true, ..i }, token: Some("r") },
    Input { key: "pause", get: |i| i.pause, set: |i| ScriptInput { pause: true, ..i }, token: Some("esc") },
    Input { key: "confirm", get: |i| i.confirm, set: |i| ScriptInput { confirm: true, ..i }, token: Some("ok") },
    Input { key: "menu_up", get: |i| i.menu_up, set: |i| ScriptInput { menu_up: true, ..i }, token: Some("mu") },
    Input { key: "menu_down", get: |i| i.menu_down, set: |i| ScriptInput { menu_down: true, ..i }, token: Some("md") },
    Input { key: "menu_left", get: |i| i.menu_left, set: |i| ScriptInput { menu_left: true, ..i }, token: Some("ml") },
    Input { key: "menu_right", get: |i| i.menu_right, set: |i| ScriptInput { menu_right: true, ..i }, token: Some("mr") },
];

fn index_of(key: &str) -> Option<usize> {
    INPUTS.iter().position(|input| input.key == key)
}

fn heavy() -> usize {
    index_of("heavy_attack").expect("the heavy attack is an input")
}

fn zandatsu() -> usize {
    index_of("zandatsu").expect("zandatsu is an input")
}

/// Every token a frame line accepts, with the `input` key it sets — in the order the writer
/// spells them: the inputs in column order, then the two compounds (`by` is Y+B on one line,
/// `dr` the D-pad right the inventory switch shares with `dl`).
pub fn vocabulary() -> Vec<(&'static str, String)> {
    let mut vocabulary = Vec::new();
    for input in &INPUTS {
        if let Some(token) = input.token {
            vocabulary.push((token, input.key.to_owned()));
        }
    }

    vocabulary.push(("by", format!("{} + {}", INPUTS[heavy()].key, INPUTS[zandatsu()].key)));
    vocabulary.push((
        "dr",
        INPUTS[index_of("weapon_select").expect("the inventory switch is an input")]
            .key
            .to_owned(),
    ));
    vocabulary
}

// ── writing ──────────────────────────────────────────────────────────────────

/// The canonical text of a document. Ends with a newline.
///
/// Commands the DSL cannot say — `raw_key`, `dik_key`, `when_enemy` — are an error rather than a
/// silent loss.
pub fn write(document: &ScriptDocument) -> ScriptResult<String> {
    json::validate(document)?;

    let mut text = String::new();
    let rules = rules(document)?;
    if !rules.is_empty() {
        text.push(RULES_MARK);
        text.push(' ');
        text.push_str(&rules.join(" "));
        text.push('\n');
    }

    for (frame, tokens) in frames(document)? {
        let _ = write!(text, "{frame}");
        for token in tokens {
            text.push(' ');
            text.push_str(&token);
        }
        text.push('\n');
    }

    Ok(text)
}

/// The rules line's attributes: name, trigger, restart — every field of the `POST /script/run`
/// body that is not a command.
fn rules(document: &ScriptDocument) -> ScriptResult<Vec<String>> {
    let mut rules = Vec::new();

    if document.name != DEFAULT_NAME {
        // Whitespace separates the attributes and `#` starts a comment, so such a name has no
        // text spelling at all — refusing beats writing a line the parser reads back as
        // something else.
        if document.name.chars().any(char::is_whitespace) || document.name.contains('#') {
            return Err(ScriptFormatException::new(format!(
                "name '{}': the text format cannot carry whitespace or '#' in a name",
                document.name
            )));
        }

        rules.push(format!("name={}", document.name));
    }

    if let Some(trigger) = &document.trigger {
        if let Some(position) = trigger.pos {
            rules.push(format!(
                "trig=pos:{},{},{}",
                number_f32(position[0]),
                number_f32(position[1]),
                number_f32(position[2])
            ));
        }

        if let Some(ticks) = trigger.ticks {
            rules.push(format!("trig=ticks:{ticks}"));
        }
    }

    if let Some(restart) = &document.restart {
        rules.push(restart_token(restart));
    }

    Ok(rules)
}

/// `restart`, or `restart:ups=2,downs=1` with the parameters that differ from the mod's
/// defaults, spelled out in the JSON's own field names.
///
/// A parameter the JSON leaves out (`None`) means the mod's default, so it is not written either.
fn restart_token(restart: &RestartSpec) -> String {
    let defaults = RestartSpec::default();
    let mut parameters = Vec::new();

    let mut add = |name: &str, value: Option<u32>, fallback: Option<u32>| {
        if let Some(frames) = value
            && Some(frames) != fallback
        {
            parameters.push(format!("{name}={frames}"));
        }
    };

    add("ups", restart.ups, defaults.ups);
    add("downs", restart.downs, defaults.downs);
    add("hold", restart.hold, defaults.hold);
    add("open_gap", restart.open_gap, defaults.open_gap);
    add("gap", restart.gap, defaults.gap);
    add("confirms", restart.confirms, defaults.confirms);
    add("confirm_gap", restart.confirm_gap, defaults.confirm_gap);
    add("tail", restart.tail, defaults.tail);

    if parameters.is_empty() {
        "restart".to_owned()
    } else {
        format!("restart:{}", parameters.join(","))
    }
}

/// One line per frame a command touches, ascending.
fn frames(document: &ScriptDocument) -> ScriptResult<Vec<(u32, Vec<String>)>> {
    let mut by_frame: BTreeMap<u32, Vec<&ScriptCommand>> = BTreeMap::new();
    for (index, command) in document.commands.iter().enumerate() {
        unsupported(command, index)?;
        by_frame.entry(command.t).or_default().push(command);
    }

    let mut lines = Vec::with_capacity(by_frame.len());
    for (frame, commands) in by_frame {
        lines.push((frame, tokens(frame, &commands)?));
    }

    Ok(lines)
}

/// The tokens of one frame, in canonical order: the sticks first (`ls`, then `rs`), then the
/// inputs in column order.
///
/// A boolean held by several commands of the frame becomes the longest of their durations: the
/// mod ORs the bits of every active command, so the run of frames is the union — the same input,
/// said shorter. A stick is the last command to set it, as the mod assigns it; two different
/// values on one frame cannot be said by one line, and that is an error.
fn tokens(frame: u32, commands: &[&ScriptCommand]) -> ScriptResult<Vec<String>> {
    let mut tokens = Vec::new();
    let mut left: Option<[f64; 2]> = None;
    let mut right: Option<[f64; 2]> = None;
    let mut left_frames = 1u32;
    let mut right_frames = 1u32;
    let mut durations = [0u32; INPUTS.len()];

    for command in commands {
        if let Some(commanded) = commanded_stick(&command.input) {
            if let Some(previous) = left
                && !same(previous, commanded)
            {
                return Err(ScriptFormatException::new(format!(
                    "frame {frame}: two commands move the left stick differently — a line says one stick"
                )));
            }

            left = Some(commanded);
            left_frames = left_frames.max(command.duration);
        }

        if let Some(camera) = command.input.camera {
            let axes = [f64::from(camera[0]), f64::from(camera[1])];
            if let Some(previous) = right
                && !same(previous, axes)
            {
                return Err(ScriptFormatException::new(format!(
                    "frame {frame}: two commands move the camera differently — a line says one stick"
                )));
            }

            right = Some(axes);
            right_frames = right_frames.max(command.duration);
        }

        for (index, input) in INPUTS.iter().enumerate() {
            if (input.get)(&command.input) && command.duration > durations[index] {
                durations[index] = command.duration;
            }
        }
    }

    if let Some(axes) = left {
        tokens.extend(stick_tokens(LEFT_STICK, LEFT_STICK_X, LEFT_STICK_Y, axes, left_frames));
    }

    if let Some(axes) = right {
        tokens.extend(stick_tokens(RIGHT_STICK, RIGHT_STICK_X, RIGHT_STICK_Y, axes, right_frames));
    }

    // Y+B is one token when the two are held for the same stretch — the shape the game's Execute
    // prompt has; held for different stretches they stay two tokens.
    let by_frames = if durations[heavy()] > 0 && durations[heavy()] == durations[zandatsu()] {
        durations[heavy()]
    } else {
        0
    };

    for (index, input) in INPUTS.iter().enumerate() {
        let Some(token) = input.token else {
            continue;
        };

        if durations[index] == 0 {
            continue;
        }

        if by_frames > 0 {
            if index == zandatsu() {
                continue;
            }

            if index == heavy() {
                tokens.push(format!("by{}", duration(by_frames)));
                continue;
            }
        }

        tokens.push(format!("{token}{}", duration(durations[index])));
    }

    Ok(tokens)
}

/// One stick as tokens. A full press reads as an angle; anything else reads as exact axis
/// values, and an axis at zero is left out because a missing axis already means zero.
fn stick_tokens(angle: &str, x: &str, y: &str, axes: [f64; 2], frames: u32) -> Vec<String> {
    let magnitude = (axes[0] * axes[0] + axes[1] * axes[1]).sqrt();
    if (magnitude - AXIS_UNIT).abs() < FULL_PRESS_TOLERANCE {
        return vec![format!("{angle}:{}{}", number_f64(angle_of(axes)), duration(frames))];
    }

    if axes[0] == 0.0 && axes[1] == 0.0 {
        return vec![format!("{x}:0{}", duration(frames))];
    }

    let mut tokens = Vec::new();
    if axes[0] != 0.0 {
        tokens.push(format!("{x}:{}{}", number_f64(rounded(axes[0])), duration(frames)));
    }

    if axes[1] != 0.0 {
        tokens.push(format!("{y}:{}{}", number_f64(rounded(axes[1])), duration(frames)));
    }

    tokens
}

/// Refuses what the DSL has no spelling for, naming the command.
fn unsupported(command: &ScriptCommand, index: usize) -> ScriptResult<()> {
    if command.when_enemy.is_some() {
        return Err(ScriptFormatException::new(format!(
            "commands[{index}]: when_enemy has no DSL spelling — the editor shows it as JSON only"
        )));
    }

    if command.input.raw_key.is_some() || command.input.dik_key.is_some() {
        return Err(ScriptFormatException::new(format!(
            "commands[{index}]: raw_key/dik_key have no DSL spelling — the editor shows them as JSON only"
        )));
    }

    Ok(())
}

/// `:frames`, left out for a single frame — the DSL's own default.
fn duration(frames: u32) -> String {
    if frames == 1 {
        String::new()
    } else {
        format!(":{frames}")
    }
}

/// Axis values as a compass angle: 0 is forward (up), 90 right, 180 back, 270 left — the way the
/// stick reads on the pad, not the way the table measures the axis.
fn angle_of(axes: [f64; 2]) -> f64 {
    let angle = axes[0].atan2(-axes[1]) * 180.0 / std::f64::consts::PI;
    let degrees = (if angle < 0.0 { angle + 360.0 } else { angle } * 1000.0).round() / 1000.0;
    if degrees >= 360.0 { 0.0 } else { degrees }
}

/// A full press at that compass angle, in axis units.
fn full_press(angle: f64) -> [f32; 2] {
    let radians = angle * std::f64::consts::PI / 180.0;
    [
        axis(AXIS_UNIT * radians.sin()),
        axis(-AXIS_UNIT * radians.cos()),
    ]
}

/// One axis value, rounded to what the text keeps and snapped away from `-0`.
fn axis(value: f64) -> f32 {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded > -AXIS_STEP / 2.0 && rounded < AXIS_STEP / 2.0 {
        0.0
    } else {
        rounded as f32
    }
}

fn rounded(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Numbers are written shortest-round-trip and in the invariant culture, so a value survives
/// `write` → `parse` unchanged.
///
/// A `f32` is formatted through the shortest round-trip of its own width: casting it to `f64`
/// first would spell `-24.7f` as `-24.700000762939453` — the value the machine stores, not the
/// value the user typed.
fn number_f32(value: f32) -> String {
    format!("{value}")
}

fn number_f64(value: f64) -> String {
    format!("{value}")
}

/// Two stick values are the same when their axes agree to the precision the text keeps.
fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < AXIS_STEP && (a[1] - b[1]).abs() < AXIS_STEP
}

// ── parsing ──────────────────────────────────────────────────────────────────

/// The text as the format reads it: lines separated by `\n`.
///
/// Every reader of a script text goes through here, so one text never reads two ways. A `.tas`
/// written on another machine, or by an editor with Windows line endings, carries `\r\n`; one that
/// went through a WinUI `TextBox` carries a lone `\r` between the lines and no `\n` at all
/// (measured — the Reactor sibling's control does exactly that). A frame line is trimmed and its
/// tokens are separated by spaces, so neither changes the meaning — but a reader that did not
/// normalise would see a script as one broken line.
pub fn lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Reads a script text.
///
/// Every failure names its line; the cross-field limits come from [`json::validate`] and name the
/// command instead.
pub fn parse(text: &str) -> ScriptResult<ScriptDocument> {
    let mut commands: Vec<ScriptCommand> = Vec::new();
    let mut name: Option<String> = None;
    let mut trigger_position: Option<[f32; 3]> = None;
    let mut trigger_ticks: Option<u64> = None;
    let mut restart: Option<RestartSpec> = None;
    let mut rules_seen = false;

    // The frame of the last line that started with a number: what every refusal reports, so a
    // message carries the `.tas` coordinate and not just the line index. `None` until one is read.
    let mut last_frame: Option<u32> = None;

    // The parse's own refusal: a message, and the frame the parse had reached.
    let refuse = |message: String, frame: Option<u32>| ScriptFormatException::at_frame(message, frame);

    for (index, raw) in lines(text).split('\n').enumerate() {
        let number = index + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with(RULES_MARK) {
            if rules_seen || !commands.is_empty() {
                return Err(refuse(
                    format!("line {number}: the rules line must come first"),
                    last_frame,
                ));
            }

            rules_seen = true;
            parse_rules(
                number,
                &line[RULES_MARK.len_utf8()..],
                last_frame,
                &mut name,
                &mut trigger_position,
                &mut trigger_ticks,
                &mut restart,
            )?;
            continue;
        }

        parse_frame(number, line, &mut commands, &mut last_frame)?;
    }

    let document = ScriptDocument {
        name: name.unwrap_or_else(|| DEFAULT_NAME.to_owned()),
        trigger: if trigger_position.is_none() && trigger_ticks.is_none() {
            None
        } else {
            Some(ScriptTrigger {
                pos: trigger_position,
                ticks: trigger_ticks,
            })
        },
        restart,
        commands,
    };

    // The cross-field limits refuse a document rather than a line: there is no line to name, so
    // the frame the whole text had is what they are re-thrown with.
    json::validate(&document).map_err(|refused| refuse(refused.message().to_owned(), last_frame))?;

    Ok(document)
}

#[allow(clippy::too_many_arguments)]
fn parse_rules(
    number: usize,
    rest: &str,
    last_frame: Option<u32>,
    name: &mut Option<String>,
    trigger_position: &mut Option<[f32; 3]>,
    trigger_ticks: &mut Option<u64>,
    restart: &mut Option<RestartSpec>,
) -> ScriptResult<()> {
    let refuse =
        |message: String| ScriptFormatException::at_frame(message, last_frame);

    for attribute in rest.split(' ').filter(|part| !part.is_empty()) {
        // An attribute is `key=value` or `key:value` — the colon is what `restart` carries its
        // own `name=value` list behind, so the split stops at whichever comes first.
        let separator = attribute.find(['=', ':']);
        let key = match separator {
            Some(at) => &attribute[..at],
            None => attribute,
        };
        let raw = separator.map(|at| &attribute[at + 1..]);

        match key.to_ascii_lowercase().as_str() {
            "name" => {
                if name.is_some() {
                    return Err(refuse(format!("line {number}: name is given twice")));
                }

                *name = Some(value_string(number, key, raw, last_frame)?);
            }

            "trig" => parse_trigger(
                number,
                &value_string(number, key, raw, last_frame)?,
                last_frame,
                trigger_position,
                trigger_ticks,
            )?,

            "restart" => {
                if restart.is_some() {
                    return Err(refuse(format!("line {number}: restart is given twice")));
                }

                *restart = Some(parse_restart(number, raw.unwrap_or(""), last_frame)?);
            }

            _ => {
                return Err(refuse(format!(
                    "line {number}: unknown attribute '{key}' (name, trig, restart)"
                )));
            }
        }
    }

    Ok(())
}

fn parse_trigger(
    number: usize,
    value: &str,
    last_frame: Option<u32>,
    position: &mut Option<[f32; 3]>,
    ticks: &mut Option<u64>,
) -> ScriptResult<()> {
    let refuse = |message: String| ScriptFormatException::at_frame(message, last_frame);

    let separator = value.find(':');
    let kind = match separator {
        Some(at) => &value[..at],
        None => value,
    };
    let argument = separator.map(|at| &value[at + 1..]).unwrap_or("");

    match kind.to_ascii_lowercase().as_str() {
        "pos" => {
            if position.is_some() {
                return Err(refuse(format!("line {number}: trig=pos is given twice")));
            }

            let parts: Vec<&str> = argument.split(',').collect();
            if parts.len() != 3 {
                return Err(refuse(format!("line {number}: trig=pos needs three numbers")));
            }

            *position = Some([
                float(number, "trig=pos", parts[0], last_frame)?,
                float(number, "trig=pos", parts[1], last_frame)?,
                float(number, "trig=pos", parts[2], last_frame)?,
            ]);
        }

        "ticks" => {
            if ticks.is_some() {
                return Err(refuse(format!("line {number}: trig=ticks is given twice")));
            }

            *ticks = Some(unsigned64(number, "trig=ticks", argument, last_frame)?);
        }

        _ => {
            return Err(refuse(format!(
                "line {number}: trig must be pos or ticks, got '{kind}'"
            )));
        }
    }

    Ok(())
}

fn parse_restart(number: usize, value: &str, last_frame: Option<u32>) -> ScriptResult<RestartSpec> {
    let refuse = |message: String| ScriptFormatException::at_frame(message, last_frame);
    let mut policy = RestartSpec::default();

    if value.is_empty() {
        return Ok(policy);
    }

    for parameter in value.split(',') {
        if parameter.is_empty() {
            continue;
        }

        let separator = parameter.find('=');
        let key = match separator {
            Some(at) => &parameter[..at],
            None => parameter,
        };
        let raw = separator.map(|at| &parameter[at + 1..]);

        let parsed = unsigned32(
            number,
            &format!("restart:{key}"),
            &value_string(number, key, raw, last_frame)?,
            last_frame,
        )?;

        match key.to_ascii_lowercase().as_str() {
            "ups" => policy.ups = Some(parsed),
            "downs" => policy.downs = Some(parsed),
            "hold" => policy.hold = Some(parsed),
            "open_gap" => policy.open_gap = Some(parsed),
            "gap" => policy.gap = Some(parsed),
            "confirms" => policy.confirms = Some(parsed),
            "confirm_gap" => policy.confirm_gap = Some(parsed),
            "tail" => policy.tail = Some(parsed),
            _ => {
                return Err(refuse(format!(
                    "line {number}: unknown restart parameter '{key}' \
                     (ups, downs, hold, open_gap, gap, confirms, confirm_gap, tail)"
                )));
            }
        }
    }

    Ok(policy)
}

fn parse_frame(
    number: usize,
    line: &str,
    into: &mut Vec<ScriptCommand>,
    last_frame: &mut Option<u32>,
) -> ScriptResult<()> {
    let refuse = |message: String, frame: Option<u32>| ScriptFormatException::at_frame(message, frame);

    let parts: Vec<&str> = line.split(' ').filter(|part| !part.is_empty()).collect();
    // The frame of the line is read — and recorded — before its tokens are checked, so a bad
    // token on frame 132 reports 132: that is the coordinate an author navigates by.
    let frame = unsigned32(number, "frame", parts[0], *last_frame)?;
    *last_frame = Some(frame);

    // Tokens of one line become one command per distinct duration: the DSL gives every token its
    // own, and a command holds a single one.
    let mut groups: BTreeMap<u32, ScriptInput> = BTreeMap::new();
    let mut held = [false; INPUTS.len()];
    let mut sticks: [Option<[f32; 2]>; 2] = [None, None];
    let mut stick_frames = [1u32; 2];
    let mut angle_seen = [false; 2];
    let mut axis_seen = [[false; 2]; 2];

    for token in &parts[1..] {
        let separator = token.find(':');
        let key = match separator {
            Some(at) => &token[..at],
            None => token,
        };
        let arguments: Vec<&str> = match separator {
            Some(at) => token[at + 1..].split(':').collect(),
            None => Vec::new(),
        };
        let lower = key.to_ascii_lowercase();

        if parse_stick(
            number,
            &lower,
            &arguments,
            frame,
            &mut sticks,
            &mut stick_frames,
            &mut angle_seen,
            &mut axis_seen,
        )? {
            continue;
        }

        let Some((set, touched)) = flag(&lower) else {
            return Err(refuse(
                format!("line {number}: unknown token '{key}' — a pad button, a stick token or `by`"),
                Some(frame),
            ));
        };

        if arguments.len() > 1 {
            return Err(refuse(
                format!("line {number}: '{key}' takes at most one duration"),
                Some(frame),
            ));
        }

        for touched in touched {
            if held[touched] {
                return Err(refuse(
                    format!(
                        "line {number}: '{}' is given twice on this frame",
                        INPUTS[touched].key
                    ),
                    Some(frame),
                ));
            }

            held[touched] = true;
        }

        let frames = if arguments.is_empty() {
            1
        } else {
            duration_of(number, key, arguments[0], frame)?
        };

        let current = groups.get(&frames).copied().unwrap_or_default();
        groups.insert(frames, set(current));
    }

    // The sticks join the line's durations like any other token — a stick held for its own
    // stretch becomes its own command.
    for side in 0..2 {
        let Some(axes) = sticks[side] else {
            continue;
        };

        let current = groups.get(&stick_frames[side]).copied().unwrap_or_default();
        let with_stick = if side == 0 {
            ScriptInput {
                left_stick: Some(axes),
                ..current
            }
        } else {
            ScriptInput {
                camera: Some(axes),
                ..current
            }
        };
        groups.insert(stick_frames[side], with_stick);
    }

    for (frames, input) in groups {
        into.push(ScriptCommand {
            t: frame,
            duration: frames,
            input,
            when_enemy: None,
        });
    }

    Ok(())
}

/// The stick tokens: `<name>:<angle>[:<frames>]` is a full press, `<name>x`/`<name>y` with a
/// value set one axis exactly. The two forms do not mix on one line, and a stick is set once per
/// line.
#[allow(clippy::too_many_arguments)]
fn parse_stick(
    number: usize,
    lower: &str,
    arguments: &[&str],
    frame: u32,
    sticks: &mut [Option<[f32; 2]>; 2],
    stick_frames: &mut [u32; 2],
    angle_seen: &mut [bool; 2],
    axis_seen: &mut [[bool; 2]; 2],
) -> ScriptResult<bool> {
    let refuse = |message: String| ScriptFormatException::at_frame(message, Some(frame));

    let is_left = matches!(lower, LEFT_STICK | LEFT_STICK_X | LEFT_STICK_Y);
    let is_right = matches!(lower, RIGHT_STICK | RIGHT_STICK_X | RIGHT_STICK_Y);
    if !is_left && !is_right {
        return Ok(false);
    }

    if arguments.is_empty() || arguments.len() > 2 {
        return Err(refuse(format!(
            "line {number}: '{lower}' needs a value and an optional duration"
        )));
    }

    let side = if is_left { 0 } else { 1 };
    let frames = if arguments.len() == 2 {
        duration_of(number, lower, arguments[1], frame)?
    } else {
        1
    };
    stick_frames[side] = frames;

    if lower == LEFT_STICK || lower == RIGHT_STICK {
        if angle_seen[side] {
            return Err(refuse(format!("line {number}: '{lower}' is given twice")));
        }

        if axis_seen[side][0] || axis_seen[side][1] {
            return Err(refuse(format!(
                "line {number}: '{lower}' and an exact stick value on the same line say two different sticks"
            )));
        }

        angle_seen[side] = true;
        sticks[side] = Some(full_press(f64::from(float(number, lower, arguments[0], Some(frame))?)));
        return Ok(true);
    }

    if angle_seen[side] {
        return Err(refuse(format!(
            "line {number}: '{lower}' and an angle on the same line say two different sticks"
        )));
    }

    let axis = if lower == LEFT_STICK_X || lower == RIGHT_STICK_X {
        0
    } else {
        1
    };

    if axis_seen[side][axis] {
        return Err(refuse(format!("line {number}: '{lower}' is given twice")));
    }

    axis_seen[side][axis] = true;
    let mut axes = sticks[side].unwrap_or([0.0, 0.0]);
    axes[axis] = axis_value(float(number, lower, arguments[0], Some(frame))?);
    sticks[side] = Some(axes);
    Ok(true)
}

/// What each token sets, and which inputs it touches — the second half is what makes "this input
/// is already on this line" checkable when one token stands for two inputs.
fn flag(token: &str) -> Option<(fn(ScriptInput) -> ScriptInput, Vec<usize>)> {
    for (index, input) in INPUTS.iter().enumerate() {
        if input.token == Some(token) {
            return Some((input.set, vec![index]));
        }
    }

    match token {
        // Y+B is the Execute prompt: one token, two inputs, same frame.
        "by" => Some((
            |input| ScriptInput {
                heavy_attack: true,
                zandatsu: true,
                ..input
            },
            vec![heavy(), zandatsu()],
        )),
        // The inventory switch is a single flag in the mod, so the D-pad's other direction
        // spells the same input — `dl` is the canonical one, `dr` is accepted.
        "dr" => flag("dl"),
        _ => None,
    }
}

/// An attribute's raw value, or a refusal naming the key that had none.
fn value_string(
    number: usize,
    key: &str,
    raw: Option<&str>,
    last_frame: Option<u32>,
) -> ScriptResult<String> {
    match raw {
        Some(value) => Ok(value.to_owned()),
        None => Err(ScriptFormatException::at_frame(
            format!("line {number}: '{key}' needs a value"),
            last_frame,
        )),
    }
}

fn duration_of(
    number: usize,
    key: &str,
    value: &str,
    frame: u32,
) -> ScriptResult<u32> {
    let frames = unsigned32(number, key, value, Some(frame))?;
    if frames == 0 {
        return Err(ScriptFormatException::at_frame(
            format!("line {number}: '{key}' duration must be >= 1"),
            Some(frame),
        ));
    }

    Ok(frames)
}

fn unsigned32(
    number: usize,
    field: &str,
    value: &str,
    frame: Option<u32>,
) -> ScriptResult<u32> {
    value.parse::<u32>().map_err(|_| {
        ScriptFormatException::at_frame(
            format!("line {number}: {field} '{value}' is not a number"),
            frame,
        )
    })
}

fn unsigned64(
    number: usize,
    field: &str,
    value: &str,
    frame: Option<u32>,
) -> ScriptResult<u64> {
    value.parse::<u64>().map_err(|_| {
        ScriptFormatException::at_frame(
            format!("line {number}: {field} '{value}' is not a number"),
            frame,
        )
    })
}

fn float(number: usize, field: &str, value: &str, frame: Option<u32>) -> ScriptResult<f32> {
    let parsed = value.parse::<f32>().map_err(|_| {
        ScriptFormatException::at_frame(
            format!("line {number}: {field} '{value}' is not a number"),
            frame,
        )
    })?;

    if !parsed.is_finite() {
        return Err(ScriptFormatException::at_frame(
            format!("line {number}: {field} '{value}' is not a number"),
            frame,
        ));
    }

    Ok(parsed)
}

/// One axis value, rounded to what the text keeps and snapped away from `-0`.
fn axis_value(value: f32) -> f32 {
    let rounded = (f64::from(value) * 1000.0).round() / 1000.0;
    if rounded > -AXIS_STEP / 2.0 && rounded < AXIS_STEP / 2.0 {
        0.0
    } else {
        rounded as f32
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find(COMMENT_MARK) {
        Some(at) => &line[..at],
        None => line,
    }
}
