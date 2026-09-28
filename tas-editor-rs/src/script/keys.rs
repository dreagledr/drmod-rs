//! The two bit orders a script has, and the error the converters raise when they disagree with
//! the text.
//!
//! They are different lists on purpose and must not be merged:
//!
//! * [`CommandKeys::ALL`] is the *converter's* order — every boolean of `input`, including the
//!   four movement flags, because the frames carry them as a bit field.
//! * [`FlagKeys::ALL`] is the *table's* order — only the flags that have a DSL token, in the
//!   order a line writes them, which is also the order its columns are headed in.
//!
//! Both are index-aligned with the bit they stand for, so a column added to one cannot silently
//! miss the other: the tests walk them and assert each field lights its own bit.

use super::model::ScriptInput;

/// The boolean inputs a frame can hold, in column order — the converter's own list.
///
/// The header **is** the token the script DSL spells that input with, two exceptions worth
/// knowing: the four movement flags (`fu`/`fd`/`fl`/`fr`) have a header but no token of their own
/// — the DSL carries movement as the stick — and the names follow the console pad the mod
/// emulates (`a` jump, `b` zandatsu, `x` light attack, `y` heavy attack, triggers as `lt`/`rt`,
/// the D-pad as `du`/`dd`/`dl` and its menu twin as `mu`/`md`/`ml`/`mr`).
pub struct CommandKey {
    pub key: &'static str,
    pub label: &'static str,
    pub get: fn(&ScriptInput) -> bool,
    pub set: fn(ScriptInput) -> ScriptInput,
}

/// The 26 booleans of `ScriptInput`, in the order the converter's bit field uses.
pub struct CommandKeys;

impl CommandKeys {
    pub const ALL: [CommandKey; 26] = [
        CommandKey { key: "forward", label: "fu", get: |i| i.forward, set: |i| ScriptInput { forward: true, ..i } },
        CommandKey { key: "backward", label: "fd", get: |i| i.backward, set: |i| ScriptInput { backward: true, ..i } },
        CommandKey { key: "left", label: "fl", get: |i| i.left, set: |i| ScriptInput { left: true, ..i } },
        CommandKey { key: "right", label: "fr", get: |i| i.right, set: |i| ScriptInput { right: true, ..i } },
        CommandKey { key: "jump", label: "a", get: |i| i.jump, set: |i| ScriptInput { jump: true, ..i } },
        CommandKey { key: "light_attack", label: "x", get: |i| i.light_attack, set: |i| ScriptInput { light_attack: true, ..i } },
        CommandKey { key: "heavy_attack", label: "y", get: |i| i.heavy_attack, set: |i| ScriptInput { heavy_attack: true, ..i } },
        CommandKey { key: "ripper", label: "lr", get: |i| i.ripper, set: |i| ScriptInput { ripper: true, ..i } },
        CommandKey { key: "blade", label: "lt", get: |i| i.blade, set: |i| ScriptInput { blade: true, ..i } },
        CommandKey { key: "ninja_run", label: "rt", get: |i| i.ninja_run, set: |i| ScriptInput { ninja_run: true, ..i } },
        CommandKey { key: "walk", label: "wk", get: |i| i.walk, set: |i| ScriptInput { walk: true, ..i } },
        CommandKey { key: "dodge", label: "ax", get: |i| i.dodge, set: |i| ScriptInput { dodge: true, ..i } },
        CommandKey { key: "lock_on", label: "rb", get: |i| i.lock_on, set: |i| ScriptInput { lock_on: true, ..i } },
        CommandKey { key: "subweapon", label: "lb", get: |i| i.subweapon, set: |i| ScriptInput { subweapon: true, ..i } },
        CommandKey { key: "item", label: "dd", get: |i| i.item, set: |i| ScriptInput { item: true, ..i } },
        CommandKey { key: "ar_mode", label: "du", get: |i| i.ar_mode, set: |i| ScriptInput { ar_mode: true, ..i } },
        CommandKey { key: "weapon_select", label: "dl", get: |i| i.weapon_select, set: |i| ScriptInput { weapon_select: true, ..i } },
        CommandKey { key: "codec", label: "cd", get: |i| i.codec, set: |i| ScriptInput { codec: true, ..i } },
        CommandKey { key: "zandatsu", label: "b", get: |i| i.zandatsu, set: |i| ScriptInput { zandatsu: true, ..i } },
        CommandKey { key: "camera_reset", label: "r", get: |i| i.camera_reset, set: |i| ScriptInput { camera_reset: true, ..i } },
        CommandKey { key: "pause", label: "esc", get: |i| i.pause, set: |i| ScriptInput { pause: true, ..i } },
        CommandKey { key: "confirm", label: "ok", get: |i| i.confirm, set: |i| ScriptInput { confirm: true, ..i } },
        CommandKey { key: "menu_up", label: "mu", get: |i| i.menu_up, set: |i| ScriptInput { menu_up: true, ..i } },
        CommandKey { key: "menu_down", label: "md", get: |i| i.menu_down, set: |i| ScriptInput { menu_down: true, ..i } },
        CommandKey { key: "menu_left", label: "ml", get: |i| i.menu_left, set: |i| ScriptInput { menu_left: true, ..i } },
        CommandKey { key: "menu_right", label: "mr", get: |i| i.menu_right, set: |i| ScriptInput { menu_right: true, ..i } },
    ];

