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
        var columns = Grid(Script(240)).Columns!;

        Assert.Equal(CommandKeys.All.Length + 5, columns.Count);
        Assert.Equal(
            new[] { "Frame", "left_stick_angle", "right_stick_angle", "left_stick_amount", "right_stick_amount" },
            columns.Take(5).Select(column => column.Name));
        Assert.Equal(CommandKeys.All.Select(key => key.Key), columns.Skip(5).Select(column => column.Name));
    }

    [Fact]
    public void Heads_every_input_column_with_the_token_the_script_text_uses()
    {
        var inputs = Grid(Script(240)).Columns!.Skip(5).ToList();

        // The header is the DSL's own spelling of that input, so the table doubles as the text
        // format's legend — short, but words are allowed (`esc`, `start` is not a key on PC).
        Assert.All(inputs, column => Assert.InRange(column.DisplayName!.Length, 1, 3));
        Assert.Equal(CommandKeys.All.Select(key => key.Label), inputs.Select(column => column.DisplayName));
    }

    [Fact]
    public void Keeps_the_frame_number_pinned_and_the_grid_editable()
    {
        var grid = Grid(Script(240));

        Assert.Equal(PinPosition.Left, grid.Columns![0].Pin);
        Assert.True(grid.Editable);
        Assert.NotNull(grid.OnRowChanged);
        Assert.Equal(SelectionMode.None, grid.SelectionMode);
    }

    [Fact]
    public void Leaves_the_frame_column_read_only_and_the_rest_editable()
    {
        var columns = Grid(Script(240)).Columns!;

        Assert.True(columns[0].IsReadOnly);
        Assert.Null(columns[0].SetValue);
        Assert.Null(columns[0].Editor);

        Assert.All(columns.Skip(1), column =>
        {
            Assert.False(column.IsReadOnly);
            Assert.NotNull(column.SetValue);
            Assert.NotNull(column.Editor);
        });
    }

    [Fact]
    public void A_flag_edit_moves_only_its_own_bit()
    {
        var columns = Grid(Script(240)).Columns!;
        var row = new CommandRow(7, 90, 0, 0.5, 0, 0);

        for (var bit = 0; bit < CommandKeys.All.Length; bit++)
        {
            // The loop that builds these columns captures the bit index — a shared capture would
            // wire every flag to the last column.
            var set = (CommandRow)columns[bit + 5].SetValue!(row, true)!;
            Assert.Equal(1u << bit, set.Buttons);
            Assert.True(set.Holds(bit));

            var cleared = (CommandRow)columns[bit + 5].SetValue!(set, false)!;
            Assert.Equal(0u, cleared.Buttons);
        }
    }

    [Fact]
    public void A_stick_edit_replaces_only_its_own_column()
    {
        var columns = Grid(Script(240)).Columns!;
        var row = new CommandRow(7, 90, 10, 0.5, 0.25, 0);

        var angled = (CommandRow)columns[1].SetValue!(row, "359.5")!;
        Assert.Equal(359.5, angled.LeftStickAngle);
        Assert.Equal(row.RightStickAngle, angled.RightStickAngle);

        var deflected = (CommandRow)columns[4].SetValue!(row, "1")!;
        Assert.Equal(1d, deflected.RightStickAmount);
        Assert.Equal(row.LeftStickAmount, deflected.LeftStickAmount);
    }

    [Fact]
    public void A_stick_edit_clamps_the_value_and_keeps_the_old_one_when_it_cannot_be_read()
    {
        var columns = Grid(Script(240)).Columns!;
        var row = new CommandRow(7, 90, 10, 0.5, 0.25, 0);

        Assert.Equal(360d, ((CommandRow)columns[1].SetValue!(row, "3600")!).LeftStickAngle);
        Assert.Equal(0d, ((CommandRow)columns[1].SetValue!(row, "-5")!).LeftStickAngle);
        Assert.Equal(1d, ((CommandRow)columns[3].SetValue!(row, "42")!).LeftStickAmount);

        // Garbage in the box must not become a value the row can't show.
        Assert.Equal(90d, ((CommandRow)columns[1].SetValue!(row, "abc")!).LeftStickAngle);
        Assert.Equal(90d, ((CommandRow)columns[1].SetValue!(row, "")!).LeftStickAngle);
    }

    [Fact]
    public async Task A_committed_edit_lands_in_the_data_source()
    {
        var script = Script(240);
        var source = Source(script);
        var grid = Grid(script);
        var forward = IndexOf("forward");
        var row = CommandRows.For(script)[7];

        // Flip whatever the generated frame holds, so the assertion is about the write landing
        // rather than about what the mock happens to contain.
        var flipped = !row.Holds(forward);
        var edited = (CommandRow)grid.Columns![forward + 5].SetValue!(row, flipped)!;
        await CommandTable.Commit(source, (RowKey)row.Frame, edited);

        // Read back through the source: an edit the grid never writes back would only live in its
        // own optimism overlay, which the next fetch drops.
        var page = await source.GetPageAsync(new DataRequest { PageSize = 20 });
        Assert.Equal(flipped, page.Items.Single(item => item.Frame == 7).Holds(forward));
    }

    [Fact]
    public void The_stick_editor_is_a_text_box_pinned_to_the_row_and_showing_the_cell_format()
    {
        var columns = Grid(Script(240)).Columns!;

        // Not a `NumberBox`: that one hosts its own text box at the stock 32 DIP minimum height, so it
        // overflowed the row (measured) — this is the control that does fit.
        var angle = Assert.IsType<TextBoxElement>(columns[1].Editor!(90d, _ => { }));
        Assert.Equal("90.00", angle.Value.Value);
        Assert.Equal(26d, angle.Modifiers?.Layout?.Height);
        Assert.Equal(0d, angle.Modifiers?.Layout?.MinHeight);

        var amount = Assert.IsType<TextBoxElement>(columns[4].Editor!(0.5, _ => { }));
        Assert.Equal("0.50", amount.Value.Value);
        Assert.Equal("Right stick deflection", amount.Modifiers?.AutomationName);
    }

    [Fact]
    public void The_stick_editor_shows_the_typed_buffer_instead_of_reformatting_it()
    {
        var columns = Grid(Script(240)).Columns!;

        // Each keystroke re-renders the editor; formatting the half-typed buffer would move the caret.
        var half = Assert.IsType<TextBoxElement>(columns[1].Editor!("12.", _ => { }));
        Assert.Equal("12.", half.Value.Value);
    }

    [Fact]
    public void The_flag_editor_is_a_check_box_without_the_stock_minimum_width()
    {
        var columns = Grid(Script(240)).Columns!;
        var forward = IndexOf("forward");

        // A stock WinUI checkbox keeps a 120 DIP minimum width — four square cells' worth.
        var box = Assert.IsType<CheckBoxElement>(columns[forward + 5].Editor!(false, _ => { }));
        Assert.Equal(0d, box.Modifiers?.Layout?.MinWidth);
        Assert.Equal(0d, box.Modifiers?.Layout?.MinHeight);
        Assert.Equal("forward", box.Modifiers?.AutomationName);
    }

    [Fact]
    public void Puts_the_cell_and_header_body_in_its_own_templates()
    {
        var grid = Grid(Script(240));

        Assert.NotNull(grid.CellTemplate);
        Assert.NotNull(grid.HeaderTemplate);
        Assert.NotNull(grid.PlaceholderCellTemplate);
        // Rows butt against each other only while the pitch is a fixed row height, and the height
        // is what has to fit an open editor.
        Assert.Equal(32d, grid.RowHeight);
    }

    [Fact]
    public void Expands_the_mock_script_to_exactly_one_row_per_frame()
    {
        var rows = CommandRows.For(Script(120));

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
        var first = CommandRows.For(Script(240))[0];

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

    /// A workspace entry for the table's own tests: the table reads a path — the mock's seed — and
    /// a frame count — how many rows to generate. What the text says is not part of it, because the
    /// rows are generated rather than parsed (`CommandRows`), so the two need not agree here.
    static ScriptEntry Script(int frames) =>
        new(@"C:\workspace\blade-run.tas", "blade-run", string.Empty, (uint)frames, null);

    static int IndexOf(string key) =>
        Array.FindIndex(CommandKeys.All, entry => entry.Key == key);

    static ListDataSource<CommandRow> Source(ScriptEntry script) =>
        new(CommandRows.For(script), row => (RowKey)row.Frame);

    static DataGridElement<CommandRow> Grid(ScriptEntry script)
    {
        var root = Assert.IsType<FlexElement>(CommandTable.View(Source(script)));
        return Assert.IsType<ComponentElement<DataGridElement<CommandRow>>>(Assert.Single(root.Children)).Props;
    }
}
