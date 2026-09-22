using System.Collections.Generic;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Controls;

namespace TasEditorCs.Tests;

/// The left-hand pane: one row per script of the folder, and the operations that manage them.
public class WorkspacePanelTests
{
    const string Folder = @"C:\workspace";
    const string BladeRunPath = @"C:\workspace\blade-run.tas";
    const string BarrierPath = @"C:\workspace\barrier-flight.tas";

    static readonly ScriptEntry[] Scripts =
    [
        new(BladeRunPath, "blade-run", "! trig=ticks:0\n0 a\n", 42, null),
        new(BarrierPath, "barrier-flight", "! trig=ticks:0\n0 a\n", 198, null),
    ];

    [Fact]
    public void Renders_one_row_per_script()
    {
        Assert.Equal(2, List().Items.Count);
    }

    [Fact]
    public void Selection_follows_the_selected_path()
    {
        Assert.Equal(0, List(selected: BladeRunPath).SelectedIndex);
        Assert.Equal(1, List(selected: BarrierPath).SelectedIndex);
        // A selection the folder no longer holds is no selection: a path is the identity, and a
        // deleted file leaves nothing behind for the list to highlight.
        Assert.Equal(-1, List(selected: @"C:\workspace\deleted.tas").SelectedIndex);
        Assert.Equal(-1, List(selected: null).SelectedIndex);
    }

    [Fact]
    public void Selecting_a_row_reports_its_path()
    {
        var reported = new List<string>();
        var selectionChanged = List(select: reported.Add).OnSelectedIndexChanged;

        Assert.NotNull(selectionChanged);
        selectionChanged!(1);

        Assert.Equal([BarrierPath], reported);
    }

    [Fact]
    public void A_row_carries_the_name_and_the_frame_count()
    {
        var cells = Assert.IsType<StackElement>(WorkspacePanel.Row(Scripts[1], selected: false, unsaved: false)).Children;

        Assert.Equal("barrier-flight", Text(cells[0]));
        Assert.Equal("198 frames", Text(cells[1]));
    }

    [Fact]
    public void A_row_keeps_the_scripts_that_do_not_read_as_scripts_visible()
    {
        // A file that does not parse is a file with a typo in it, not a file that is not there —
        // its row carries the parser's own message rather than a frame count it does not have.
        var broken = new ScriptEntry(BarrierPath, "broken", "0 zz\n", 0, "line 1: unknown token 'zz'");
        var cells = Assert.IsType<StackElement>(WorkspacePanel.Row(broken, selected: false, unsaved: false)).Children;

        Assert.Equal("line 1: unknown token 'zz'", Text(cells[1]));
    }

    [Fact]
    public void A_row_says_when_its_text_has_not_been_written_back()
    {
        var cells = Assert.IsType<StackElement>(WorkspacePanel.Row(Scripts[1], selected: false, unsaved: true)).Children;

        Assert.Equal("unsaved · 198 frames", Text(cells[1]));
    }

    [Fact]
    public void The_file_actions_need_a_folder_and_a_selection()
    {
        Assert.Equal((true, true, true), Buttons(View(Scripts, selected: BladeRunPath, folder: Folder)));

        // No folder: nothing to create in, nothing to copy, nothing to delete.
        Assert.Equal((false, false, false), Buttons(View(Scripts, folder: null)));

        // A folder but nothing selected: New works, the two that act on a script do not.
        Assert.Equal((true, false, false), Buttons(View(Scripts, folder: Folder)));
    }

    [Fact]
    public void The_note_names_the_folder_and_its_scripts()
    {
        Assert.Equal("2 scripts in the folder", Note(View(Scripts, folder: Folder)));
    }

    [Fact]
    public void The_note_asks_for_a_folder_while_there_is_none_and_repeats_a_failure()
    {
        Assert.Equal(
            "Pick a folder to work on. Only .tas files are scripts here.",
            Note(View([], folder: null)));

        // The workspace's own wording reaches the pane — it already names the file or the folder.
        Assert.Equal(
            "The workspace folder is gone: C:\\workspace",
            Note(View([], folder: Folder, error: "The workspace folder is gone: C:\\workspace")));
    }

    static TemplatedListViewElement<ScriptEntry> List(
        string? selected = BladeRunPath,
        Action<string>? select = null) =>
        Assert.IsType<TemplatedListViewElement<ScriptEntry>>(
            Assert.IsType<FlexElement>(WorkspacePanel.View(Props(selected, select))).Children[2]);

    static FlexElement View(
        IReadOnlyList<ScriptEntry> scripts,
        string? selected = null,
        string? folder = Folder,
        string? error = null,
        IReadOnlyDictionary<string, string>? buffers = null) =>
        Assert.IsType<FlexElement>(WorkspacePanel.View(Props(selected, null, scripts, folder, error, buffers)));

    /// The buttons in the order the pane lays them out, with whether each one is live.
    static (bool New, bool Duplicate, bool Delete) Buttons(FlexElement view) =>
        (Enabled(view, 0), Enabled(view, 1), Enabled(view, 2));

    /// Whether a file action is live. `.IsEnabled(...)` lands in the element's modifiers rather
    /// than in its own record property — that is the entry the reconciler applies to the control
    /// (`ElementExtensions.IsEnabled`) — so the modifier is what the pane's own choice shows up in.
    static bool Enabled(FlexElement view, int index)
    {
        var applied = Action(view, index).Modifiers?.IsEnabled;

        Assert.NotNull(applied);
        return applied.Value;
    }

    static ButtonElement Action(FlexElement view, int index) =>
        Assert.IsType<ButtonElement>(Assert.IsType<StackElement>(view.Children[3]).Children[index]);

    /// The pane's own line under the buttons — what went wrong, or what the folder holds.
    static string? Note(FlexElement view) => Text(view.Children[4]);

    static WorkspacePanelProps Props(
        string? selected,
        Action<string>? select = null,
        IReadOnlyList<ScriptEntry>? scripts = null,
        string? folder = Folder,
        string? error = null,
        IReadOnlyDictionary<string, string>? buffers = null) =>
        new(folder,
            scripts ?? Scripts,
            buffers ?? ScriptBuffers.Empty,
            selected,
            error,
            select ?? (_ => { }),
            () => { },
            () => { },
            () => { },
            () => { });

    static string? Text(Element element) => Assert.IsType<TextBlockElement>(element).Content;
}
