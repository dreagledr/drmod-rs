using System.Collections.Generic;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;    // BackdropKind
using Microsoft.UI.Reactor.Docking; // DockManager, DockSplit, DockTabGroup, DockGroupRole
using Microsoft.UI.Xaml.Controls;   // Orientation
using static Microsoft.UI.Reactor.Factories;

/// Root component: the window shell.
///
/// It owns the *content* — which panes exist and what each shows — and the docking host
/// owns the *shape* (split ratios, panes the user drags out or re-docks). The host matches
/// the two by pane `Key`, so `Layout` is rebuilt from this state on every render and never
/// stored back: feeding the host's live tree into our own state double-owns the shape and
/// breaks re-docking, tab switching and splitter drags.
sealed class Editor : Component
{
    // Stub data. Real scripts come from the on-disk workspace, which is not wired up yet.
    static readonly IReadOnlyList<ScriptEntry> StubScripts =
    [
        new ScriptEntry("s1", "blade-run", 42),
        new ScriptEntry("s2", "barrier-flight", 198),
        new ScriptEntry("s3", "lightning-strike", 7),
    ];

    public override Element Render()
    {
        var (selectedId, setSelectedId) = UseState<string?>(StubScripts[0].Id);
        var selected = Find(StubScripts, selectedId);

        var workspacePane = new ToolWindow
        {
            Title = "Scripts",
            Key = WorkspacePaneKey,
            // Collapsing the workspace is forbidden outright: every route out of the docked
            // state lands it in a state the shell does not survive yet. Measured on the pin /
            // auto-hide affordance — collapsed, the pane unrenders but the split still reserves
            // its half, leaving an empty left column with an orphaned tab; brought back, it
            // returns as an unthemed floating overlay (dark surface, dark heading on top of it,
            // caption clipped) covering the document. The cause is library-side — the collapse
            // does not release the split space, and the floating chrome gets no app theme — so
            // the stub keeps the pane pinned in place instead.
            CanPin = false,
            CanAutoHide = false,
            CanHide = false,
            CanFloat = false,
            Content = Component<WorkspacePanel, WorkspacePanelProps>(
                new WorkspacePanelProps(StubScripts, selectedId, setSelectedId)),
        };

        var scriptPane = new Document
        {
            Title = selected?.Name ?? "No script",
            Key = ScriptPaneKey,
            // The pane shows whichever script is selected, so there is nothing to close
            // yet — closing becomes a real action when scripts are opened rather than
            // selected.
            CanClose = false,
            Content = Component<ScriptPanel, ScriptPanelProps>(new ScriptPanelProps(selected)),
        };

        var layout = new DockSplit(Orientation.Horizontal, new DockNode[]
        {
            new DockTabGroup(new DockableContent[] { workspacePane }, Width: 340,
                Role: DockGroupRole.General),
            new DockTabGroup(new DockableContent[] { scriptPane },
                Role: DockGroupRole.DocumentArea),
        });

        var titleBar = TitleBar("TAS Editor").Flex(shrink: 0);

        return FlexColumn(
            titleBar,
            new DockManager { Layout = layout }.Flex(grow: 1, basis: 0)
        ).Backdrop(BackdropKind.Mica);
    }

    const string WorkspacePaneKey = "tool:workspace";
    const string ScriptPaneKey = "doc:script";

    static ScriptEntry? Find(IReadOnlyList<ScriptEntry> scripts, string? id)
    {
        foreach (var script in scripts)
        {
            if (script.Id == id) return script;
        }
        return null;
    }
}
