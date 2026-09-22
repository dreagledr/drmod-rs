using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Docking;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace TasEditorCs.Tests;

/// The script text region. Structural like the other pane tests: the text box is asserted as the
/// element it is and is never activated as a WinUI control.
///
/// The region's body is a dock split of two panes — the editor, and the command reference when it
/// is open — so its boundary is the docking host's own splitter. What a test can pin here is the
/// panes and their keys (the host owns the ratio); where the splitter ends up is the host's.
public class ScriptTextEditorTests
{
    const string Draft = "! name=blade-run\n0 ls:0:6\n6 a:2\n";

    static readonly ScriptTextStatus Read = ScriptTextStatus.Of(Draft);
    static readonly ScriptTextStatus Refused = ScriptTextStatus.Of("0 zz\n");

    [Fact]
    public void Shows_the_text_it_was_given()
    {
        Assert.Equal(Draft, Box(Read).Value.Value);
    }

    [Fact]
    public void Hands_back_what_is_typed()
    {
        var typed = new List<string>();
        var box = Box(Read, typed: typed.Add);

        Assert.NotNull(box.OnChanged);
        box.OnChanged("0 a\n");

        Assert.Equal(new[] { "0 a\n" }, typed);
    }

    [Fact]
    public void Keeps_one_line_per_frame()
    {
        // A wrapped line would read as two frames, so the editor scrolls sideways instead.
        var box = Box(Read);

        Assert.True(box.AcceptsReturn);
        Assert.Equal(TextWrapping.NoWrap, box.TextWrapping);
        Assert.False(box.IsSpellCheckEnabled);
    }

    [Fact]
    public void Sums_up_a_text_it_read()
    {
        Assert.Equal("blade-run · 2 commands · last frame 8", Status(Read));
        Assert.Equal(ScriptTextEditor.Summary(Read.Document!), Status(Read));
    }

    [Fact]
    public void Shows_a_refused_text_with_the_parsers_own_message()
    {
        Assert.StartsWith("line 1: unknown token 'zz'", Status(Refused));
    }

    [Theory]
    [InlineData("a\rb", "a\nb")]
    [InlineData("a\r\nb", "a\nb")]
    [InlineData("a\nb", "a\nb")]
    public void Puts_the_formats_line_separator_back(string asReported, string expected)
    {
        Assert.Equal(expected, ScriptTextEditor.Lines(asReported));
    }

    [Fact]
    public void Reads_a_draft_the_way_the_text_box_reports_it()
    {
        // Measured live: a WinUI `TextBox` hands its text back with a lone `\r`, so a draft stored
        // as it comes would read as one long line — the caption would paint an error the user
        // cannot clear by editing the text.
        var reported = ScriptTextStatus.Of(ScriptTextEditor.Lines(Draft.Replace('\n', '\r')));

        Assert.True(reported.IsOk);
        Assert.Equal(8u, ScriptTextStatus.LastFrame(reported.Document!));
    }

    [Fact]
    public void Gives_the_panes_the_row_that_is_left_over()
    {
        // The status line takes what it needs, the split takes the rest. A pane body is
        // content-sized unless something gives it a size — measured live on the editor: 92.67 DIP
        // of a 116.75 DIP flex slot — and both panes carry a star row for it.
        var grid = Body(Read);

        Assert.Equal(
            new[] { GridUnitType.Auto, GridUnitType.Star },
            grid.Definition.Rows.Select(row => row.Type));
        Assert.Equal(new[] { GridUnitType.Star }, grid.Definition.Columns.Select(column => column.Type));
        Assert.IsType<GridElement>(grid.Children[0]);
        Assert.IsType<FlexElement>(grid.Children[1]);
    }

    [Fact]
    public void Offers_the_command_reference_whether_or_not_it_is_open()
    {
        var closed = CommandsButton(Read, reference: false);
        var open = CommandsButton(Read, reference: true);

        Assert.Equal("Commands", closed.Label);
        Assert.Equal("Hide commands", open.Label);
        Assert.NotNull(closed.OnClick);
    }

    [Fact]
    public void The_editor_is_a_pane_of_the_split()
    {
        // The split is horizontal, so the splitter between the panes is a vertical bar — the same
        // one the docking host draws between the regions above.
        var split = Split(RegionFor(Read));

        Assert.Equal(Orientation.Horizontal, split.Orientation);
        Assert.Equal(new object[] { "script:text:editor" }, Documents(split).Select(pane => pane.Key));
        Assert.Equal(new double?[] { null }, Documents(split).Select(pane => pane.Width));
    }

    [Fact]
    public void The_reference_is_a_pane_beside_the_editor_only_when_it_is_open()
    {
        var open = RegionFor(Read, reference: true);
        var split = Split(open);

        Assert.Equal(
            new object[] { "script:text:editor", "script:text:commands" },
            Documents(split).Select(pane => pane.Key));
        // The editor takes what the reference does not, whichever side it sits on.
        Assert.Equal(new double?[] { null, 360 }, Documents(split).Select(pane => pane.Width));
    }

