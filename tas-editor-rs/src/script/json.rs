//! The API JSON of a script — the body of the mod's `POST /script/run`.
//!
//! The shapes themselves live in [`crate::script::model`]; this module is the mod's own
//! cross-field limits (`parse_script` in `src/api.rs`) plus the read/write wrappers. The editor
//! refuses a script the game would answer `400` on instead of storing it.
//!
//! Unknown keys are refused by the types themselves (`deny_unknown_fields`), mirroring the mod.
//!
//! ⚠️ **Reading goes through `serde`; writing does not.** The written form is a *contract* — it is
//! what the mod is handed and what the C# editor's goldens pin down byte for byte
//! (`tests/golden.rs`) — and one detail of it is not `serde_json`'s to give: an integral float is
//! written as an integer (`1000`, not `1000.0`), the way .NET's `System.Text.Json` writes it. So
//! [`write`] emits the document itself, in the field order of [`crate::script::model`], and a bug
//! in it cannot hide: the goldens fail, and `read` reads every written script back.

use std::fmt::Write as _;

use super::error::{ScriptFormatException, ScriptResult};
use super::model::{
    EnemyCondition, MAX_NAME_LENGTH, MAX_SCRIPT_FRAMES, RestartSpec, ScriptCommand, ScriptDocument,
    ScriptInput, ScriptTrigger,
};

/// Reads a script. Types and unknown keys are covered by deserialization; everything else is
/// cross-field and is checked by [`validate`].
pub fn read(json: &str) -> ScriptResult<ScriptDocument> {
    let document: ScriptDocument = serde_json::from_str(json)
        .map_err(|error| ScriptFormatException::new(format!("script: {error}")))?;
    validate(&document)?;
    Ok(document)
}

/// Writes a script the way the C# editor's goldens spell it, refusing one the mod would refuse
/// rather than emitting it.
///
/// Unset fields stay out of the JSON: an absent key and a `false`/`null` mean the same to the mod,
/// and writing them would bury the one line that matters under 26 lines of `false`. `t` and
/// `duration` are written unconditionally — zero is a real frame number.
///
/// No trailing newline: a script body is a body, and the goldens end at the closing brace.
pub fn write(document: &ScriptDocument) -> ScriptResult<String> {
    validate(document)?;

    let root = Node::Object(document_fields(document));
    Ok(render(&root))
}

