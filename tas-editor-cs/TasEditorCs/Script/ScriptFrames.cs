using System;
using System.Collections.Generic;
using System.Linq;

/// The text converter's view of a document: one <see cref="CommandRow"/> per frame, in the
/// angle-plus-deflection shape the converter collapses back into commands.
///
/// The command table does not use this — it shows <see cref="ScriptFrame"/>, the DSL's own
/// terms. This is the intermediate the text writer goes through.
///
/// Only frame-level inputs project: `raw_key`, `dik_key` and `when_enemy` have no
/// column, so commands carrying them are left out — and a document collapsed back from
/// the frames loses them. The tests pin that down instead of hiding it.
///
/// The projection is lossy by nature, which is why the JSON stays the source of
/// truth: a documented stick arrives as an angle plus a deflection, and the axes
/// come back from the trigonometry within a few thousandths. A command whose
/// inputs the table cannot show at all (a `camera` of exactly `[0, 0]`) also loses
/// its frames — nothing of it reaches a row.
internal static class ScriptFrames
{
    /// Raw axis units per deflection unit, as in the DSL: `1.0` is a full axis,
    /// `√2` the corner — the JSON's own `[-1000, 1000]`.
    const double AxisUnit = 1000.0;

    /// Where a stick that is at rest points: a released stick keeps its last
    /// direction, so a run of frames does not snap to zero between two pushes. A
    /// full push forward is the angle 270 in this shape (0 in the DSL's compass).
    const double RestAngle = 270.0;

    /// Axes closer than this are the same value: `cos 270°` is 6·10⁻¹⁷, which is a
    /// direction only to a mathematician.
    const double AxisEpsilon = 1e-6;

    /// Axes go back to the document rounded to this many decimals — the same
    /// precision the DSL keeps.
    const int AxisDecimals = 3;

    // The bit `walk` halves the finished stick with — the mod's own way of encoding
    // walking, which is a magnitude rather than a key.
    static readonly uint Walk = CommandKeys.Mask("walk");

    /// The frames of a document: 0 up to the end of its last projectable command.
    /// Commands keep the order they appear in, so a stick held by two of them is
    /// the later one — the document's own last-wins rule.
    internal static IReadOnlyList<CommandRow> Expand(ScriptDocument document)
    {
        var commands = document.Commands
            .Where(command => command.WhenEnemy is null && command.Input.RawKey is null && command.Input.DikKey is null)
            .ToList();

        var total = commands.Count == 0 ? 0 : commands.Max(command => command.T + command.Duration);
        var rows = new List<CommandRow>((int)total);
        var leftAngle = RestAngle;
        var rightAngle = 0.0;

        for (uint frame = 0; frame < total; frame++)
        {
            var buttons = 0u;
            double[]? left = null;
            double[]? right = null;
            foreach (var command in commands)
            {
                if (frame < command.T || frame >= command.T + command.Duration)
                {
                    continue;
                }

                buttons |= Bits(command.Input);

                // The last command to set the stick is what the game sees: the mod
                // assigns it per command (`script_tick`), so a later command's
                // directions replace an earlier command's explicit stick, and back.
                if (CommandedStick(command.Input) is { } commanded)
                {
                    left = commanded;
                }

                if (command.Input.Camera is { } camera)
                {
                    right = Axes(camera);
                }
            }

            // Walking is a magnitude the game reads rather than a key: the mod
            // halves the stick it ended up with, however it got there.
            if ((buttons & Walk) != 0 && left is not null)
            {
                left = [left[0] / 2, left[1] / 2];
            }

            var (nextLeftAngle, leftAmount) = Polar(left ?? [0.0, 0.0]);
            var (nextRightAngle, rightAmount) = Polar(right ?? [0.0, 0.0]);
            if (leftAmount > 0)
            {
                leftAngle = nextLeftAngle;
            }

            if (rightAmount > 0)
            {
                rightAngle = nextRightAngle;
            }

            rows.Add(new CommandRow((int)frame, leftAngle, rightAngle, leftAmount, rightAmount, buttons));
        }

        return rows;
    }

    /// The frames back to commands: one command per run of identical frames, and
    /// no command for a run where nothing is held — the mod's own reading of a
    /// frame without input.
    internal static List<ScriptCommand> Collapse(IReadOnlyList<CommandRow> frames)
    {
        var commands = new List<ScriptCommand>();
        var start = 0;
        while (start < frames.Count)
        {
            if (Empty(frames[start]))
            {
                start++;
                continue;
            }

            var end = start;
            while (end + 1 < frames.Count && !Empty(frames[end + 1]) && Same(frames[end], frames[end + 1]))
            {
                end++;
            }

            commands.Add(Command(frames[start], (uint)frames[start].Frame, (uint)(frames[end].Frame - frames[start].Frame + 1)));
            start = end + 1;
        }

        return commands;
    }

