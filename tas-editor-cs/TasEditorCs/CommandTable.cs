using System;
using System.Collections.Generic;
using System.Globalization;
using System.Threading.Tasks;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Controls;        // CellContext, HeaderContext, ColumnBuilder, SelectionMode
using Microsoft.UI.Reactor.Core;            // Element, ThemeRef, Theme
using Microsoft.UI.Reactor.Data;            // FieldDescriptor, IDataSource, IMutableDataSource, RowKey
using Microsoft.UI.Reactor.Data.Providers;  // ListDataSource
using Microsoft.UI.Xaml;                    // HorizontalAlignment, VerticalAlignment
using static Microsoft.UI.Reactor.Advanced.Factories; // DataGrid, Column
using static Microsoft.UI.Reactor.Factories;          // Border, CheckBox, TextBox, TextBlock, Empty

sealed record CommandTableProps(ScriptEntry Script);

/// The command table: one row per script frame, one column per stick value and per boolean
/// input — the whole script as a matrix, so a timing reads off the shape of the lit cells
/// instead of a JSON command list.
///
/// Rows are generated mock frames until the on-disk workspace is wired up. The grid virtualizes
/// them, so a 20 000-frame script only ever renders the visible ten.
///
/// Editing is the grid's own inline editing: a tap on a cell opens that column's editor, and the
/// commit goes through `onRowChanged`, which writes the new row back into the source.
sealed class CommandTable : Component<CommandTableProps>
{
    public override Element Render()
    {
        // Memoized on purpose: DataGrid keys the grid's mount off the source's identity, so a
        // source rebuilt on every render would remount the grid and drop the scroll position.
        var source = UseMemo(
            () => new ListDataSource<CommandRow>(CommandRows.For(Props.Script), row => (RowKey)row.Frame),
            Props.Script);

        return View(source);
    }

    /// The pane body. Split out of the component because `Component<TProps>.Props` is read-only
    /// and set by the host, so a headless unit test has no way to render the component itself —
    /// it asserts on this instead.
    internal static Element View(IDataSource<CommandRow> source) =>
        FlexColumn(
            DataGrid(
                source: source,
                columns: Columns,
                rowHeight: RowHeight,
                cellTemplate: Cell,
                headerTemplate: Header,
                placeholderCellTemplate: PlaceholderCell,
                editable: true,
                onRowChanged: (key, row) => Commit(source, key, row)
            ).Flex(grow: 1, basis: 0)
        ).Flex(grow: 1);

    /// An edit only exists in the grid's own optimism overlay until it is written back here — the
    /// overlay is dropped on the next fetch, so without this the value would revert. The grid may
    /// call this off the UI thread, which `ListDataSource.UpdateAsync` takes a lock for.
    internal static Task Commit(IDataSource<CommandRow> source, RowKey key, CommandRow row) =>
        source is IMutableDataSource<CommandRow> mutable
            ? mutable.UpdateAsync(key, row)
            : Task.CompletedTask;

    // The cells butt against each other: no padding, no border, no gap, and the row pitch is
    // exactly the row height. What separates a cell from its neighbour is the surface it is
    // painted with, so neighbouring cells never share one — with the held inputs lit, the whole
    // script reads as a checkerboard of lit and unlit cells.
    //
    // The height is set by what has to fit while a cell is open for editing: at 18 — the dense pitch
    // that reads best as a matrix — the stick editor, a stock `TextBox` with a caret and a selection,
    // has no usable room. The height is a knob: the shrunk editors above fit at 24.
    const double RowHeight = 32;
    const double HeaderHeight = 20;
    // Flag cells are square: the row height doubles as the width, so one of the two cannot drift
    // away from a matrix that reads as a matrix.
    const double FlagWidth = RowHeight;
    const double AngleWidth = 46;
    const double AmountWidth = 40;
    const double FrameWidth = 44;

    /// Built once: the grid keys its column-layout cache on this list's reference identity, so
    /// a list rebuilt per render would rebuild every row's column definitions with it.
    internal static readonly IReadOnlyList<FieldDescriptor> Columns = BuildColumns();

