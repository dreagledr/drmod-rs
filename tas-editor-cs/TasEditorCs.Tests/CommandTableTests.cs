using Microsoft.UI.Reactor.Controls;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Data;
using Microsoft.UI.Reactor.Data.Providers;

namespace TasEditorCs.Tests;

/// The command table. Like the other pane tests these are structural — nothing here creates a
/// WinUI control, and nothing here calls the cell or header templates, which allocate brushes
/// (a headless test host has no WinUI runtime and throws COMException if it tries).
public class CommandTableTests
{
    [Fact]
    public void Lays_out_the_frame_the_four_stick_values_and_one_column_per_input()
    {
        var columns = Grid(new ScriptEntry("s1", "blade-run", 240)).Columns!;

        Assert.Equal(CommandKeys.All.Length + 5, columns.Count);
        Assert.Equal(
            new[] { "Frame", "left_stick_angle", "right_stick_angle", "left_stick_amount", "right_stick_amount" },
            columns.Take(5).Select(column => column.Name));
        Assert.Equal(CommandKeys.All.Select(key => key.Key), columns.Skip(5).Select(column => column.Name));
    }

    [Fact]
    public void Heads_every_input_column_with_a_one_or_two_letter_label()
    {
        var inputs = Grid(new ScriptEntry("s1", "blade-run", 240)).Columns!.Skip(5).ToList();

        Assert.All(inputs, column => Assert.InRange(column.DisplayName!.Length, 1, 2));
        Assert.Equal(CommandKeys.All.Select(key => key.Label), inputs.Select(column => column.DisplayName));
    }

    [Fact]
    public void Keeps_the_frame_number_pinned_and_the_grid_read_only()
    {
        var grid = Grid(new ScriptEntry("s1", "blade-run", 240));

        Assert.Equal(PinPosition.Left, grid.Columns![0].Pin);
        Assert.All(grid.Columns, column => Assert.True(column.IsReadOnly));
        Assert.False(grid.Editable);
        Assert.Null(grid.OnRowChanged);
        Assert.Equal(SelectionMode.None, grid.SelectionMode);
    }

    [Fact]
    public void Puts_the_cell_and_header_body_in_its_own_templates()
    {
        var grid = Grid(new ScriptEntry("s1", "blade-run", 240));

        Assert.NotNull(grid.CellTemplate);
        Assert.NotNull(grid.HeaderTemplate);
        Assert.NotNull(grid.PlaceholderCellTemplate);
        // Rows butt against each other only while the pitch is a fixed row height.
        Assert.Equal(18d, grid.RowHeight);
    }

    [Fact]
    public void Expands_the_mock_script_to_exactly_one_row_per_frame()
    {
        var rows = CommandRows.For(new ScriptEntry("s1", "blade-run", 120));

        Assert.Equal(120, rows.Count);
        Assert.Equal(Enumerable.Range(0, 120), rows.Select(row => row.Frame));
        Assert.All(rows, row =>
        {
            Assert.InRange(row.LeftStickAngle, 0d, 359.999d);
            Assert.InRange(row.RightStickAngle, 0d, 359.999d);
            Assert.InRange(row.LeftStickAmount, 0d, 1d);
            Assert.InRange(row.RightStickAmount, 0d, 1d);
            // Only the inputs the table has columns for — a stray bit would be invisible.
            Assert.True(row.Buttons < 1u << CommandKeys.All.Length);
        });
    }

    [Fact]
    public void Opens_the_mock_script_on_its_first_run_phase()
    {
        var first = CommandRows.For(new ScriptEntry("s1", "blade-run", 240))[0];

        // Phases[0] is the long run, so the table never opens on a blank first row.
        Assert.True(first.Holds(IndexOf("forward")));
        Assert.True(first.Holds(IndexOf("ninja_run")));
        Assert.Equal(1d, first.LeftStickAmount);
    }

    [Fact]
    public void Never_paints_a_cell_with_the_same_surface_as_its_neighbour()
    {
        var up = CommandTable.CellSurface(0, "Frame");

        Assert.NotEqual(up, CommandTable.CellSurface(0, "left_stick_angle"));   // across a column
        Assert.NotEqual(up, CommandTable.CellSurface(1, "Frame"));              // down a row
        Assert.Equal(up, CommandTable.CellSurface(2, "Frame"));                 // two down is the same parity
    }

    static int IndexOf(string key) =>
        Array.FindIndex(CommandKeys.All, entry => entry.Key == key);

    static DataGridElement<CommandRow> Grid(ScriptEntry script)
    {
        var source = new ListDataSource<CommandRow>(CommandRows.For(script), row => (RowKey)row.Frame);
        var root = Assert.IsType<FlexElement>(CommandTable.View(source));
        return Assert.IsType<ComponentElement<DataGridElement<CommandRow>>>(Assert.Single(root.Children)).Props;
    }
}