    /// One run of frames as a command: its booleans, a stick only where the
    /// directions do not imply it, and a camera only while the stick is off centre.
    static ScriptCommand Command(CommandRow row, uint t, uint duration)
    {
        var input = new ScriptInput();
        for (var bit = 0; bit < ScriptInput.Booleans.Length; bit++)
        {
            if (row.Holds(bit))
            {
                input = ScriptInput.Booleans[bit].Set(input);
            }
        }

        var left = Vector(row.LeftStickAngle, row.LeftStickAmount);
        if (!Same(left, Implied(input)))
        {
            input = input with { LeftStick = Rounded(left) };
        }

        if (row.RightStickAmount > 0)
        {
            input = input with { Camera = Rounded(Vector(row.RightStickAngle, row.RightStickAmount)) };
        }

        return new ScriptCommand { T = t, Duration = duration, Input = input };
    }

    /// The bit field of an input, one bit per command-table column.
    static uint Bits(ScriptInput input)
    {
        var buttons = 0u;
        for (var bit = 0; bit < ScriptInput.Booleans.Length; bit++)
        {
            if (ScriptInput.Booleans[bit].Get(input))
            {
                buttons |= 1u << bit;
            }
        }

        return buttons;
    }

    /// The stick a command feeds the game **before** walking halves it: the explicit
    /// one, or the one its movement flags stand for. `null` when the command says
    /// nothing about the stick — which is not the same as a stick at rest, since a
    /// command that says nothing leaves the previous value standing.
    internal static double[]? CommandedStick(ScriptInput input) =>
        input.LeftStick is { } stick ? Axes(stick) : Directions(input);

    /// The stick one command's movement flags stand for, as the mod assembles it:
    /// each direction adds a full axis (`forward` is (0, −1000)) and diagonals add
    /// up. `null` when the command moves nowhere; `walk` is not part of it — the
    /// mod halves the finished stick instead.
    static double[]? Directions(ScriptInput input)
    {
        double x = 0;
        double y = 0;
        if (input.Forward)
        {
            y -= AxisUnit;
        }

        if (input.Backward)
        {
            y += AxisUnit;
        }

        if (input.Left)
        {
            x -= AxisUnit;
        }

        if (input.Right)
        {
            x += AxisUnit;
        }

        return x == 0 && y == 0 ? null : [x, y];
    }

    /// The stick a command ends up with when it carries nothing explicit — its own
    /// directions, halved by `walk` as the mod halves the finished stick.
    static double[] Implied(ScriptInput input)
    {
        var axes = Directions(input) ?? [0.0, 0.0];
        return input.Walk ? [axes[0] / 2, axes[1] / 2] : axes;
    }

    /// Axis values as the table's pair: an angle in degrees from +X and a
    /// deflection, where 1 is a full axis.
    static (double Angle, double Deflection) Polar(double[] axes)
    {
        var angle = Math.Atan2(axes[1], axes[0]) * 180.0 / Math.PI;
        if (angle < 0)
        {
            angle += 360.0;
        }

        var length = Math.Sqrt(axes[0] * axes[0] + axes[1] * axes[1]) / AxisUnit;
        return (Math.Round(angle, AxisDecimals), length);
    }

    /// An input's axes as the projection's own numbers.
    static double[] Axes(float[] axes) => [axes[0], axes[1]];

    /// The table's pair back to axis values.
    static double[] Vector(double angle, double deflection)
    {
        var radians = angle * Math.PI / 180.0;
        return [AxisUnit * deflection * Math.Cos(radians), AxisUnit * deflection * Math.Sin(radians)];
    }

    /// Axis values rounded for a document, with the `-0` of a 90° cosine snapped
    /// away — it would otherwise read as a direction.
    static float[] Rounded(double[] axes) => [(float)Math.Round(axes[0], AxisDecimals) + 0f, (float)Math.Round(axes[1], AxisDecimals) + 0f];

    /// Whether two frames hold the same thing — what makes a run a run. Axes
    /// compare with a tolerance: an angle and a deflection that describe the same
    /// direction must not split a run over 10⁻¹³.
    static bool Same(CommandRow a, CommandRow b) =>
        a.Buttons == b.Buttons
        && Same(Vector(a.LeftStickAngle, a.LeftStickAmount), Vector(b.LeftStickAngle, b.LeftStickAmount))
        && Same(Vector(a.RightStickAngle, a.RightStickAmount), Vector(b.RightStickAngle, b.RightStickAmount));

    static bool Same(double[] a, double[] b) =>
        Math.Abs(a[0] - b[0]) < AxisEpsilon && Math.Abs(a[1] - b[1]) < AxisEpsilon;

    static bool Empty(CommandRow row) =>
        row.Buttons == 0 && row.LeftStickAmount == 0 && row.RightStickAmount == 0;
}
