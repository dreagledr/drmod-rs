using System;
using System.Collections.Generic;

/// What kind of word a command is — which is the section it is listed under in the reference
/// panel.
internal enum ScriptCommandKind
{
    /// A pad button of a frame line — `a`, `lt`, `by`.
    Button,

    /// A stick token of a frame line — `ls`, `lsx`, `rsy`.
    Stick,

    /// An attribute of the rules line — `name`, `trig`, `restart`.
    Rule,
}

/// One command of the script DSL as the reference lists it: the token the text spells it with,
/// that spelling with its arguments (`lsx:<value>`), and the one line of help next to it.
internal sealed record ScriptCommandHelp(
    string Token,
    string Spelling,
    string Help,
    ScriptCommandKind Kind);

/// The DSL's vocabulary with help attached — what the region's reference panel shows.
///
/// The pad buttons are <see cref="ScriptDsl.Vocabulary"/>'s own tokens, not a second spelling of
/// them: this file only answers *what a token means*. Tests walk both directions — every token of
/// the vocabulary has a help line here, and every line offered is a token the parser accepts — so
/// the list cannot drift away from the format.
internal static class ScriptCommands
{
    /// The six stick tokens, listed after the buttons because the format writes them first on a
    /// line. They are not flags of the DSL's input table — a stick is the shape of a whole command
    /// — so the vocabulary does not carry them and they are spelled here.
    internal static readonly ScriptCommandHelp[] Sticks =
    [
        new("ls", "ls:<angle>", "full press by compass angle", ScriptCommandKind.Stick),
        new("lsx", "lsx:<value>", "exact X axis of the left stick, ±1000", ScriptCommandKind.Stick),
        new("lsy", "lsy:<value>", "exact Y axis of the left stick, ±1000", ScriptCommandKind.Stick),
        new("rs", "rs:<angle>", "full camera press, the same compass as ls", ScriptCommandKind.Stick),
        new("rsx", "rsx:<value>", "exact X axis of the camera, ±1000", ScriptCommandKind.Stick),
        new("rsy", "rsy:<value>", "exact Y axis of the camera, ±1000", ScriptCommandKind.Stick),
    ];

    /// The rules line's attributes — a line of their own in the reference, since a rules line
    /// spells no frames.
    internal static readonly ScriptCommandHelp[] Rules =
    [
        new("name", "name=<word>", "the script's name, no spaces", ScriptCommandKind.Rule),
        new("trig", "trig=pos:x,y,z", "where the script starts — or trig=ticks:N for a tick count", ScriptCommandKind.Rule),
        new("restart", "restart[:k=v,…]", "restart the mission from the pause menu first", ScriptCommandKind.Rule),
    ];

    /// The frame line's commands, in the order the command table's columns have them. The tokens
    /// come from the DSL; this is only what each one means on the pad.
    static readonly Dictionary<string, string> Help = new(StringComparer.OrdinalIgnoreCase)
    {
        ["a"] = "jump",
        ["b"] = "zandatsu, the ninja kill",
        ["x"] = "light attack",
        ["y"] = "strong attack",
        ["by"] = "Y+B on one line — the Execute prompt",
        ["lt"] = "blade mode, held",
        ["rt"] = "ninja run, held, with the stick",
        ["lb"] = "sub-weapon",
        ["rb"] = "lock-on, toggles",
        ["r"] = "centre the camera",
        ["lr"] = "ripper mode",
        ["ax"] = "evade",
        ["du"] = "augment mode",
        ["dd"] = "use a recovery item",
        ["dl"] = "access the inventory",
        ["dr"] = "the inventory switch, the same flag as dl",
        ["mu"] = "D-Pad up in a menu",
        ["md"] = "D-Pad down in a menu",
        ["ml"] = "D-Pad left in a menu",
        ["mr"] = "D-Pad right in a menu",
        ["ok"] = "confirm",
        ["esc"] = "pause",
        ["cd"] = "codec",
        ["wk"] = "halves the left stick — walking pace",
    };

    /// Buttons then sticks: the order the reference reads in, which is the docs table's. A token
    /// of the vocabulary without a help line is left out rather than listed blank — a test refuses
    /// that state instead of letting it pass unnoticed.
    internal static readonly ScriptCommandHelp[] Frame = BuildFrame();

    static ScriptCommandHelp[] BuildFrame()
    {
        var frame = new List<ScriptCommandHelp>(ScriptDsl.Vocabulary.Count + Sticks.Length);
        foreach (var (token, _) in ScriptDsl.Vocabulary)
        {
            if (Help.TryGetValue(token, out var help))
            {
                frame.Add(new ScriptCommandHelp(token, token, help, ScriptCommandKind.Button));
            }
        }

        frame.AddRange(Sticks);
        return [.. frame];
    }
}
