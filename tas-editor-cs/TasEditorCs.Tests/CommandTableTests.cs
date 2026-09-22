using Microsoft.UI.Reactor.Controls;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Data;
using Microsoft.UI.Reactor.Data.Providers;

namespace TasEditorCs.Tests;

/// The command table. Like the other pane tests these are structural — nothing here creates a
/// WinUI control, and nothing here calls the cell or header templates, which allocate brushes
/// (a headless test host has no WinUI runtime and throws COMException if it tries).
///
/// The table visualizes the `.tas` text and is read-only, so what is asserted here is the shape
/// of the columns and the frames a text projects to — not editing, which belongs to the text
/// region.
public class CommandTableTests
{
    [Fact]
    public void Lays_out_the_frame_then_each_sticks_angle_and_axes_then_one_column_per_token()
    {
        var columns = Grid(Text).Columns!;

        // 1 frame + (angle, x, y) per stick + one column per DSL token.
        Assert.Equal(FlagKeys.All.Length + 7, columns.Count);
        Assert.Equal(
            new[] { "Frame", "ls", "lsx", "lsy", "rs", "rsx", "rsy" },
            columns.Take(7).Select(column => column.Name));
        Assert.Equal(FlagKeys.All.Select(key => key.Token), columns.Skip(7).Select(column => column.Name));
    }

    [Fact]
    public void Heads_every_column_with_its_dsl_token_except_the_frame()
    {
        var columns = Grid(Text).Columns!;

        // The token is the header, so the table doubles as the text format's legend — including the
        // sticks, whose header is the token the line actually writes (`ls`, `lsx`, `lsy`, …). The
        // frame is the one column the format does not spell as a token: a frame is the bare number
        // at the start of the line.
        Assert.Equal("#", columns[0].DisplayName);
        Assert.Equal(
            new[] { "ls", "lsx", "lsy", "rs", "rsx", "rsy" },
            columns.Skip(1).Take(6).Select(column => column.DisplayName));
        Assert.Equal(FlagKeys.All.Select(key => key.Token), columns.Skip(7).Select(column => column.DisplayName));
    }

    [Fact]
    public void Is_read_only_end_to_end()
    {
        var grid = Grid(Text);

        Assert.False(grid.Editable);
        Assert.Null(grid.OnRowChanged);
        Assert.Equal(SelectionMode.None, grid.SelectionMode);
        Assert.All(grid.Columns!, column =>
        {
            Assert.True(column.IsReadOnly);
            Assert.Null(column.SetValue);
            Assert.Null(column.Editor);
        });
    }

    [Fact]
    public void Keeps_the_frame_number_pinned()
    {
        Assert.Equal(PinPosition.Left, Grid(Text).Columns![0].Pin);
    }

    [Fact]
    public void Puts_the_cell_and_header_body_in_its_own_templates()
    {
        var grid = Grid(Text);

        Assert.NotNull(grid.CellTemplate);
        Assert.NotNull(grid.HeaderTemplate);
        Assert.NotNull(grid.PlaceholderCellTemplate);
        Assert.Equal(24d, grid.RowHeight);
    }

    [Fact]
    public void Opens_a_row_for_every_frame_the_text_touches_and_no_more()
    {
        // Frames run 0..last, so a gap the text writes nothing for still has a row — it is the
        // table's blank line, which is exactly what the format says by omitting it. `lt:3` on
        // frame 4 covers frames 4, 5 and 6, so the last frame is 6.
        var frames = Frames("0 a\n4 lt:3\n");

        Assert.Equal(7, frames.Count);
        Assert.Equal(Enumerable.Range(0, 7).Select(frame => (uint)frame), frames.Select(frame => frame.Frame));
        Assert.True(frames[0].Holds(FlagKeys.IndexOf("a")));
        Assert.False(frames[0].Holds(FlagKeys.IndexOf("lt")));
        Assert.False(frames[3].Holds(FlagKeys.IndexOf("lt")));
        Assert.True(frames[4].Holds(FlagKeys.IndexOf("lt")));
        Assert.True(frames[6].Holds(FlagKeys.IndexOf("lt")));
    }

    [Fact]
    public void Lights_the_column_of_every_token_the_line_writes()
    {
        var frame = Assert.Single(Frames("0 a x lt rt\n"));

        Assert.True(frame.Holds(FlagKeys.IndexOf("a")));
        Assert.True(frame.Holds(FlagKeys.IndexOf("x")));
        Assert.True(frame.Holds(FlagKeys.IndexOf("lt")));
        Assert.True(frame.Holds(FlagKeys.IndexOf("rt")));
        Assert.False(frame.Holds(FlagKeys.IndexOf("y")));
    }

    [Fact]
    public void An_angle_stick_fills_the_angle_column_and_leaves_the_axes_blank()
    {
        // The table shows the token the line wrote. `ls:0` is an angle, so it goes in the angle
        // column and the axis columns stay blank — the table does not resolve it into axes.
        var frame = Assert.Single(Frames("0 ls:0\n"));

        Assert.Equal(0.0, frame.Left.Angle);
        Assert.Null(frame.Left.X);
        Assert.Null(frame.Left.Y);
    }