    static IReadOnlyList<FieldDescriptor> BuildColumns()
    {
        var columns = new List<FieldDescriptor>(CommandKeys.All.Length + 5)
        {
            // The frame number is the row's identity: not editable, and pinned so it stays put
            // when the table is scrolled sideways.
            Column<CommandRow>("Frame", row => row.Frame, displayName: "#",
                width: FrameWidth, pin: PinPosition.Left).NotSortable(),

            // Each stick is two columns: where it points, and how far it is pushed.
            Editable(
                Column<CommandRow>("left_stick_angle", row => row.LeftStickAngle,
                    displayName: "LD", width: AngleWidth).NotSortable(),
                (row, value) => row with { LeftStickAngle = ReadNumber(value, row.LeftStickAngle, 360) },
                NumberEditor("Left stick direction in degrees")),
            Editable(
                Column<CommandRow>("right_stick_angle", row => row.RightStickAngle,
                    displayName: "RD", width: AngleWidth).NotSortable(),
                (row, value) => row with { RightStickAngle = ReadNumber(value, row.RightStickAngle, 360) },
                NumberEditor("Right stick direction in degrees")),
            Editable(
                Column<CommandRow>("left_stick_amount", row => row.LeftStickAmount,
                    displayName: "LM", width: AmountWidth).NotSortable(),
                (row, value) => row with { LeftStickAmount = ReadNumber(value, row.LeftStickAmount, 1) },
                NumberEditor("Left stick deflection")),
            Editable(
                Column<CommandRow>("right_stick_amount", row => row.RightStickAmount,
                    displayName: "RM", width: AmountWidth).NotSortable(),
                (row, value) => row with { RightStickAmount = ReadNumber(value, row.RightStickAmount, 1) },
                NumberEditor("Right stick deflection")),
        };

        // Named after the script's `input` keys so the columns line up with the format the
        // script is written in; the header is the 1-2 letter label that fits the column width.
        for (var bit = 0; bit < CommandKeys.All.Length; bit++)
        {
            var captured = bit;
            var (key, label) = CommandKeys.All[bit];
            columns.Add(Editable(
                Column<CommandRow>(key, row => row.Holds(captured),
                    displayName: label, width: FlagWidth).NotSortable(),
                (row, value) => row.With(captured, (bool)value!),
                FlagEditor(key)));
        }

        return columns;
    }

    /// Wires a column for inline editing. The setter has to be supplied by hand: the column
    /// builder derives it from a property named after the column, and these columns are named
    /// after the script's input keys — the frame has no such property, and the flags live in one
    /// bit field.
    static FieldDescriptor Editable(
        ColumnBuilder<CommandRow> column,
        Func<CommandRow, object?, CommandRow> setValue,
        Func<object, Action<object>, Element> editor) =>
        column.Build() with
        {
            SetValue = (owner, value) => setValue((CommandRow)owner, value),
            IsReadOnly = false,
            Editor = editor,
        };

    // The flag editor is shrunk to the cell: a stock WinUI checkbox keeps a 120 DIP minimum width,
    // which would paint over four of the square flag columns. Not `Editors.CheckBox()` — that wraps
    // this same control without the shrinking.
    //
    // It carries an automation name because a bare checkbox has no caption of its own and the
    // column header next to it is a 1-2 letter label (REACTOR_A11Y_003).
    static Func<object, Action<object>, Element> FlagEditor(string input) => (value, onChange) =>
        CheckBox((bool)(value ?? false), held => onChange(held))
            .MinWidth(0)
            .MinHeight(0)
            .HAlign(HorizontalAlignment.Center)
            .AutomationName(input);

    // Stick values edit in a plain `TextBox`, not a `NumberBox`, and it is a fit problem rather than
    // a taste one: a number box hosts its own text box, whose stock 32 DIP minimum height the outer
    // `MinHeight(0)` cannot reach — measured, the control overflowed the 32 DIP row — and it brings
    // a clear button that eats a 46 DIP wide cell. The explicit height pins the fit.
    //
    // The buffer stays the raw text (see `EditorText`), so `SetValue` is where the number is read:
    // parsing per keystroke would reformat the text under the caret.
    static Func<object, Action<object>, Element> NumberEditor(string name) => (value, onChange) =>
        TextBox(EditorText(value), text => onChange(text))
            .MinHeight(0)
            .Height(RowHeight - 6)
            .FontSize(10)
            .AutomationName(name);

