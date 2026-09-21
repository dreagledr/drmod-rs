using System;
using System.Collections.Generic;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Controls;        // CellContext, HeaderContext, SelectionMode
using Microsoft.UI.Reactor.Core;            // Element, ThemeRef, Theme
using Microsoft.UI.Reactor.Data;            // FieldDescriptor, IDataSource, RowKey, PinPosition
using Microsoft.UI.Reactor.Data.Providers;  // ListDataSource
using Microsoft.UI.Xaml;                    // HorizontalAlignment, VerticalAlignment
using static Microsoft.UI.Reactor.Advanced.Factories; // DataGrid, Column
using static Microsoft.UI.Reactor.Factories;          // Border, TextBlock, Empty, FlexColumn

sealed record CommandTableProps(ScriptEntry Script);

/// The command table: one row per script frame, one column per stick value and per boolean
/// input — the whole script as a matrix, so a timing reads off the shape of the lit cells
/// instead of a JSON command list.
///
/// Read-only for now: `editable` is off, so no editor is built and no commit path exists. The
/// grid virtualizes rows, so the 20 000-frame scripts the workspace is expected to hold only
/// ever render the visible ~20 of them.
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
                placeholderCellTemplate: PlaceholderCell
            ).Flex(grow: 1, basis: 0)
        ).Flex(grow: 1);

    // The cells butt against each other: no padding, no border, no gap, and the row pitch is
    // exactly the row height. What separates a cell from its neighbour is the surface it is
    // painted with, so neighbouring cells never share one — with the held inputs lit, the whole
    // script reads as a checkerboard of lit and unlit cells.
    const double RowHeight = 18;
    const double HeaderHeight = 20;
    const double AngleWidth = 46;
    const double AmountWidth = 40;
    const double FlagWidth = 18;

    /// Built once: the grid keys its column-layout cache on this list's reference identity, so
    /// a list rebuilt per render would rebuild every row's column definitions with it.
    internal static readonly IReadOnlyList<FieldDescriptor> Columns = BuildColumns();

    static IReadOnlyList<FieldDescriptor> BuildColumns()
    {
        var columns = new List<FieldDescriptor>(CommandKeys.All.Length + 5)
        {
            // The frame number is pinned so it stays put when the table is scrolled sideways.
            Column<CommandRow>("Frame", row => row.Frame, displayName: "#",
                width: 44, pin: PinPosition.Left).NotSortable(),
            // Each stick is two columns: where it points, and how far it is pushed.
            Column<CommandRow>("left_stick_angle", row => row.LeftStickAngle,
                displayName: "LD", width: AngleWidth).NotSortable(),
            Column<CommandRow>("right_stick_angle", row => row.RightStickAngle,
                displayName: "RD", width: AngleWidth).NotSortable(),
            Column<CommandRow>("left_stick_amount", row => row.LeftStickAmount,
                displayName: "LM", width: AmountWidth).NotSortable(),
            Column<CommandRow>("right_stick_amount", row => row.RightStickAmount,
                displayName: "RM", width: AmountWidth).NotSortable(),
        };

        // Named after the script's `input` keys so the columns line up with the format the
        // script is written in; the header is the 1-2 letter label that fits the column width.
        for (var bit = 0; bit < CommandKeys.All.Length; bit++)
        {
            var captured = bit;
            var (key, label) = CommandKeys.All[bit];
            columns.Add(Column<CommandRow>(key, row => row.Holds(captured),
                displayName: label, width: FlagWidth).NotSortable());
        }

        return columns;
    }

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
