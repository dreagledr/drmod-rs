using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Docking;
using Microsoft.UI.Xaml.Controls;

namespace TasEditorCs.Tests;

/// The right-hand pane. Assertions are structural: `Element` is a record, so the rendered
/// tree can be inspected directly. Nothing here creates a WinUI control — a headless test
/// cannot, and gets a COMException if it tries.
public class ScriptPanelTests
{
    [Fact]
    public void Stacks_three_bare_panes_top_to_bottom()
    {
        var split = Split(new ScriptEntry("s1", "blade-run", 42));

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
        var split = Split(new ScriptEntry("s1", "blade-run", 42));

        Assert.Contains("42 frames", Text(RegionBody(split, 0).Children[0]));
    }

    [Fact]
    public void The_command_table_region_hosts_the_table_for_the_selected_script()
    {
        var script = new ScriptEntry("s1", "blade-run", 42);
        var table = Assert.IsType<ComponentElement<CommandTableProps>>(
            Regions(Split(script)).ElementAt(1).Content);

        Assert.Equal(script, table.Props.Script);
    }

    [Fact]
    public void The_script_text_region_hosts_the_editor_on_the_text_it_was_given()
    {
        // The pane is handed the text rather than reading the mock itself: which drafts exist and
        // which one a script opens with is the pane component's state, and that is what the
        // headless layer cannot render.
        var editor = Assert.IsType<ComponentElement<ScriptTextEditorProps>>(
            Regions(Split(new ScriptEntry("s1", "blade-run", 42), "0 a\n")).ElementAt(2).Content);

        Assert.Equal("0 a\n", editor.Props.Text);
    }

    [Fact]
    public void Says_so_when_nothing_is_selected()
    {
        var children = Assert.IsType<FlexElement>(ScriptPanel.View(null, string.Empty, _ => { })).Children;

        Assert.Single(children);
        Assert.Equal("No script selected.", Text(children[0]));
    }

    static DockSplit Split(ScriptEntry? script, string text = "") =>
        Assert.IsType<DockSplit>(Assert.IsType<DockManager>(
            Assert.Single(Assert.IsType<FlexElement>(ScriptPanel.View(script, text, _ => { })).Children)).Layout);

    static IEnumerable<Document> Regions(DockSplit split) => split.Children.OfType<Document>();

    static FlexElement RegionBody(DockSplit split, int index) =>
        Assert.IsType<FlexElement>(Regions(split).ElementAt(index).Content);

    static string? Text(Element element) => Assert.IsType<TextBlockElement>(element).Content;
}
