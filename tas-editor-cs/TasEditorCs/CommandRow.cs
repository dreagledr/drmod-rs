using System;
using System.Collections.Generic;

/// One frame of a script in the converter's own shape: each stick is a direction in degrees
/// plus a deflection magnitude, and every other input is a boolean.
///
/// The command table shows <see cref="ScriptFrame"/> instead — the DSL's terms, where a stick
/// is an angle or exact axes — because the table visualizes the `.tas` text. This is the shape
/// the text converter works in (`ScriptFrames.Collapse`/`Expand`), which is what turns a
/// document into `.tas` text and back.
sealed record CommandRow(
    int Frame,
    double LeftStickAngle,
    double RightStickAngle,
    double LeftStickAmount,
    double RightStickAmount,
    uint Buttons)
{
    /// Whether the input at `bit` — an index into <see cref="CommandKeys.All"/> — is held on
    /// this frame.
    internal bool Holds(int bit) => (Buttons & (1u << bit)) != 0;
}

/// The boolean inputs a frame can hold, in column order. `Key` is the script's `input` key
/// (`docs/API.md` §4.2 in the sibling repo), `Label` the column header.
///
/// The header **is** the token the script DSL spells that input with (`docs/SCRIPT_DSL.md`), so
/// the table doubles as the text format's legend. Two consequences worth knowing: the four
/// movement flags (`fu`/`fd`/`fl`/`fr`) have a header but no token of their own — the DSL carries
/// movement as the stick — and the names follow the console pad that the mod emulates (`a` jump,
/// `b` zandatsu, `x` light attack, `y` heavy attack, triggers as `lt`/`rt`, the D-pad as
/// `du`/`dd`/`dl` and its menu twin as `mu`/`md`/`ml`/`mr`).
internal static class CommandKeys
{
    internal static readonly (string Key, string Label)[] All =
    [
        ("forward", "fu"),
        ("backward", "fd"),
        ("left", "fl"),
        ("right", "fr"),
        ("jump", "a"),
        ("light_attack", "x"),
        ("heavy_attack", "y"),
        ("ripper", "lr"),
        ("blade", "lt"),
        ("ninja_run", "rt"),
        ("walk", "wk"),
        ("dodge", "ax"),
        ("lock_on", "rb"),
        ("subweapon", "lb"),
        ("item", "dd"),
        ("ar_mode", "du"),
        ("weapon_select", "dl"),
        ("codec", "cd"),
        ("zandatsu", "b"),
        ("camera_reset", "r"),
        ("pause", "esc"),
        ("confirm", "ok"),
        ("menu_up", "mu"),
        ("menu_down", "md"),
        ("menu_left", "ml"),
        ("menu_right", "mr"),
    ];

    /// The bit mask for a set of `input` keys, e.g. `Mask("forward", "ninja_run")`. Throws on
    /// an unknown key rather than silently shifting out of range — a typo in the phase table
    /// below would otherwise just light up the wrong column.
    internal static uint Mask(params string[] keys)
    {
        var mask = 0u;
        foreach (var key in keys)
        {
            var bit = IndexOf(key);
            if (bit < 0) throw new ArgumentException($"Unknown input key '{key}'.", nameof(keys));
            mask |= 1u << bit;
        }
        return mask;
    }

    static int IndexOf(string key)
    {
        for (var bit = 0; bit < All.Length; bit++)
        {
            if (All[bit].Key == key) return bit;
        }
        return -1;
    }
}
