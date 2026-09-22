using System;
using System.Collections.Generic;
using System.Globalization;

/// The command table's view of a script text: one <see cref="ScriptFrame"/> per frame, showing the
/// tokens exactly as the lines write them.
///
/// The table reads the text, not the parsed document. A document has already resolved `ls:<angle>`
/// into axis numbers and the movement flags into a stick, so the two spellings are
/// indistinguishable by the time it exists — but the table is meant to show what the script *says*,
/// and the line says `ls:0` or `lsx:500`, not a pair of numbers that happens to mean the same
/// thing. Reading the tokens keeps the table a picture of the text rather than a second opinion
/// about it.
///
/// Frames run from 0 to the end of the last token held, so a row exists for every frame a `.tas`
/// could name; the frames no token touches are the table's blank rows, which is what the format
/// itself says by writing nothing.
internal static class ScriptFrameProjection
{
    /// The frames of a script text, or nothing when it does not parse — the text region carries
    /// the parser's message, so the table stays silent rather than repeating it.
    internal static IReadOnlyList<ScriptFrame> Project(string text)
    {
        // One entry per frame, grown on demand: a token held for N frames fills N of them.
        var rows = new Dictionary<uint, ScriptFrame>();

        foreach (var line in ScriptDsl.Lines(text).Split('\n'))
        {
            if (!TryReadLine(line, out var number, out var tokens))
            {
                continue;
            }

            foreach (var token in tokens)
            {
                Hold(rows, number, token);
            }
        }

        return Fill(rows);
    }

    /// Fills frames `number` .. `number + token.Duration - 1` with the column that token carries.
    ///
    /// A token says one thing — a flag, or one stick — so each write fills one column for the whole
    /// run. On a frame two tokens both touch, the longer run wins, which is the format's own "OR the
    /// bits, keep the longest" rule; for a stick it means the later token's value stands, matching
    /// the mod's last-wins assignment.
    static void Hold(Dictionary<uint, ScriptFrame> rows, uint number, Token token)
    {
        for (var offset = 0u; offset < token.Duration; offset++)
        {
            var frame = number + offset;
            var row = rows.TryGetValue(frame, out var existing)
                ? existing
                : new ScriptFrame(frame, StickValue.None, StickValue.None, 0, Held: true);
            rows[frame] = token.Apply(row);
        }
    }

    /// One row per frame from 0 to the last one a token touched, so a gap the text leaves is a
    /// blank row rather than a missing one.
    static IReadOnlyList<ScriptFrame> Fill(Dictionary<uint, ScriptFrame> rows)
    {
        var total = 0u;
        foreach (var frame in rows.Keys)
        {
            if (frame + 1 > total)
            {
                total = frame + 1;
            }
        }

        var frames = new List<ScriptFrame>((int)total);
        for (uint frame = 0; frame < total; frame++)
        {
            frames.Add(rows.TryGetValue(frame, out var row)
                ? row
                : new ScriptFrame(frame, StickValue.None, StickValue.None, 0, Held: false));
        }

        return frames;
    }

    /// One token of a frame line: what it does, and how many frames it holds for.
    readonly record struct Token(uint Duration, Func<ScriptFrame, ScriptFrame> Apply);

    /// One frame line: its number, and the tokens that follow it. A blank line, a comment and the
    /// rules line are not frames.
    static bool TryReadLine(string line, out uint number, out List<Token> tokens)
    {
        number = 0;
        tokens = [];

        var text = StripComment(line).Trim();
        if (text.Length == 0 || text[0] == '!')
        {
            return false;
        }

        var parts = text.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length == 0 || !uint.TryParse(parts[0], NumberStyles.None, CultureInfo.InvariantCulture, out number))
        {
            return false;
        }

        for (var index = 1; index < parts.Length; index++)
        {
            if (Read(parts[index]) is { } token)
            {
                tokens.Add(token);
            }
        }

        return true;
    }

    /// One token as the write it performs. An unknown token, a stick without a value and a
    /// malformed one are skipped — the table shows what it can read, and the parse refuses the
    /// line separately (which is what empties the table anyway).
    static Token? Read(string token)
    {
        var separator = token.IndexOf(':');
        var key = (separator < 0 ? token : token[..separator]).ToLowerInvariant();
        var arguments = separator < 0 ? [] : token[(separator + 1)..].Split(':');

        switch (key)
        {
            // A stick carries its value first, so its duration is the second argument.
            case "ls":
                return ReadStick(arguments, ReadDuration(arguments, at: 1), left: true, angle: true);
            case "rs":
                return ReadStick(arguments, ReadDuration(arguments, at: 1), left: false, angle: true);
            case "lsx":
                return ReadStick(arguments, ReadDuration(arguments, at: 1), left: true, angle: false, axis: 0);
            case "lsy":
                return ReadStick(arguments, ReadDuration(arguments, at: 1), left: true, angle: false, axis: 1);
            case "rsx":
                return ReadStick(arguments, ReadDuration(arguments, at: 1), left: false, angle: false, axis: 0);
            case "rsy":
                return ReadStick(arguments, ReadDuration(arguments, at: 1), left: false, angle: false, axis: 1);
        }

        // A flag has no value of its own, so its single argument is the duration when there is one.
        var bit = FlagKeys.IndexOf(key);
        return bit < 0
            ? null
            : new Token(ReadDuration(arguments, at: 0), row => row with { Buttons = row.Buttons | 1u << bit });
    }

    /// A stick token as its write. The angle form is the whole value; an axis form sets one axis
    /// and leaves the other as the line left it, so a `lsx` with no `lsy` shows a blank Y.
    static Token? ReadStick(string[] arguments, uint duration, bool left, bool angle, int axis = -1)
    {
        if (arguments.Length == 0 || !TryNumber(arguments[0], out var value))
        {
            return null;
        }

        if (angle)
        {
            var direction = StickValue.Direction(value);
            return new Token(duration, row => left ? row with { Left = direction } : row with { Right = direction });
        }

        return new Token(duration, row =>
        {
            var stick = left ? row.Left : row.Right;
            stick = axis == 0 ? stick.WithX(value) : stick.WithY(value);
            return left ? row with { Left = stick } : row with { Right = stick };
        });
    }

    /// The duration a token holds for, taken from the argument at `at`: `1` when it is absent or
    /// unreadable — the DSL's own default for a token that names none.
    static uint ReadDuration(string[] arguments, int at) =>
        arguments.Length > at
        && uint.TryParse(arguments[at], NumberStyles.None, CultureInfo.InvariantCulture, out var frames)
        && frames >= 1
            ? frames
            : 1u;

    static bool TryNumber(string value, out double number) =>
        double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out number);

    static string StripComment(string line)
    {
        var comment = line.IndexOf('#');
        return comment < 0 ? line : line[..comment];
    }
}
