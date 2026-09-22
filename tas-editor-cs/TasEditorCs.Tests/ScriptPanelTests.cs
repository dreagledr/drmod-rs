using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Docking;
using Microsoft.UI.Xaml.Controls;

namespace TasEditorCs.Tests;

/// The right-hand pane. Assertions are structural: `Element` is a record, so the rendered
/// tree can be inspected directly. Nothing here creates a WinUI control — a headless test
/// cannot, and gets a COMException if it tries.
public class ScriptPanelTests
{
    const string Text = "! trig=ticks:0\n0 a\n";
    static readonly ScriptEntry Script = new(@"C:\workspace\blade-run.tas", "blade-run", Text, 1, null);

    [Fact]
    public void Stacks_three_bare_panes_top_to_bottom()
    {
        var split = Split(Script);

        Assert.Equal(Orientation.Vertical, split.Orientation);
        // Bare panes, never tab groups: a group would render a tab strip around the region.
        Assert.Equal(
            new object[] { "script:controls", "script:table", "script:text" },
            Regions(split).Select(region => region.Key));
        // The first two carry an initial height; the last one takes what is left of the pane.
        Assert.Equal(new double?[] { 180, 320, null }, Regions(split).Select(region => region.Height));
    }

    [Fact]
    public void The_script_controls_region_names_the_selected_script()
    {
        Assert.Equal(
            "1 frames — name, trigger and restart policy come next",
            Content(Body(0).Children[1]));
    }

    [Fact]
    public void The_script_controls_region_reads_the_text_not_the_file_behind_it()
    {
        // The entry's file ends on frame 1; the text on screen is what the pane is about, and it is
        // edited freely without being saved. A line saying "not a script the mod would run" above a
        // text region that reads it perfectly is the contradiction this pins down.
        var shorter = "! trig=ticks:0\n0 a:9\n";

        Assert.Equal(
            "9 frames — name, trigger and restart policy come next",
            Content(Body(0, shorter).Children[1]));

        Assert.Equal(
            "Not a script the mod would run — the text region below says why",
            Content(Body(0, "0 zz\n").Children[1]));
    }

    [Fact]
    public void The_script_controls_region_says_when_the_text_has_not_been_written_back()
    {
        Assert.Equal("saved", Caption(Controls(Script), 1));
        Assert.Equal("unsaved changes", Caption(Controls(Script, dirty: true), 1));
    }

    [Fact]
    public void Save_is_live_only_while_there_is_something_to_write_back()
    {
        // The writer decides: an enabled Save with nothing to save would be a button that lies
        // about having work to do. What the text *says* is not part of it — a half-written script
        // is still work worth keeping, so a refused text saves like any other.
        Assert.True(SaveButton(Script, dirty: true).Command!.IsEnabled);
        Assert.False(SaveButton(Script, dirty: false).Command!.IsEnabled);
        Assert.True(SaveButton(Script, text: "0 zz\n", dirty: true).Command!.IsEnabled);
    }

    [Fact]
    public void The_command_table_region_hosts_the_table_for_the_selected_script()
    {
        var table = Assert.IsType<ComponentElement<CommandTableProps>>(
            Regions(Split(Script)).ElementAt(1).Content);

        // The table takes the script for its identity, the text for its frames and the pane's own
        // parse for the empty state: it visualizes the text on screen, not the file behind it.
        Assert.Equal(Script, table.Props.Script);
        Assert.Equal(Text, table.Props.Text);
        Assert.True(table.Props.Status.IsOk);
    }

    [Fact]
    public void The_script_text_region_hosts_the_editor_on_the_text_and_status_it_was_given()
    {
        // The pane is handed the text and its own reading of it rather than computing either: the
        // status is what its Save button is enabled by, and the region only paints it.
        var editor = Assert.IsType<ComponentElement<ScriptTextEditorProps>>(
            Regions(Split(Script)).ElementAt(2).Content);

        Assert.Equal(Text, editor.Props.Text);
        Assert.True(editor.Props.Status.IsOk);
        Assert.Equal(1u, ScriptTextStatus.LastFrame(editor.Props.Status.Document!));
    }

    [Fact]
    public void A_refused_text_reaches_the_pane_as_its_parsers_own_message()
    {
        var refused = ScriptTextStatus.Of(ScriptDsl.Lines("0 zz\n"));

        Assert.False(refused.IsOk);
        Assert.StartsWith("line 1: unknown token 'zz'", refused.Error);
    }

    [Fact]
    public void Says_so_when_nothing_is_selected()
    {
        var children = Assert.IsType<FlexElement>(ScriptPanel.View(View(null))).Children;

        Assert.Single(children);
        Assert.Equal("No script selected.", Content(children[0]));
    }

    static ScriptPanelView View(ScriptEntry? script, string text = Text, bool dirty = false) =>
        new(script, text, dirty, ScriptTextStatus.Of(ScriptDsl.Lines(text)), _ => { }, () => { });

    static DockManager Host(ScriptEntry? script, string text = Text, bool dirty = false) =>
        Assert.IsType<DockManager>(Assert.IsType<CommandHostElement>(
            Assert.Single(Assert.IsType<FlexElement>(ScriptPanel.View(View(script, text, dirty))).Children))
            .Child);

    static DockSplit Split(ScriptEntry? script, string text = Text, bool dirty = false) =>
        Assert.IsType<DockSplit>(Host(script, text, dirty).Layout);

    static IEnumerable<Document> Regions(DockSplit split) => split.Children.OfType<Document>();

    /// The body of one region — the controls one is a flex column of its own.
    static FlexElement Body(int index, string text = Text, bool dirty = false) =>
        Assert.IsType<FlexElement>(Regions(Split(Script, text, dirty)).ElementAt(index).Content);

    static StackElement Controls(ScriptEntry script, bool dirty = false) =>
        Assert.IsType<StackElement>(Body(0, Text, dirty).Children[0]);

    static ButtonElement SaveButton(ScriptEntry script, bool dirty, string text = Text) =>
        Assert.IsType<ButtonElement>(Assert.IsType<StackElement>(Body(0, text, dirty).Children[0]).Children[0]);

    static string? Caption(StackElement row, int index) => Content(row.Children[index]);

    static string? Content(Element element) => Assert.IsType<TextBlockElement>(element).Content;
}