    /// What the editor shows: the row's value formatted like the cell, or — once the user has typed
    /// — the buffer itself, so a half-typed number survives the re-render each keystroke causes.
    static string EditorText(object? value) => value switch
    {
        string typed => typed,
        double number => number.ToString("F2", CultureInfo.InvariantCulture),
        _ => string.Empty,
    };

    /// Reads a committed edit back. Unparsable input and values outside the column's range leave the
    /// field alone rather than throwing or writing nonsense into the row.
    static double ReadNumber(object? value, double fallback, double max) =>
        value is string text
        && double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed)
        && double.IsFinite(parsed)
            ? Math.Clamp(parsed, 0, max)
            : fallback;

    static readonly Dictionary<string, int> ColumnOrder = BuildColumnOrder();

    static Dictionary<string, int> BuildColumnOrder()
    {
        var order = new Dictionary<string, int>(StringComparer.Ordinal);
        for (var index = 0; index < Columns.Count; index++) order[Columns[index].Name] = index;
        return order;
    }

    /// One cell: a block that fills its column and its row whole. The grid centres whatever the
    /// template returns and leaves its other modifiers alone, so the block carries its height
    /// explicitly and stretches to the column width on its own.
    internal static Element Cell(CellContext<CommandRow> cell)
    {
        var surface = CellSurface(cell.Row.Frame, cell.Column.Name);

        return cell.Value is bool held
            ? Border(Empty()).Background(held ? HeldSurface : surface).Height(RowHeight)
            : Border(Value(cell.Value)
                    .FontSize(10)
                    .HAlign(HorizontalAlignment.Center)
                    .VAlign(VerticalAlignment.Center))
                .Background(surface)
                .Height(RowHeight);
    }

    /// A header cell is a label and nothing else — at 18 px per column the 1-2 letter header is
    /// all that fits, so the meaning has to come from the table's own legend.
    internal static Element Header(HeaderContext header) =>
        Border(TextBlock(header.Column.DisplayName ?? header.Column.Name)
                .FontSize(10)
                .HAlign(HorizontalAlignment.Center)
                .VAlign(VerticalAlignment.Center))
            .Background(HeaderSurface)
            .Height(HeaderHeight);

    static Element Value(object? value) => value switch
    {
        double number => TextBlock(number.ToString("F2")),
        int number => TextBlock(number.ToString()),
        _ => TextBlock(""),
    };

    /// A row whose block the cache has not fetched yet. Same block geometry as a real cell, but
    /// flat: this callback gets no row index, so it cannot join the checkerboard, and the grid's
    /// own shimmer is a translucent fill that all but disappears on a dark table.
    internal static Element PlaceholderCell(FieldDescriptor column, double width) =>
        Border(Empty()).Background(SurfaceA).Height(RowHeight);

    // Opaque surfaces from the solid background family, not the translucent layer fills:
    // measured in the light theme, LayerFill/CardBackground resolve to #80FFFFFF and
    // #B3FFFFFF, and two translucent whites over a light pane land ~2 RGB steps apart —
    // an invisible checkerboard. These are #F3F3F3 / #DADADA in the light theme and
    // #202020 / #0A0A0A in the dark one, resolved through the same ThemeResource path, so
    // the table still follows the shell's dark / light toggle.
    static readonly ThemeRef SurfaceA = Theme.Ref("SolidBackgroundFillColorBaseBrush");
    static readonly ThemeRef SurfaceB = Theme.Ref("SolidBackgroundFillColorBaseAltBrush");
    static readonly ThemeRef HeaderSurface = Theme.Ref("SolidBackgroundFillColorQuinaryBrush");
    static readonly ThemeRef HeldSurface = Theme.Accent;

    /// The surface of one cell: the parity of its row and column, so no cell matches the one
    /// above or beside it.
    internal static ThemeRef CellSurface(int frame, string column)
    {
        var index = ColumnOrder.TryGetValue(column, out var found) ? found : 0;
        return ((frame ^ index) & 1) == 0 ? SurfaceA : SurfaceB;
    }
}
