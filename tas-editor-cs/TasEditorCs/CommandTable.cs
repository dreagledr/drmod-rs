using System;
using System.Collections.Generic;
using System.Globalization;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Controls;        // CellContext, HeaderContext, FieldDescriptor, SelectionMode
using Microsoft.UI.Reactor.Core;            // Element, ThemeRef, Theme
using Microsoft.UI.Reactor.Data;            // IDataSource, RowKey
using Microsoft.UI.Reactor.Data.Providers;  // ListDataSource
using Microsoft.UI.Xaml;                    // HorizontalAlignment, VerticalAlignment
using static Microsoft.UI.Reactor.Advanced.Factories; // DataGrid, Column
using static Microsoft.UI.Reactor.Factories;          // Border, TextBlock, Empty

sealed record CommandTableProps(ScriptEntry Script, string Text, ScriptTextStatus Status);

/// The command table: one row per frame, one column per DSL token — the whole script as a
/// matrix, so a timing reads off the shape of the lit cells instead of a JSON command list.
///
/// Read-only. The table visualizes the `.tas` text in the region below it: a row reads as the
/// line the format would write for that frame, and editing is done in the text, in one place,
/// rather than in two representations that would have to be kept in step.
///
/// The frames come from the text on screen, read token by token rather than from the parsed
/// document: a document has already resolved `ls:<angle>` into axis numbers, so the two spellings
/// are indistinguishable by then, and the table is meant to show what the script says.
sealed class CommandTable : Component<CommandTableProps>
{
    public override Element Render()
    {
        // Memoized on the text: DataGrid keys the grid's mount off the source's identity, so a
        // source rebuilt on every render would remount the grid and drop the scroll position.
        var source = UseMemo(
            () => new ListDataSource<ScriptFrame>(Frames(Props.Text), row => (RowKey)(int)row.Frame),
            Props.Text);

        return View(source);
    }

    /// The frames the table shows: the text's own, or nothing when it does not parse. The text
    /// region carries the parser's message, so the table stays silent rather than saying the same
    /// thing a third way.
    internal static IReadOnlyList<ScriptFrame> Frames(string text) =>
        ScriptTextStatus.Of(text).Document is not null ? ScriptFrameProjection.Project(text) : [];

    /// The pane body. Split out of the component because `Component<TProps>.Props` is read-only
    /// and set by the host, so a headless unit test has no way to render the component itself —
    /// it asserts on this instead.
    internal static Element View(IDataSource<ScriptFrame> source) =>
        FlexColumn(
            DataGrid(
                source: source,
                columns: Columns,
                rowHeight: RowHeight,
                cellTemplate: Cell,
                headerTemplate: Header,
                placeholderCellTemplate: PlaceholderCell,
                editable: false,
                selectionMode: SelectionMode.None
            ).Flex(grow: 1, basis: 0)
        ).Flex(grow: 1);

    // The cells butt against each other: no padding, no border, no gap, and the row pitch is
    // exactly the row height. What separates a cell from its neighbour is the surface it is
    // painted with, so neighbouring cells never share one — with the held inputs lit, the whole
    // script reads as a checkerboard of lit and unlit cells.
    const double RowHeight = 24;
    const double HeaderHeight = 20;
    // Flag cells are square: the row height doubles as the width, so one of the two cannot drift
    // away from a matrix that reads as a matrix.
    const double FlagWidth = RowHeight;
    const double FrameWidth = 44;
    // An angle cell holds up to "359.999" — six characters of the mono face at 10 px.
    const double AngleWidth = 44;
    // An axis cell holds up to "-1000" in the same face.
    const double AxisWidth = 40;

    /// Built once: the grid keys its column-layout cache on this list's reference identity, so
    /// a list rebuilt per render would rebuild every row's column definitions with it.
    internal static readonly IReadOnlyList<FieldDescriptor> Columns = BuildColumns();

    static IReadOnlyList<FieldDescriptor> BuildColumns()
    {
        var columns = new List<FieldDescriptor>(FlagKeys.All.Length + 7)
        {
            // The frame number is the row's identity: not editable, and pinned so it stays put
            // when the table is scrolled sideways. Its header is not a DSL token — the format
            // spells a frame as the bare number at the start of the line, which is what "#" says.
            Column<ScriptFrame>("Frame", row => row.Frame, displayName: "#", width: FrameWidth, pin: PinPosition.Left).NotSortable(),
        };

        // Each stick is three columns, named and headed by the three tokens the DSL gives it: the
        // angle `ls`/`rs`, and the two axes `lsx`/`lsy` and `rsx`/`rsy`. A line writes one form or
        // the other, so exactly one of the three carries a value and the rest are blank — the table
        // never invents the form the text did not write.
        Stick(columns, "ls");
        Stick(columns, "rs");

        // The flag columns, in the DSL's token order, named and headed by the token itself.
        for (var bit = 0; bit < FlagKeys.All.Length; bit++)
        {
            var captured = bit;
            columns.Add(
                Column<ScriptFrame>(FlagKeys.All[bit].Token, row => row.Holds(captured),
                    displayName: FlagKeys.All[bit].Token, width: FlagWidth).NotSortable());
        }

        return columns;
    }

