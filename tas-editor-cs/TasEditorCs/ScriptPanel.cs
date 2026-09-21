using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Docking; // DockManager, DockSplit, Document, DockNode
using Microsoft.UI.Xaml.Controls;   // Orientation
using static Microsoft.UI.Reactor.Factories;

sealed record ScriptPanelProps(ScriptEntry? Script);

/// Right pane: the one selected script.
///
/// The body is three regions stacked top to bottom — script controls, command table,
/// script text — separated by the docking host's drag-resize splitters. The command table is
/// real (read-only, on mock frames); the other two still carry a note, and the set of panes is
/// fixed, so the host keeps its own split ratios across renders and the selection only decides
/// what the regions read.
sealed class ScriptPanel : Component<ScriptPanelProps>
{
    public override Element Render() => View(Props.Script);

    /// The pane body. Split out of the component because `Component<TProps>.Props` is
    /// read-only and set by the host, so a headless unit test has no way to render the
    /// component itself — it asserts on this instead.
    internal static Element View(ScriptEntry? script)
    {
        if (script is null)
        {
            return FlexColumn(Caption("No script selected."))
                .FlexPadding(16)
                .Flex(grow: 1);
        }

        // Orientation.Vertical means the splitters between the children are horizontal
        // bars. Initial heights belong on the panes — the split's direct children — and
        // only the last one is left open so it takes what the others do not.
        var regions = new DockSplit(Orientation.Vertical, new DockNode[]
        {
            Region(ScriptControlsKey, "Script controls", 180,
                Placeholder($"{script.Frames} frames — name, trigger and restart policy come next.")),
            Region(CommandTableKey, "Command table", 320,
                Component<CommandTable, CommandTableProps>(new CommandTableProps(script))),
            Region(ScriptTextKey, "Script text", null,
                Placeholder("The JSON view of the script comes next.")),
        });

        // A docked pane body is content-sized unless it is told to grow — the wrapper is
        // what makes the three regions fill the pane instead of collapsing to their
        // desired height at the top of it.
        return FlexColumn(
            new DockManager { Layout = regions }.Flex(grow: 1, basis: 0)
        ).Flex(grow: 1);
    }

    const string ScriptControlsKey = "script:controls";
    const string CommandTableKey = "script:table";
    const string ScriptTextKey = "script:text";

    /// One region of the body: a bare pane rather than a tab group, because a group always
    /// carries a tab strip and the only thing we want around a region is its splitter.
    ///
    /// Nothing here closes, floats or drags either — the regions are fixed and only the
    /// splitters between them move, which keeps the pane out of the docking states the
    /// shell does not survive (see `Editor.cs` and `README.md`).
    static DockNode Region(string key, string title, double? height, Element body) =>
        new Document
        {
            Title = title,
            Key = key,
            Content = body,
            Height = height,
            CanClose = false,
            CanFloat = false,
            CanMove = false,
            CanDockAsToolWindow = false,
        };

    static Element Placeholder(string note) =>
        FlexColumn(Caption(note))
            .FlexPadding(12)
            .Flex(grow: 1);
}