    /// The index of an `input` key, or `None` for one that is not a boolean.
    pub fn index_of(key: &str) -> Option<usize> {
        Self::ALL.iter().position(|entry| entry.key == key)
    }
}

/// The bit mask for a set of `input` keys, e.g. `command_mask(&["forward", "ninja_run"])`.
///
/// # Panics
///
/// On an unknown key rather than silently shifting out of range — a typo in a test would
/// otherwise just light up the wrong column.
pub fn command_mask(keys: &[&str]) -> u32 {
    let mut mask = 0u32;
    for key in keys {
        let bit = CommandKeys::index_of(key)
            .unwrap_or_else(|| panic!("unknown input key '{key}'"));
        mask |= 1u32 << bit;
    }

    mask
}

/// The DSL token a flag column belongs to, and the header it is painted under.
pub struct FlagKey {
    pub token: &'static str,
    pub key: &'static str,
}

/// The flag columns of the command table, headed and named by the token the DSL spells them
/// with.
///
/// `dr` is a token the parser accepts as a synonym of `dl`, and `by` — the game's Y+B Execute
/// prompt — is spelled as one token while lighting two columns; both are text spellings the
/// columns cannot show one-to-one, which is why they are not columns of their own.
pub struct FlagKeys;

impl FlagKeys {
    /// The token order of the DSL, which is also the order a frame line writes its tokens in.
    pub const ALL: [FlagKey; 22] = [
        FlagKey { token: "a", key: "jump" },
        FlagKey { token: "x", key: "light_attack" },
        FlagKey { token: "y", key: "heavy_attack" },
        FlagKey { token: "lr", key: "ripper" },
        FlagKey { token: "lt", key: "blade" },
        FlagKey { token: "rt", key: "ninja_run" },
        FlagKey { token: "wk", key: "walk" },
        FlagKey { token: "ax", key: "dodge" },
        FlagKey { token: "rb", key: "lock_on" },
        FlagKey { token: "lb", key: "subweapon" },
        FlagKey { token: "dd", key: "item" },
        FlagKey { token: "du", key: "ar_mode" },
        FlagKey { token: "dl", key: "weapon_select" },
        FlagKey { token: "cd", key: "codec" },
        FlagKey { token: "b", key: "zandatsu" },
        FlagKey { token: "r", key: "camera_reset" },
        FlagKey { token: "esc", key: "pause" },
        FlagKey { token: "ok", key: "confirm" },
        FlagKey { token: "mu", key: "menu_up" },
        FlagKey { token: "md", key: "menu_down" },
        FlagKey { token: "ml", key: "menu_left" },
        FlagKey { token: "mr", key: "menu_right" },
    ];

    /// The bit of a flag token, or `None`.
    pub fn index_of(token: &str) -> Option<usize> {
        Self::ALL.iter().position(|entry| entry.token == token)
    }
}

/// The bit mask for a set of flag tokens, e.g. `flag_mask(&["a", "lt"])`.
///
/// # Panics
///
/// On an unknown token, for the same reason [`command_mask`] does.
pub fn flag_mask(tokens: &[&str]) -> u32 {
    let mut mask = 0u32;
    for token in tokens {
        let bit = FlagKeys::index_of(token).unwrap_or_else(|| panic!("unknown token '{token}'"));
        mask |= 1u32 << bit;
    }

    mask
}