/// A JSON value, as a tree — built first, rendered second.
///
/// The shape of the written script is fixed and its nesting is deeper than it looks (a command's
/// `input`, an array inside that), so the indentation cannot be a suffix per field: an object's
/// lines are indented by where the object *sits*, which the field that holds it is the only one who
/// knows. Building the tree and rendering it in one place is what keeps `input`'s keys where the
/// golden puts them.
enum Node {
    /// A value already spelled as JSON: a number, `true`, or a quoted string.
    Text(String),
    /// An array, one element per line.
    Array(Vec<Node>),
    /// An object of key → value, in the order given.
    Object(Vec<(&'static str, Node)>),
}

impl Node {
    /// A number or `true`, already spelled.
    fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    fn string(value: &str) -> Self {
        Self::Text(quoted(value))
    }
}

/// The document's own fields, in [`ScriptDocument`]'s order.
fn document_fields(document: &ScriptDocument) -> Vec<(&'static str, Node)> {
    let mut fields = vec![("name", Node::string(&document.name))];

    if let Some(trigger) = &document.trigger {
        fields.push(("trigger", Node::Object(trigger_fields(trigger))));
    }

    if let Some(restart) = &document.restart {
        fields.push(("restart", Node::Object(restart_fields(restart))));
    }

    fields.push((
        "commands",
        Node::Array(
            document
                .commands
                .iter()
                .map(|command| Node::Object(command_fields(command)))
                .collect(),
        ),
    ));

    fields
}

/// `trigger`: whichever of the two the document carries — at least one, by [`validate`].
fn trigger_fields(trigger: &ScriptTrigger) -> Vec<(&'static str, Node)> {
    let mut fields = Vec::new();

    if let Some(position) = trigger.pos {
        fields.push((
            "pos",
            Node::Array(position.iter().map(|value| Node::text(number(*value))).collect()),
        ));
    }

    if let Some(ticks) = trigger.ticks {
        fields.push(("ticks", Node::text(ticks.to_string())));
    }

    fields
}

/// `restart`: every parameter that is set, in the mod's own field order.
///
/// ⚠️ The order is written out here rather than taken from the struct: it is the *mod's* field
/// order, and a field added on one side must not reorder the other's output.
fn restart_fields(restart: &RestartSpec) -> Vec<(&'static str, Node)> {
    let mut fields = Vec::new();

    let mut add = |name: &'static str, value: Option<u32>| {
        if let Some(frames) = value {
            fields.push((name, Node::text(frames.to_string())));
        }
    };

    add("ups", restart.ups);
    add("downs", restart.downs);
    add("hold", restart.hold);
    add("open_gap", restart.open_gap);
    add("gap", restart.gap);
    add("confirms", restart.confirms);
    add("confirm_gap", restart.confirm_gap);
    add("tail", restart.tail);

    fields
}

/// One command: its frame, its length, its input, and a condition when it has one.
fn command_fields(command: &ScriptCommand) -> Vec<(&'static str, Node)> {
    let mut fields = vec![
        ("t", Node::text(command.t.to_string())),
        ("duration", Node::text(command.duration.to_string())),
        ("input", Node::Object(input_fields(&command.input))),
    ];

    if let Some(condition) = &command.when_enemy {
        fields.push(("when_enemy", Node::Object(condition_fields(condition))));
    }

    fields
}

/// The `input` object: every flag that is set, then the two raw codes and the two sticks.
///
/// The order is [`ScriptInput`]'s own — the flags in the command table's column order, the
/// compounds of the text format nowhere, since the JSON carries the flags themselves.
fn input_fields(input: &ScriptInput) -> Vec<(&'static str, Node)> {
    let mut fields = Vec::new();

    /// A flag, written only when it is on: an absent key and a `false` mean the same to the mod.
    fn flag(fields: &mut Vec<(&'static str, Node)>, name: &'static str, set: bool) {
        if set {
            fields.push((name, Node::text("true")));
        }
    }

    flag(&mut fields, "forward", input.forward);
    flag(&mut fields, "backward", input.backward);
    flag(&mut fields, "left", input.left);
    flag(&mut fields, "right", input.right);
    flag(&mut fields, "jump", input.jump);
    flag(&mut fields, "light_attack", input.light_attack);
    flag(&mut fields, "heavy_attack", input.heavy_attack);

    if let Some(camera) = input.camera {
        fields.push(("camera", pair(camera)));
    }

    flag(&mut fields, "ripper", input.ripper);
    flag(&mut fields, "blade", input.blade);
    flag(&mut fields, "ninja_run", input.ninja_run);
    flag(&mut fields, "walk", input.walk);
    flag(&mut fields, "dodge", input.dodge);
    flag(&mut fields, "lock_on", input.lock_on);
    flag(&mut fields, "subweapon", input.subweapon);
    flag(&mut fields, "item", input.item);
    flag(&mut fields, "ar_mode", input.ar_mode);
    flag(&mut fields, "weapon_select", input.weapon_select);
    flag(&mut fields, "codec", input.codec);
    flag(&mut fields, "zandatsu", input.zandatsu);
    flag(&mut fields, "camera_reset", input.camera_reset);
    flag(&mut fields, "pause", input.pause);
    flag(&mut fields, "confirm", input.confirm);
    flag(&mut fields, "menu_up", input.menu_up);
    flag(&mut fields, "menu_down", input.menu_down);
    flag(&mut fields, "menu_left", input.menu_left);
    flag(&mut fields, "menu_right", input.menu_right);

    if let Some(raw) = input.raw_key {
        fields.push(("raw_key", Node::text(raw.to_string())));
    }

    if let Some(dik) = input.dik_key {
        fields.push(("dik_key", Node::text(dik.to_string())));
    }

    if let Some(stick) = input.left_stick {
        fields.push(("left_stick", pair(stick)));
    }

    fields
}

/// `when_enemy`: every bound that is set, in the mod's own field order.
///
/// The bounds are of two kinds and it shows: the animations and the animation-frame range are
/// **integers** (`i32`, the enemy's own units), while the distances and heights are floats.
fn condition_fields(condition: &EnemyCondition) -> Vec<(&'static str, Node)> {
    let mut fields = Vec::new();

    if !condition.anim.is_empty() {
        fields.push((
            "anim",
            Node::Array(
                condition
                    .anim
                    .iter()
                    .map(|id| Node::text(id.to_string()))
                    .collect(),
            ),
        ));
    }

    if let Some(value) = condition.frame_min {
        fields.push(("frame_min", Node::text(value.to_string())));
    }

    if let Some(value) = condition.frame_max {
        fields.push(("frame_max", Node::text(value.to_string())));
    }

    let mut float = |name: &'static str, value: Option<f32>| {
        if let Some(value) = value {
            fields.push((name, Node::text(number(value))));
        }
    };

    float("dist_max", condition.dist_max);
    float("blade_dy_min", condition.blade_dy_min);
    float("player_y_min", condition.player_y_min);
    float("player_y_max", condition.player_y_max);
    float("player_vy_max", condition.player_vy_max);
    drop(float);

    if condition.repeat {
        fields.push(("repeat", Node::text("true")));
    }

    fields
}

/// A two-number array — a stick or a camera.
fn pair(values: [f32; 2]) -> Node {
    Node::Array(values.iter().map(|value| Node::text(number(*value))).collect())
}

/// Two spaces per level, the indent .NET's `WriteIndented` uses.
fn indent(level: usize) -> String {
    "  ".repeat(level)
}

/// The tree as the text: an object's keys one level in from its braces, an array's elements the
/// same, and a collapsed `{}`/`[]` for anything empty.
fn render(node: &Node) -> String {
    let mut out = String::new();
    render_into(node, 0, &mut out);
    out
}

fn render_into(node: &Node, level: usize, out: &mut String) {
    match node {
        Node::Text(text) => out.push_str(text),

        Node::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }

            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }

