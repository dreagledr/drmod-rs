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
        // The heights are the region's share of the pane — one to two to three — not a size in DIPs:
        // the host normalizes the three into ratios when all of them carry a hint (`BootstrapRatios`),
        // which is what makes the base layout proportional and the splitter drag the author's.
        Assert.Equal(new double?[] { 1, 2, 3 }, Regions(split).Select(region => region.Height));
    }

    [Fact]
    public void The_script_controls_region_hosts_the_run_controls_it_was_given()
    {
        // The pane owns no run state: the region is handed a value and paints that value's own view
        // (`ScriptControlsTests` asserts what the region makes of it). What the pane has to get right is
        // that the value reaches the region whole — its Save button is the pane's own command, and its
        // status line is the region's own reading of the game it was handed.
        var view = View(Script);
        var region = Assert.IsType<FlexElement>(Regions(Split(view)).First().Content);

        Assert.Same(
            view.SaveCommand,
            Assert.IsType<ButtonElement>(Assert.IsType<StackElement>(region.Children[0]).Children[0]).Command);
        // …and the status line is the region's own reading of the game it was handed. This view's game
        // was never polled, so that reading is the offline line.
        Assert.Equal(ScriptControls.Offline, Content(region.Children[2]));
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
        // The pane is handed the text and the shell's own reading of it rather than parsing either:
        // the status is what the region's errors and the Save button's state are about, and the region
        // only paints it.
        var editor = Assert.IsType<ComponentElement<ScriptTextEditorProps>>(
            Regions(Split(Script)).ElementAt(2).Content);

        Assert.Equal(Text, editor.Props.Text);
        Assert.True(editor.Props.Status.IsOk);
        Assert.Equal(1u, ScriptTextStatus.LastFrame(editor.Props.Status.Document!));
    }

    [Fact]
    public void The_save_command_is_the_one_the_pane_registers_and_the_controls_region_shows()
    {
        // One command for the Ctrl+S accelerator and for the controls region's button, so the shortcut
        // and the button cannot disagree about whether there is anything to write back.
        var view = View(Script, dirty: true);
        var host = Assert.IsType<CommandHostElement>(
            Assert.Single(Assert.IsType<FlexElement>(ScriptPanel.View(view)).Children));

        Assert.Same(view.SaveCommand, Assert.Single(host.Commands));
        Assert.Same(view.SaveCommand, view.Controls.SaveCommand);
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
        var children = Assert.IsType<FlexElement>(
            ScriptPanel.View(View(null))).Children;

        Assert.Single(children);
        Assert.Equal("No script selected.", Content(children[0]));
    }

    static ScriptPanelView View(ScriptEntry? script, string text = Text, bool dirty = false)
    {
        // One command for the pane and for the run controls: the shell builds it once from its own
        // `dirty`, and both readers have to see the same instance.
        var save = SaveCommand(dirty);
        return new(script,
            text,
            ScriptTextStatus.Of(ScriptDsl.Lines(text)),
            save,
            Controls(text, dirty, save),
            _ => { });
    }

    static Command SaveCommand(bool dirty) => StandardCommand.Save(() => { }, dirty);

    static ScriptControlsView Controls(string text = Text, bool dirty = false, Command? save = null) =>
        new(ScriptTextStatus.Of(ScriptDsl.Lines(text)),
            dirty,
            save ?? SaveCommand(dirty),
            PlaybackRules.Default,
            "1",
            GameStatus.Offline,
            Preparing: false,
            Error: null,
            _ => { },
            _ => { },
            () => { },
            () => { },
            () => { });

    static DockManager Host(ScriptPanelView view) =>
        Assert.IsType<DockManager>(Assert.IsType<CommandHostElement>(
            Assert.Single(Assert.IsType<FlexElement>(ScriptPanel.View(view)).Children))
            .Child);

    static DockSplit Split(ScriptPanelView view) => Assert.IsType<DockSplit>(Host(view).Layout);

    static DockSplit Split(ScriptEntry? script, string text = Text, bool dirty = false) =>
        Split(View(script, text, dirty));

    static IEnumerable<Document> Regions(DockSplit split) => split.Children.OfType<Document>();

    static string? Content(Element element) => Assert.IsType<TextBlockElement>(element).Content;
}
