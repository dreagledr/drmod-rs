using System;
using System.Collections.Generic;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;    // Element, Theme
using Microsoft.UI.Reactor.Docking; // DockManager, DockSplit, DockNode, Document
using Microsoft.UI.Reactor.Layout;  // GridSize
using Microsoft.UI.Xaml;            // TextWrapping, VerticalAlignment
using Microsoft.UI.Xaml.Controls;   // ScrollBarVisibility, ScrollViewer, ScrollMode
using Microsoft.UI.Xaml.Media;      // FontFamily
using static Microsoft.UI.Reactor.Factories;

sealed record ScriptTextEditorProps(string Text, ScriptTextStatus Status, Action<string> TextChanged);

/// Everything the region paints and everything it can do, in one record.
///
/// Gathered instead of passed one by one because this is the headless layer's entry point:
/// `Component<TProps>.Props` is read-only and set by the host, so a test renders <see
/// cref="ScriptTextEditor.View"/> instead of the component — and the region takes its status, its
/// reference panel and its interactions as values rather than reaching for them.
internal sealed record ScriptTextEditorView(
    string Text,
    ScriptTextStatus Status,
    bool Reference,
    Action ToggleReference,
    Action<string> TextChanged);

/// The script text region: the selected script as `.tas` text, with what that text currently
/// says above it and the format's command reference beside it.
///
/// The text is edited in place and read back on every keystroke — the status line is the
/// converter's own answer, either the document's name, command count and last frame or the line
/// the parser refused. The parse itself is the pane's: it is the same text the Save button and the
/// other regions are about, so it is read once there and handed down here.
///
/// What is typed stays exactly as the control reported it — the text box is the buffer, and
/// keeping its own bytes is what stops the reconciler from writing the text back on every
/// keystroke and moving the caret with it. The format's line separator is put back where a text is
/// read or written (`ScriptDsl.Lines`), never in the buffer.
///
/// Nothing here reaches the command table: the two regions read the same script, and neither
/// writes for the other yet.
///
/// The text box is monospaced and does not wrap on purpose: one line is one frame
/// (`docs/SCRIPT_DSL.md` in the sibling repo), so a wrapped line would read as two.
///
/// The reference panel is a plain list and nothing more: it does not follow the caret, does not
/// complete anything and does not color the text. Coloring a token means walking the document on
/// every keystroke, which costs the editor its responsiveness on a text of a few hundred lines
/// (observed live on the 20 000-frame mock) — the editor stays a plain `TextBox` for that reason,
/// and `Script/ScriptCommands.cs` is read by the panel only.
sealed class ScriptTextEditor : Component<ScriptTextEditorProps>
{
    public override Element Render()
    {
        // Whether the reference is open is the region's own business: it reads no script, and
        // nothing else in the shell has an opinion about it. Where the boundary between the two
        // panes sits is *not* state here — the docking host owns the split ratio.
        var (reference, setReference) = UseState(false);

        return View(new ScriptTextEditorView(
            Props.Text,
            Props.Status,
            reference,
            () => setReference(!reference),
            Props.TextChanged));
    }

    /// The region body. Split out of the component because `Component<TProps>.Props` is
    /// read-only and set by the host, so a headless unit test has no way to render the component
    /// itself — it asserts on this instead.
    ///
    /// A grid with the panes in its star row: the status line takes what it needs and the rest is
    /// the split's.
    internal static Element View(ScriptTextEditorView view) =>
        Grid(
            columns: [GridSize.Star()],
            rows: [GridSize.Auto, GridSize.Star()],
            children:
            [
                Status(view).Grid(row: 0),
                Panes(view).Grid(row: 1),
            ])
        .Padding(12)
        .Flex(grow: 1);