    [Fact]
    public void A_pane_keeps_its_content_filling_it()
    {
        // A pane body would otherwise collapse to its desired height at the top of the pane — the
        // editor and the reference are both star rows inside their own pane.
        foreach (var pane in Documents(Split(RegionFor(Read, reference: true))))
        {
            var fill = Assert.IsType<GridElement>(pane.Content);

            Assert.Equal(new[] { GridUnitType.Star }, fill.Definition.Rows.Select(row => row.Type));
            Assert.Single(fill.Children);
        }
    }

    [Fact]
    public void Lists_every_command_of_the_format_with_its_help()
    {
        // The reference covers the format's vocabulary end to end — every token the parser accepts
        // is in it, spelled and explained — in the docs table's order, with the rules line's
        // attributes under a caption of their own.
        var entries = Entries();
        var rows = entries.Where(entry => entry is GridElement).ToList();
        var offered = ScriptCommands.Frame.Concat(ScriptCommands.Rules).ToList();

        Assert.Equal(offered.Count, rows.Count);
        Assert.Equal(
            offered.Select(command => command.Spelling),
            rows.Select(row => Text(Assert.IsType<GridElement>(row).Children[0])));
        Assert.Equal(
            offered.Select(command => command.Help),
            rows.Select(row => Text(Assert.IsType<GridElement>(row).Children[1])));
        Assert.Contains(entries, entry => entry is TextBlockElement caption && caption.Content == "Rules line");
    }

    [Fact]
    public void Wraps_the_help_inside_the_panel()
    {
        // A row is a grid: the spelling sits in an `Auto` column and the help in a `Star` one, so
        // the help has a width to wrap into. A `StackPanel` hands its children unbounded width,
        // which is what let the help run past the panel's border (measured live).
        foreach (var row in Entries().OfType<GridElement>())
        {
            Assert.IsType<TextBlockElement>(row.Children[1]);
            Assert.Equal(
                new[] { GridUnitType.Auto, GridUnitType.Star },
                row.Definition.Columns.Select(column => column.Type));
        }
    }

    static string? Status(ScriptTextStatus status) =>
        Assert.IsType<TextBlockElement>(StatusRow(status).Children[0]).Content;

    static ButtonElement CommandsButton(ScriptTextStatus status, bool reference) =>
        Assert.IsType<ButtonElement>(StatusRow(status, reference: reference).Children[1]);

    static GridElement StatusRow(ScriptTextStatus? status = null, bool reference = false) =>
        Assert.IsType<GridElement>(Body(status ?? Read, reference: reference).Children[0]);

    static TextBoxElement Box(ScriptTextStatus status, string text = Draft, Action<string>? typed = null) =>
        Assert.IsType<TextBoxElement>(Assert.IsType<GridElement>(EditorPane(status, text, typed).Content).Children[0]);

    /// The rows of the reference: the first pane when it is the only one, the second when the
    /// editor shares the split with it.
    static IReadOnlyList<Element> Entries() =>
        Assert.IsType<StackElement>(Assert.IsType<ScrollViewerElement>(
            FlexChildren(Assert.IsType<BorderElement>(Assert.IsType<GridElement>(
                ReferencePane().Content).Children[0]).Child!)[1]).Child!).Children;

    static Document ReferencePane() =>
        Documents(Split(RegionFor(Read, reference: true))).ElementAt(1);

    static Document EditorPane(ScriptTextStatus status, string text = Draft, Action<string>? typed = null) =>
        Assert.Single(Documents(Split(RegionFor(status, text, typed))));

    static DockSplit Split(ScriptTextEditorView view) =>
        Assert.IsType<DockSplit>(Assert.IsType<DockManager>(
            Assert.Single(Assert.IsType<FlexElement>(Body(view).Children[1]).Children)).Layout);

    static IEnumerable<Document> Documents(DockSplit split) => split.Children.OfType<Document>();

    static IReadOnlyList<Element> FlexChildren(Element element) =>
        Assert.IsType<FlexElement>(element).Children;

    static string? Text(Element element) => Assert.IsType<TextBlockElement>(element).Content;

    static GridElement Body(ScriptTextEditorView view) =>
        Assert.IsType<GridElement>(ScriptTextEditor.View(view));

    static GridElement Body(
        ScriptTextStatus status,
        string text = Draft,
        Action<string>? typed = null,
        bool reference = false) =>
        Body(RegionFor(status, text, typed, reference));

    /// The region as another test builds it — the scanner in `AccessibilityTests` walks the same
    /// element tree the pane would mount.
    internal static ScriptTextEditorView RegionFor(
        ScriptTextStatus status,
        string text = Draft,
        Action<string>? typed = null,
        bool reference = false) =>
        new(text, status, reference, () => { }, typed ?? (_ => { }));
}
