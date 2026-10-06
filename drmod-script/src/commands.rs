//! The DSL's vocabulary with help attached — what the text region's reference panel shows.
//!
//! The pad buttons are [`dsl::vocabulary`]'s own tokens, not a second spelling of them: this
//! module only answers *what a token means*. Tests walk both directions — every token of the
//! vocabulary has a help line here, and every line offered is a token the parser accepts — so the
//! list cannot drift away from the format.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::dsl;

/// What kind of word a command is — which is the section it is listed under in the reference
/// panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptCommandKind {
    /// A frame line's first token — the tick, absolute or `+delta`.
    Frame,

    /// A pad button of a frame line — `a`, `lt`, `by`.
    Button,

    /// A stick token of a frame line — `ls`, `lsx`, `rsy`.
    Stick,

    /// An attribute of the rules line — `name`, `trig`, `restart`.
    Rule,
}

/// One command of the script DSL as the reference lists it: the token the text spells it with,
/// that spelling with its arguments (`lsx:<value>`), and the one line of help next to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptCommandHelp {
    pub token: &'static str,
    pub spelling: &'static str,
    pub help: &'static str,
    pub kind: ScriptCommandKind,
}

impl ScriptCommandHelp {
    /// The token an autocomplete offers for this command: the spelling up to whatever argument
    /// marker it carries, so `lsx:<value>` completes to `lsx:` and `restart[:k=v,…]` to `restart`.
    pub fn spelling_prefix(&self) -> &'static str {
        let end = self
            .spelling
            .find([':', '['])
            .unwrap_or(self.spelling.len());
        match self.spelling.find(':') {
            Some(colon) if colon == end => &self.spelling[..=end],
            _ => &self.spelling[..end],
        }
    }
}

/// The first token of a frame line: the tick, absolute or relative. Listed on its own because it
/// is not a pad input at all — it says *when* the line's tokens fire.
pub const FRAME: ScriptCommandHelp = ScriptCommandHelp {
    token: "<frame>",
    spelling: "<frame> | +<frame>",
    help: "the simulation tick, or +N = N frames after the previous line (the first line is absolute)",
    kind: ScriptCommandKind::Frame,
};

/// The six stick tokens, listed after the buttons because the format writes them first on a line.
///
/// They are not flags of the DSL's input table — a stick is the shape of a whole command — so
/// the vocabulary does not carry them and they are spelled here.
pub const STICKS: [ScriptCommandHelp; 6] = [
    ScriptCommandHelp { token: "ls", spelling: "ls:<angle>", help: "full press by compass angle", kind: ScriptCommandKind::Stick },
    ScriptCommandHelp { token: "lsx", spelling: "lsx:<value>", help: "exact X axis of the left stick, ±1000", kind: ScriptCommandKind::Stick },
    ScriptCommandHelp { token: "lsy", spelling: "lsy:<value>", help: "exact Y axis of the left stick, ±1000", kind: ScriptCommandKind::Stick },
    ScriptCommandHelp { token: "rs", spelling: "rs:<angle>", help: "full camera press, the same compass as ls", kind: ScriptCommandKind::Stick },
    ScriptCommandHelp { token: "rsx", spelling: "rsx:<value>", help: "exact X axis of the camera, ±1000", kind: ScriptCommandKind::Stick },
    ScriptCommandHelp { token: "rsy", spelling: "rsy:<value>", help: "exact Y axis of the camera, ±1000", kind: ScriptCommandKind::Stick },
];

/// The rules line's attributes — a line of their own in the reference, since a rules line spells
/// no frames.
pub const RULES: [ScriptCommandHelp; 3] = [
    ScriptCommandHelp { token: "name", spelling: "name=<word>", help: "the script's name, no spaces", kind: ScriptCommandKind::Rule },
    ScriptCommandHelp { token: "trig", spelling: "trig=pos:x,y,z", help: "where the script starts — or trig=ticks:N for a tick count", kind: ScriptCommandKind::Rule },
    ScriptCommandHelp { token: "restart", spelling: "restart[:k=v,…]", help: "restart the mission from the pause menu first", kind: ScriptCommandKind::Rule },
];

/// What each token means on the pad.
static HELP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("a", "jump"),
        ("b", "zandatsu, the ninja kill"),
        ("x", "light attack"),
        ("y", "strong attack"),
        ("by", "Y+B on one line — the Execute prompt"),
        ("lt", "blade mode, held"),
        ("rt", "ninja run, held, with the stick"),
        ("lb", "sub-weapon"),
        ("rb", "lock-on, toggles"),
        ("r", "centre the camera"),
        ("lr", "ripper mode"),
        ("ax", "evade"),
        ("du", "augment mode"),
        ("dd", "use a recovery item"),
        ("dl", "access the inventory"),
        ("dr", "the inventory switch, the same flag as dl"),
        ("mu", "D-Pad up in a menu"),
        ("md", "D-Pad down in a menu"),
        ("ml", "D-Pad left in a menu"),
        ("mr", "D-Pad right in a menu"),
        ("ok", "confirm"),
        ("esc", "pause"),
        ("cd", "codec"),
        ("wk", "halves the left stick — walking pace"),
    ])
});

/// The frame line's commands: the vocabulary's tokens with their help, then the sticks.
///
/// A token of the vocabulary without a help line is left out rather than listed blank — a test
/// refuses that state instead of letting it pass unnoticed.
pub fn frame() -> Vec<ScriptCommandHelp> {
    let mut commands = Vec::new();

    for (token, _) in dsl::vocabulary() {
        if let Some(help) = HELP.get(token) {
            commands.push(ScriptCommandHelp {
                token,
                spelling: token,
                help,
                kind: ScriptCommandKind::Button,
            });
        }
    }

    commands.extend(STICKS);
    commands
}