    /// The editor, and the reference while it is open, as the panes of a dock split — so the
    /// boundary between them is the docking host's own splitter, the same one that separates the
    /// regions of this pane, and the split ratio survives a re-render (every keystroke re-renders
    /// this whole region).
    ///
    /// A hand-rolled handle was tried first and taken out again: `OnPan` went through component
    /// state, so every pan event re-rendered the region — 39 reference rows and the editor — and
    /// the boundary felt heavier than the shell's own splitters (the user's report). The host
    /// moves its splitter itself and costs a layout pass.
    ///
    /// Which panes exist is the region's state; the host merges a changed key set into its shape,
    /// so closing the reference drops the pane and opening it again puts it back at
    /// `ReferenceWidth`.
    static Element Panes(ScriptTextEditorView view)
    {
        // A pane with no width takes what the others do not, wherever it sits in the split, so the
        // editor is the flexible one and the reference keeps its own width on the right.
        var panes = new List<DockNode> { Pane(EditorKey, "Script text", null, Text(view)) };

        if (view.Reference)
        {
            panes.Add(Pane(ReferenceKey, "Commands", ReferenceWidth, Reference()));
        }

        return FlexColumn(
            new DockManager { Layout = new DockSplit(Orientation.Horizontal, [.. panes]) }
                .Flex(grow: 1, basis: 0)
        ).Flex(grow: 1);
    }

    /// One pane of the split: a bare `Document` rather than a tab group, because a group always
    /// carries a tab strip and the only thing wanted around a pane here is its splitter.
    ///
    /// Nothing here closes, floats or moves — the region's panes are fixed and only the splitter
    /// between them moves, which keeps this out of the docking states the shell does not survive
    /// (`Editor.cs`, `README.md`).
    static DockNode Pane(string key, string title, double? width, Element body) =>
        new Document
        {
            Title = title,
            Key = key,
            Content = Fill(body),
            Width = width,
            CanClose = false,
            CanFloat = false,
            CanMove = false,
            CanDockAsToolWindow = false,
        };

    /// A pane body is content-sized unless it is given room to grow, and a text box stays
    /// content-sized even in a flex slot that grows (measured live: 92.67 DIP of a 116.75 slot) —
    /// a star row is what stretches either of them into the pane.
    static Element Fill(Element body) =>
        Grid(columns: [GridSize.Star()], rows: [GridSize.Star()], children: [body.Grid(row: 0)])
            .Flex(grow: 1);

    /// The line above the panes: what the text says, or why it says nothing, and the button that
    /// opens the command reference. An error keeps the parser's own wording — it already names
    /// the line or the command — and is painted in the theme's error color, so a text the mod
    /// would answer `400` on never looks like one it accepts.
    internal static Element Status(ScriptTextEditorView view) =>
        Grid(
            columns: [GridSize.Star(), GridSize.Auto],
            rows: [GridSize.Auto],
            children:
            [
                (view.Status.Document is { } document
                    ? Caption(Summary(document))
                    : Caption(view.Status.Error!).Foreground(Theme.SystemCritical)).Grid(row: 0, column: 0),
                Button(
                    view.Reference ? "Hide commands" : "Commands",
                    view.ToggleReference)
                    // The analyzer reads a button's own label as its name when the content is an
                    // element tree, so a labelled button still counts as unnamed here
                    // (REACTOR_A11Y_001).
                    .AutomationName("Show the commands the script text understands")
                    .Padding(8, 2)
                    .Grid(row: 0, column: 1),
            ]);

    internal static string Summary(ScriptDocument document) =>
        $"{document.Name} · {document.Commands.Count} commands · last frame {ScriptTextStatus.LastFrame(document)}";

    /// The editor itself, pinned to the top of whatever height its pane gives it — the default
    /// content alignment centres the lines when the text is shorter than the box.
    ///
    /// The automation name is what keeps a bare text box from being an unnamed form field
    /// (REACTOR_A11Y_003): the caption above it is a status message, not a label.
    internal static Element Text(ScriptTextEditorView view) =>
        TextBox(view.Text, view.TextChanged)
            .AcceptsReturn()
            .TextWrapping(TextWrapping.NoWrap)
            .IsSpellCheckEnabled(false)
            .FontSize(12)
            .VerticalContentAlignment(VerticalAlignment.Top)
            .AutomationName("Script text")
            .Set(box =>
            {
                box.FontFamily = new FontFamily(Monospace);
                // A `TextBox` keeps both of its scrollbars hidden until told otherwise, which is
                // why a text that scrolls (the wheel works) still looks like it has nowhere to
                // go. The attached properties are the box's own way in — the value lands on the
                // `ScrollViewer` inside its template (measured). The type is spelled out because
                // the bare name is the `ScrollViewer` *factory* in this file.
                Microsoft.UI.Xaml.Controls.ScrollViewer.SetVerticalScrollBarVisibility(
                    box, ScrollBarVisibility.Auto);
                Microsoft.UI.Xaml.Controls.ScrollViewer.SetHorizontalScrollBarVisibility(
                    box, ScrollBarVisibility.Auto);
            });