                out.push_str(&indent(level + 1));
                render_into(item, level + 1, out);
            }

            out.push('\n');
            out.push_str(&indent(level));
            out.push(']');
        }

        Node::Object(fields) => {
            out.push_str("{\n");
            for (index, (key, value)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }

                out.push_str(&indent(level + 1));
                out.push_str(&quoted(key));
                out.push_str(": ");
                render_into(value, level + 1, out);
            }

            out.push('\n');
            out.push_str(&indent(level));
            out.push('}');
        }
    }
}

/// A JSON string, escaped.
fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');

    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Everything below 0x20 has to be escaped, and nothing else does: the rest is already
            // valid JSON as it stands, UTF-8 included.
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }

    out.push('"');
    out
}

/// A float the way the C# editor's goldens spell one: shortest round-trip, and **an integral value
/// as an integer** (`1000`, not `1000.0`).
///
/// That last part is the whole reason this module writes the JSON itself: it is what .NET's
/// `System.Text.Json` does for a float, and `serde_json` deliberately does not — it keeps the `.0`
/// so a float stays distinguishable from an integer. Both parse to the same value, but the golden
/// files pin the spelling, and a port that spells it differently is a port whose output has to be
/// re-checked by hand.
///
/// `-0.0` is written as `0`: the text format already snaps it away (a `-0` would read as a
/// direction), so writing it here would make the two forms of one script disagree.
fn number(value: f32) -> String {
    if value == 0.0 {
        // Catches `-0.0` too: it compares equal to `0.0`, and that is the answer wanted.
        return "0".to_owned();
    }

    format!("{value}")
}

/// The checks the mod runs after serde: everything here is cross-field.
pub fn validate(document: &ScriptDocument) -> ScriptResult<()> {
    if document.name.chars().count() > MAX_NAME_LENGTH {
        return Err(ScriptFormatException::new(format!(
            "name is longer than {MAX_NAME_LENGTH} characters"
        )));
    }

    if document.commands.is_empty() {
        return Err(ScriptFormatException::new("commands is empty"));
    }

    if let Some(trigger) = &document.trigger
        && trigger.pos.is_none()
        && trigger.ticks.is_none()
    {
        return Err(ScriptFormatException::new("trigger needs pos or ticks"));
    }

    for (index, command) in document.commands.iter().enumerate() {
        let where_ = format!("commands[{index}]");

        if command.duration == 0 {
            return Err(ScriptFormatException::new(format!(
                "{where_}: duration must be >= 1"
            )));
        }

        if command.t > MAX_SCRIPT_FRAMES || command.duration > MAX_SCRIPT_FRAMES {
            return Err(ScriptFormatException::new(format!(
                "{where_}: t/duration exceeds max {MAX_SCRIPT_FRAMES}"
            )));
        }

        if command.t + command.duration > MAX_SCRIPT_FRAMES {
            return Err(ScriptFormatException::new(format!(
                "{where_}: t+duration exceeds max {MAX_SCRIPT_FRAMES}"
            )));
        }

        if command.input.is_empty() {
            return Err(ScriptFormatException::new(format!(
                "{where_}: input is empty"
            )));
        }
    }

    Ok(())
}