    [Fact]
    public void Reads_every_compass_angle_the_way_the_text_writes_it()
    {
        // The number is passed through unchanged — the table is a picture of the text, so it does
        // not reinterpret the compass.
        Assert.Equal(0.0, Assert.Single(Frames("0 ls:0\n")).Left.Angle);
        Assert.Equal(90.0, Assert.Single(Frames("0 ls:90\n")).Left.Angle);
        Assert.Equal(180.0, Assert.Single(Frames("0 ls:180\n")).Left.Angle);
        Assert.Equal(270.0, Assert.Single(Frames("0 ls:270\n")).Left.Angle);
    }

    [Fact]
    public void An_exact_stick_fills_the_axes_and_leaves_the_angle_blank()
    {
        var frame = Assert.Single(Frames("0 lsx:300 lsy:-800\n"));

        Assert.Null(frame.Left.Angle);
        Assert.Equal(300.0, frame.Left.X);
        Assert.Equal(-800.0, frame.Left.Y);
    }

    [Fact]
    public void The_right_stick_is_the_camera_tokens()
    {
        var angle = Assert.Single(Frames("0 rs:90\n"));

        Assert.Equal(90.0, angle.Right.Angle);
        Assert.Null(angle.Right.X);
        Assert.Null(angle.Left.Angle);

        var axes = Assert.Single(Frames("0 rsx:6500\n"));

        Assert.Equal(6500.0, axes.Right.X);
        Assert.Null(axes.Right.Y);
        Assert.Null(axes.Right.Angle);
    }

    [Fact]
    public void Keeps_an_axis_the_line_left_out_blank_rather_than_zero()
    {
        // `lsx` alone says nothing about Y, and the table shows exactly that: the Y column is
        // blank, not 0 — the text never wrote a Y.
        var frame = Assert.Single(Frames("0 lsx:500\n"));

        Assert.Equal(500.0, frame.Left.X);
        Assert.Null(frame.Left.Y);
    }

    [Fact]
    public void Shows_the_value_of_a_stick_held_for_several_frames()
    {
        // The duration rides behind a second colon; the value is the first argument.
        var frames = Frames("0 lsx:500:3\n");

        Assert.Equal(3, frames.Count);
        Assert.All(frames, frame => Assert.Equal(500.0, frame.Left.X));
    }

    [Fact]
    public void Draws_the_tokens_the_line_carries_and_nothing_else()
    {
        // The contract in one place: `ls:270` is an angle and stays an angle — the table does not
        // resolve it into the axes the mod would assemble, because the text never said those.
        var frame = Assert.Single(Frames("0 ls:270 lt\n"));

        Assert.Equal(270.0, frame.Left.Angle);
        Assert.Null(frame.Left.X);
        Assert.Null(frame.Left.Y);
        Assert.Null(frame.Right.Angle);

        // And the flag is the column of its own token, lit.
        Assert.True(frame.Holds(FlagKeys.IndexOf("lt")));
    }

    [Fact]
    public void Keeps_a_multi_frame_run_on_every_frame_it_covers()
    {
        // `lt:3` covers frames 0, 1 and 2: each is a row with the column lit, not just the first.
        var frames = Frames("0 lt:3\n");

        Assert.Equal(3, frames.Count);
        Assert.All(frames, frame => Assert.True(frame.Holds(FlagKeys.IndexOf("lt"))));
        Assert.All(frames, frame => Assert.True(frame.Held));
    }

    [Fact]
    public void Says_nothing_when_the_text_does_not_parse()
    {
        // The text region carries the parser's message, so the table stays silent rather than
        // saying the same thing a third way.
        var status = ScriptTextStatus.Of(ScriptDsl.Lines("0 zz\n"));

        Assert.False(status.IsOk);
        Assert.Empty(CommandTable.Frames(ScriptDsl.Lines("0 zz\n")));
    }

    [Fact]
    public void Shows_the_frame_number_the_text_writes()
    {
        var frames = Frames("40 a\n");

        Assert.Equal(41, frames.Count);
        Assert.Equal(40u, frames[40].Frame);
    }

    [Fact]
    public void Never_paints_a_cell_with_the_same_surface_as_its_neighbour()
    {
        var up = CommandTable.CellSurface(0, "Frame");

        Assert.NotEqual(up, CommandTable.CellSurface(0, "ls"));        // across a column
        Assert.NotEqual(up, CommandTable.CellSurface(1, "Frame"));      // down a row
        Assert.Equal(up, CommandTable.CellSurface(2, "Frame"));         // two down is the same parity
    }

    const string Text = "! trig=ticks:0\n0 a\n";

    internal static IReadOnlyList<ScriptFrame> Frames(string text) =>
        CommandTable.Frames(ScriptDsl.Lines(text));

    static DataGridElement<ScriptFrame> Grid(string text)
    {
        var root = Assert.IsType<FlexElement>(CommandTable.View(Source(text)));
        return Assert.IsType<ComponentElement<DataGridElement<ScriptFrame>>>(Assert.Single(root.Children)).Props;
    }

    static ListDataSource<ScriptFrame> Source(string text) =>
        new(CommandTable.Frames(ScriptDsl.Lines(text)), row => (RowKey)(int)row.Frame);
}
