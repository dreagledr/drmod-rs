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

    [Fact]
    public void Names_the_frame_the_refused_line_was_on()
    {
        // A `.tas` is navigated by frame, not by line: the line says where in the file the text broke,
        // and the frame says where in the script that was — which is what makes a typo findable. The
        // frame of the line itself is read before its tokens are, so the number is the one to look at.
        var refused = ScriptTextStatus.Of("! trig=ticks:0\n0 a:4\n4 x:2\n20 zz\n");

        Assert.StartsWith("line 4: unknown token 'zz'", refused.Error);
        Assert.EndsWith("· up to frame 20", refused.Error);
    }

    [Fact]
    public void Names_the_frame_of_a_line_whose_own_frame_number_is_malformed()
    {
        // The frame number is read before the tokens are checked, so even a line that fails on its
        // very first token reports the frame it named: `lt` is a stick, and `lt:2d` is its bad value.
        var refused = ScriptTextStatus.Of("! trig=ticks:0\n0 a:2\n132 lsx:997:2 lsy:79:2 lt:2d\n");

        Assert.StartsWith("line 3: lt '2d' is not a number", refused.Error);
        Assert.EndsWith("· up to frame 132", refused.Error);
    }

    [Fact]
    public void Names_no_frame_while_the_parse_has_not_reached_one()
    {
        // A broken rules line comes before any frame exists — there is no frame to name, and a
        // made-up zero would read as one.
        var refused = ScriptTextStatus.Of("! name=script\n! name=twice\n");

        Assert.StartsWith("line 2: the rules line must come first", refused.Error);
        Assert.DoesNotContain("frame", refused.Error);
    }

    [Fact]
    public void Names_the_last_frame_of_the_text_when_the_limits_refuse_the_document()
    {
        // The cross-field limits (`ScriptJson.Validate`) refuse a document, not a line: the frame is
        // the last one the whole text had — the command starting at 3600 is the one that is too long,
        // and the text ran up to 3600.
        var refused = ScriptTextStatus.Of($"! trig=ticks:0\n3500 a:1\n3600 x:2\n");

        Assert.StartsWith("commands[1]: t+duration exceeds max 3600", refused.Error);
        Assert.EndsWith("· up to frame 3600", refused.Error);
    }

    [Fact]
    public void Reads_a_draft_the_way_the_text_box_reports_it()
    {
        // Measured live: a WinUI `TextBox` hands its text back with a lone `\r`, and the pane puts
        // the format's separator back on the way into the parser (`ScriptDsl.Lines`) — a draft read
        // as it comes would be one long line, and the caption would paint an error the user cannot
        // clear by editing the text.
        var reported = ScriptTextStatus.Of(ScriptDsl.Lines(Draft.Replace('\n', '\r')));

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
    ///
    /// The three holders are the region's own wiring (`Mirror`): it is the mounted controls that know
    /// where the text has scrolled, and `OnMount` is what fills the first two, so a test that only
    /// renders the tree gets fresh, empty ones — what a headless test cannot do is make them follow
    /// each other.
    internal static ScriptTextEditorView RegionFor(
        ScriptTextStatus status,
        string text = Draft,
        Action<string>? typed = null,
        bool reference = false) =>
        new(text, status, reference, () => { }, typed ?? (_ => { }));
}