    /// A stick as its three columns, named and headed by the tokens the DSL spells it with: the
    /// angle token (`ls`, `rs`) and its two axes (`lsx`/`lsy`, `rsx`/`rsy`). The value column
    /// selector supplies the number for the cell template, which formats it.
    static void Stick(List<FieldDescriptor> columns, string token)
    {
        columns.Add(Column<ScriptFrame>(token, row => Stick(row, token).Angle,
            displayName: token, width: AngleWidth).NotSortable());
        columns.Add(Column<ScriptFrame>(token + "x", row => Stick(row, token).X,
            displayName: token + "x", width: AxisWidth).NotSortable());
        columns.Add(Column<ScriptFrame>(token + "y", row => Stick(row, token).Y,
            displayName: token + "y", width: AxisWidth).NotSortable());
    }

    static StickValue Stick(ScriptFrame row, string token) =>
        token == "ls" ? row.Left : row.Right;

    /// One cell: a block that fills its column and its row whole. The grid centres whatever the
    /// template returns and leaves its other modifiers alone, so the block carries its height
    /// explicitly and stretches to the column width on its own.
    ///
    /// A value the line left out is a blank block rather than a zero: the table shows the tokens
    /// the text carries, and an axis the text does not mention has no value to show.
    internal static Element Cell(CellContext<ScriptFrame> cell)
    {
        var surface = CellSurface((int)cell.Row.Frame, cell.Column.Name);
        var text = Text(cell);

        return cell.Value is bool held
            ? Border(Empty()).Background(held ? HeldSurface : surface).Height(RowHeight)
            : Border(TextBlock(text)
                    .FontSize(10)
                    .HAlign(HorizontalAlignment.Center)
                    .VAlign(VerticalAlignment.Center))
                .Background(surface)
                .Height(RowHeight);
    }

    /// What one cell says — every non-flag cell is a stick value the line wrote, keyed by the DSL
    /// token it came from, and a value the line did not write is blank rather than zero.
    static string Text(CellContext<ScriptFrame> cell)
    {
        var row = cell.Row;
        return cell.Column.Name switch
        {
            "Frame" => row.Frame.ToString(CultureInfo.InvariantCulture),
            "ls" => Value(row.Left.Angle),
            "lsx" => Value(row.Left.X),
            "lsy" => Value(row.Left.Y),
            "rs" => Value(row.Right.Angle),
            "rsx" => Value(row.Right.X),
            "rsy" => Value(row.Right.Y),
            _ => "",
        };
    }

    /// A stick value the way the text wrote it, or blank when the line left it out.
    static string Value(double? value) =>
        value is { } number ? number.ToString(CultureInfo.InvariantCulture) : "";

    /// A header cell is a label and nothing else — at 24 px per column the token is all that
    /// fits, and the token is what the text region's reference spells out.
    internal static Element Header(HeaderContext header) =>
        Border(TextBlock(header.Column.DisplayName ?? header.Column.Name)
                .FontSize(10)
                .HAlign(HorizontalAlignment.Center)
                .VAlign(VerticalAlignment.Center))
            .Background(HeaderSurface)
            .Height(HeaderHeight);

    /// A row whose block the cache has not fetched yet. Same block geometry as a real cell, but
    /// flat: this callback gets no row index, so it cannot join the checkerboard, and the grid's
    /// own shimmer is a translucent fill that all but disappears on a dark table.
    internal static Element PlaceholderCell(FieldDescriptor column, double width) =>
        Border(Empty()).Background(SurfaceA).Height(RowHeight);

    /// Numbers are written the way the DSL writes them: shortest round-trip, invariant culture,
    /// so a value in the table is the value the text carries.
    static string Number(double value) => value.ToString(CultureInfo.InvariantCulture);

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

    static readonly Dictionary<string, int> ColumnOrder = BuildColumnOrder();

    static Dictionary<string, int> BuildColumnOrder()
    {
        var order = new Dictionary<string, int>(StringComparer.Ordinal);
        for (var index = 0; index < Columns.Count; index++) order[Columns[index].Name] = index;
        return order;
    }
}
