using System;

/// One frame of the command table, holding each stick exactly as the `.tas` text spells it.
///
/// A stick is written in one of two forms, and the frame keeps whichever the text used rather
/// than translating between them: `ls:<angle>` is a direction, `lsx`/`lsy` (and `rsx`/`rsy`) are
/// exact axis values. The table paints each into its own column, so a row reads as the line the
/// format would write for that frame.
///
/// Read-only: the table shows a script, it does not edit one.
sealed record ScriptFrame(
    uint Frame,
    StickValue Left,
    StickValue Right,
    uint Buttons,
    bool Held)
{
    /// Whether the input at `bit` — an index into <see cref="FlagKeys.All"/> — is held on this
    /// frame.
    internal bool Holds(int bit) => (Buttons & (1u << bit)) != 0;
}

/// One stick of one frame, in the form the text wrote it. The two forms are exclusive, because a
/// line cannot say both (the parser refuses it) — so exactly one of the angle and the axes is
/// present, and the other is not.
sealed record StickValue(double? Angle, double? X, double? Y)
{
    /// A stick the frame does not mention: both forms absent, which is the blank columns.
    internal static readonly StickValue None = new(null, null, null);

    /// The `ls:<angle>` form.
    internal static StickValue Direction(double angle) => new(angle, null, null);

    /// The `lsx` axis of the `lsx`/`lsy` form, added to whatever the line has said so far.
    internal StickValue WithX(double x) => this with { X = x };

    /// The `lsy` axis of the `lsx`/`lsy` form.
    internal StickValue WithY(double y) => this with { Y = y };
}

/// The DSL token a flag column belongs to, and the header it is painted under.
///
/// The token **is** the column name and the header **is** that token: the DSL spells every flag
/// column with exactly one token, so there is no separate label to keep in step. `dr` is a token
/// the parser accepts as a synonym of `dl`, and `by` — the game's Y+B Execute prompt — is spelled
/// as one token while lighting two columns; both are text spellings the columns cannot show
/// one-to-one, which is why they are not columns of their own.
///
/// Order is the DSL's token order, which is also the order a frame line writes its tokens in.
internal static class FlagKeys
{
    internal static readonly (string Token, string Key)[] All =
    [
        ("a", "jump"),
        ("x", "light_attack"),
        ("y", "heavy_attack"),
        ("lr", "ripper"),
        ("lt", "blade"),
        ("rt", "ninja_run"),
        ("wk", "walk"),
        ("ax", "dodge"),
        ("rb", "lock_on"),
        ("lb", "subweapon"),
        ("dd", "item"),
        ("du", "ar_mode"),
        ("dl", "weapon_select"),
        ("cd", "codec"),
        ("b", "zandatsu"),
        ("r", "camera_reset"),
        ("esc", "pause"),
        ("ok", "confirm"),
        ("mu", "menu_up"),
        ("md", "menu_down"),
        ("ml", "menu_left"),
        ("mr", "menu_right"),
    ];

    /// The bit mask for a set of tokens, e.g. `Mask("a", "lt")`. Throws on an unknown token
    /// rather than silently shifting out of range — a typo in a test would otherwise just light
    /// up the wrong column.
    internal static uint Mask(params string[] tokens)
    {
        var mask = 0u;
        foreach (var token in tokens)
        {
            var bit = IndexOf(token);
            if (bit < 0) throw new ArgumentException($"Unknown token '{token}'.", nameof(tokens));
            mask |= 1u << bit;
        }

        return mask;
    }

    internal static int IndexOf(string token) =>
        Array.FindIndex(All, entry => entry.Token == token);
}