    /// The command reference beside the editor: one row per token — how it is spelled, then what
    /// it means — in the order the docs table has them, with the rules line's attributes in a
    /// section of their own.
    ///
    /// A row is not clickable and nothing here follows the caret: this is a reference, so a row is
    /// text. Every text that can outgrow the panel sits in a `Grid` star column — a `StackPanel`
    /// hands its children unbounded width, which is what clipped the help lines while the rows
    /// were `HStack`s (measured live).
    internal static Element Reference() =>
        Border(FlexColumn(
                Header(),
                ScrollViewer(VStack(2, [.. Entries()]))
                    .VerticalScrollMode(ScrollMode.Auto)
                    .HorizontalScrollMode(ScrollMode.Disabled)))
            // Opaque surfaces, not the translucent layer fills: this panel sits beside the text
            // and over the pane, and a translucent card would let the pane show through it. The
            // same family the command table paints its cells with (`CommandTable.CellSurface`).
            .Background(Theme.SolidBackground)
            .WithBorder(Theme.CardStroke)
            .CornerRadius(8)
            .Padding(8);

    /// What the list is, and the one thing every token shares — the duration behind a `:`.
    static Element Header() =>
        Grid(
            columns: [GridSize.Auto, GridSize.Star()],
            rows: [GridSize.Auto],
            children:
            [
                TextBlock("Commands").Grid(row: 0, column: 0),
                Caption(":frames holds a token that long — 1 by default")
                    .Foreground(Theme.SecondaryText)
                    .TextWrapping(TextWrapping.Wrap)
                    .Margin(8, 0, 0, 0)
                    .Grid(row: 0, column: 1),
            ]);

    /// The rows of the reference: the frame line's commands, then the rules line's own line.
    static IEnumerable<Element> Entries()
    {
        foreach (var command in ScriptCommands.Frame)
        {
            yield return Entry(command);
        }

        yield return Caption("Rules line").Margin(0, 8, 0, 0);
        foreach (var rule in ScriptCommands.Rules)
        {
            yield return Entry(rule);
        }
    }

    /// One line of the reference: how a command is spelled (in the editor's own mono face, so the
    /// tokens read as code), then what it means. The spelling column is an `Auto` column with a
    /// floor under it, so the rows line up until a token outgrows it.
    static Element Entry(ScriptCommandHelp command) =>
        Grid(
            columns: [GridSize.Auto, GridSize.Star()],
            rows: [GridSize.Auto],
            children:
            [
                TextBlock(command.Spelling)
                    .MinWidth(SpellingWidth)
                    .Set(text => text.FontFamily = new FontFamily(Monospace))
                    .Grid(row: 0, column: 0),
                TextBlock(command.Help)
                    .TextWrapping(TextWrapping.Wrap)
                    .Margin(10, 0, 0, 0)
                    .Grid(row: 0, column: 1),
            ]);

    /// The keys the host matches the panes by: stable and equatable, so the reference's pane keeps
    /// its place (and the split ratio) across the re-renders a keystroke causes.
    const string EditorKey = "script:text:editor";
    const string ReferenceKey = "script:text:commands";

    /// Where the boundary starts when the reference is opened — the splitter moves it from here,
    /// and the host remembers where it was left while both panes stay.
    const double ReferenceWidth = 360;

    /// The floor under the spelling column, so the spellings line up.
    const double SpellingWidth = 96;

    /// Set through `.Set` rather than through the `.FontFamily(…)` modifier on purpose: that
    /// modifier resolves the name into a `FontFamily` WinRT object while the element is *built*,
    /// which throws `COMException` in the headless unit layer (measured — the same trap as
    /// `.SemiBold()`). A setter runs against the mounted control instead, which only the app has.
    ///
    /// The first font of the list a machine is likely to have: Cascadia ships with Windows
    /// Terminal and the newer SDKs, Consolas with Windows itself, and the last entry is the
    /// generic fallback WinUI understands.
    const string Monospace = "Cascadia Mono, Consolas, monospace";
}
