using Microsoft.UI.Reactor.Core;

namespace TasEditorCs.Tests;

/// The left-hand pane: one row per script, and the selected row is the one the panel was
/// told about.
public class WorkspacePanelTests
{
    static readonly ScriptEntry[] Scripts =
    [
        new ScriptEntry("s1", "blade-run", 42),
        new ScriptEntry("s2", "barrier-flight", 198),
    ];

    [Fact]
    public void Renders_one_row_per_script()
    {
        var list = List(selectedId: "s1");

        Assert.Equal(2, list.Items.Count);
    }

    [Fact]
    public void Selection_follows_the_selected_id()
    {
        Assert.Equal(0, List(selectedId: "s1").SelectedIndex);
        Assert.Equal(1, List(selectedId: "s2").SelectedIndex);
    }

    [Fact]
    public void Selecting_a_row_reports_its_id()
    {
        var reported = new List<string>();
        var list = List(selectedId: "s1", select: reported.Add);
        var selectionChanged = list.OnSelectedIndexChanged;

        Assert.NotNull(selectionChanged);
        selectionChanged!(1);

        Assert.Equal(["s2"], reported);
    }

    [Fact]
    public void A_row_carries_the_name_and_the_frame_count()
    {
        var cells = Assert.IsType<StackElement>(WorkspacePanel.Row(Scripts[1], selected: false)).Children;

        Assert.Equal("barrier-flight", Text(cells[0]));
        Assert.Equal("198 frames", Text(cells[1]));
    }

    static TemplatedListViewElement<ScriptEntry> List(string? selectedId, Action<string>? select = null) =>
        Assert.IsType<TemplatedListViewElement<ScriptEntry>>(
            Assert.IsType<FlexElement>(
                WorkspacePanel.View(Scripts, selectedId, select ?? (_ => { }))).Children[1]);

    static string? Text(Element element) => Assert.IsType<TextBlockElement>(element).Content;
}
