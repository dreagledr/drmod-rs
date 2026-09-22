using System;
using System.Collections.Generic;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;    // BackdropKind, FolderPickerOptions
using Microsoft.UI.Reactor.Docking; // DockManager, DockSplit, DockTabGroup, DockGroupRole
using Microsoft.UI.Xaml;            // ElementTheme
using Microsoft.UI.Xaml.Controls;   // Orientation, ContentDialogButton, ContentDialogResult
using static Microsoft.UI.Reactor.Factories;

/// Root component: the window shell, and the owner of the workspace.
///
/// It owns the *content* — which panes exist, what each shows, and which folder the workspace is
/// on — and the docking host owns the *shape* (split ratios, panes the user drags out or re-docks).
/// The host matches the two by pane `Key`, so `Layout` is rebuilt from this state on every render
/// and never stored back: feeding the host's live tree into our own state double-owns the shape and
/// breaks re-docking, tab switching and splitter drags.
///
/// The workspace lives here rather than in a pane because more than one pane reads it: the list
/// shows which scripts have unsaved text, the editor shows that text, and Save writes it back. The
/// state is the folder, the listing read from it and the text buffers, tied together by one
/// counter: every write to the folder bumps `revision`, the listing is read again, and the frame
/// counts in the list come from the files themselves.
sealed class Editor : Component
{
    public override Element Render()
    {
        // The remembered folder, read from the settings file once: `UseMemo` answers with the same
        // value for every later render, so a re-render never touches the disk again.
        var remembered = UseMemo(() => EditorSettings.Load(), []);
        var (folder, setFolder) = UseState<string?>(remembered);

        // Bumped by every write to the folder — a save, a new file, a delete — which is what makes
        // the listing below read the files again.
        var (revision, updateRevision) = UseReducer<int>(0);
        var listing = UseMemo(() => Workspace.List(folder), folder, revision);

        var (selectedPath, setSelectedPath) = UseState<string?>(null);
        var (buffers, updateBuffers) = UseReducer<IReadOnlyDictionary<string, string>>(ScriptBuffers.Empty);
        var (message, setMessage) = UseState<string?>(null);
        var (pendingDelete, setPendingDelete) = UseState<ScriptEntry?>(null);

        // The folder picker, taken as a method group so that no render can open a dialog: what the
        // render captures is the helper, and the handler below is what calls it. Despite the name it
        // holds no hook slot — it looks the owning window up and returns the dialog's own task — so
        // calling it from a click is what it is for. (The library's own picker sample does the same:
        // `docs/_pipeline/apps/windows/App.cs`.)
        var pickFolder = UseFolderPickerAsync;

        var selected = Find(listing.Scripts, selectedPath);
        var dirty = selected is not null && ScriptBuffers.IsDirty(buffers, selected);

        // Hooks run on every render before anything else, so the app scheme is read
        // unconditionally and only then folded into the decision below.
        var appSchemeIsDark = UseIsDarkTheme();

        // null = follow the system. `UseIsDarkTheme` reports the app-global scheme, so it
        // only decides where the toggle starts: once the user pins a scheme, that value is
        // the truth, because the hook does not observe the per-element override below.
        var (pinnedTheme, setPinnedTheme) = UseState<ElementTheme?>(null);
        var isDark = (pinnedTheme ?? (appSchemeIsDark ? ElementTheme.Dark : ElementTheme.Light))
            == ElementTheme.Dark;

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
            // the pane stays pinned in place instead.
            CanPin = false,
            CanAutoHide = false,
            CanHide = false,
            CanFloat = false,
            Content = Component<WorkspacePanel, WorkspacePanelProps>(new WorkspacePanelProps(
                Folder: folder,
                Scripts: listing.Scripts,
                Buffers: buffers,
                SelectedPath: selectedPath,
                // Whatever went wrong with the last action, and otherwise whatever the listing
                // itself found wrong with the folder.
                Error: message ?? listing.Error,
                Select: setSelectedPath,
                ChooseFolder: ChooseFolder,
                New: NewScript,
                Duplicate: DuplicateScript,
                Delete: AskToDelete)),
        };

        var scriptPane = new Document
        {
            Title = selected?.Name ?? "No script",
            Key = ScriptPaneKey,
            // The pane shows whichever script is selected, so there is nothing to close — a file
            // leaves the workspace by being deleted from the list, not by closing a tab.
            CanClose = false,
            Content = Component<ScriptPanel, ScriptPanelProps>(new ScriptPanelProps(
                selected,
                selected is null ? string.Empty : ScriptBuffers.Resolve(buffers, selected),
                dirty,
                // A keystroke can only come from the text region, which exists only while a script
                // is selected, so the script is the selected one by construction.
                typed => updateBuffers(current => ScriptBuffers.Typed(current, selected!, typed)),
                Save)),
        };

        var layout = new DockSplit(Orientation.Horizontal, new DockNode[]
        {
            new DockTabGroup(new DockableContent[] { workspacePane }, Width: 340,
                Role: DockGroupRole.General),
            new DockTabGroup(new DockableContent[] { scriptPane },
                Role: DockGroupRole.DocumentArea),
        });

        var titleBar = TitleBar("TAS Editor")
            .RightHeader(ToggleSwitch(
                isDark,
                dark => setPinnedTheme(dark ? ElementTheme.Dark : ElementTheme.Light),
                onContent: "Dark",
                offContent: "Light"))
            .Flex(shrink: 0);

        // The theme override sits on the shell root — the region that wraps every pane — so
        // one value re-themes the lot. WinUI resolves every `ThemeResource` brush against
        // `RequestedTheme`, and the host listens for the change to re-resolve our own
        // `Theme.*` tokens as well.
        return FlexColumn(
            titleBar,
            new DockManager { Layout = layout }.Flex(grow: 1, basis: 0),
            Confirm(
                pendingDelete,
                pendingDelete is not null && ScriptBuffers.IsDirty(buffers, pendingDelete),
                Closed)
        ).Backdrop(BackdropKind.Mica)
         .RequestedTheme(pinnedTheme ?? ElementTheme.Default);

        /// Opens the folder picker and makes the answer the workspace. The picker is the host's own
        /// — it needs the window's HWND — and it is called from the handler rather than from the
        /// render on purpose: renders happen for reasons of their own, and a modal dialog is not one
        /// of them. `async void` is the shape a click handler has, so the guard is what keeps a
        /// failure from taking the process down.
        async void ChooseFolder()
        {
            try
            {
                var picked = await pickFolder(new FolderPickerOptions());
                if (picked is null) return;

                setFolder(picked.Path);
                setSelectedPath(null);
                setMessage(null);
                EditorSettings.Save(picked.Path);
            }
            catch (Exception failure)
            {
                setMessage($"The folder picker failed: {failure.Message}");
            }
        }

        /// A new, empty file, selected as soon as the listing has read it. The selection is set in
        /// the same pass as the revision, so the two land in one render — by the time it runs, the
        /// new file is in the listing.
        void NewScript()
        {
            if (folder is null) return;

            var (path, error) = Workspace.Create(folder);
            setMessage(error);
            if (path is null) return;

            updateRevision(current => current + 1);
            setSelectedPath(path);
        }

        void DuplicateScript()
        {
            if (selected is null) return;

            var (path, error) = Workspace.Duplicate(selected.Path);
            setMessage(error);
            if (path is null) return;

            updateRevision(current => current + 1);
            setSelectedPath(path);
        }

        /// Writes the selected script's buffer back over its file. What the text says is the
        /// author's business — a half-written script is still work worth keeping, so nothing here
        /// asks whether the mod would run it; the text region says that in its own status line.
        ///
        /// The buffer is left where it is: the text box already holds that text, and feeding the
        /// file's own text back to it after a save would only move the caret. What changes is the
        /// listing — the file now reads as the buffer does, so the script is no longer unsaved, and
        /// its frame count is the one the file ends on.
        void Save()
        {
            if (selected is null || !ScriptBuffers.IsDirty(buffers, selected)) return;

            setMessage(Workspace.Write(selected.Path, ScriptBuffers.Resolve(buffers, selected)));
            updateRevision(current => current + 1);
        }

        void AskToDelete()
        {
            if (selected is not null) setPendingDelete(selected);
        }

        /// The dialog's own answer: the script it was asked about is the one captured when the
        /// button was pressed, not whatever the selection is by the time the user answers.
        void Closed(ContentDialogResult result)
        {
            var doomed = pendingDelete;
            setPendingDelete(null);
            if (result != ContentDialogResult.Primary || doomed is null) return;

            var error = Workspace.Delete(doomed.Path);
            setMessage(error);
            if (error is not null) return;

            updateBuffers(current => ScriptBuffers.Without(current, doomed.Path));
            if (selectedPath == doomed.Path) setSelectedPath(null);
            updateRevision(current => current + 1);
        }
    }

    const string WorkspacePaneKey = "tool:workspace";
    const string ScriptPaneKey = "doc:script";

    /// The delete confirmation, declaratively: the dialog stays in the tree with `IsOpen` toggled,
    /// rather than a `ShowAsync` a handler drives — an imperatively shown dialog gets no parent
    /// theme and cannot be tested (REACTOR_DIALOG_001).
    ///
    /// It asks what cannot be undone, so the question names the file — and says so when that file
    /// has unsaved text, because deleting it takes the text with it. Cancel is the close button
    /// rather than the primary one: the destructive answer is the one the user has to reach for.
    ///
    /// The handler is passed in rather than wired here: it answers with the shell's own state, and
    /// a static helper cannot reach that. Keeping the dialog in the tree while it is closed is what
    /// lets the same element be the one that is opened.
    internal static Element Confirm(ScriptEntry? script, bool unsaved, Action<ContentDialogResult> closed) =>
        ContentDialog(
            "Delete script?",
            TextBlock(script is null
                ? string.Empty
                : $"Delete {script.Name}{Workspace.Extension} from the folder?"
                  + (unsaved ? " Its unsaved changes go with it." : string.Empty)),
            "Delete") with
        {
            IsOpen = script is not null,
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            OnClosed = closed,
        };

    /// The script the list is showing, if the listing still holds it — a deleted file, or a folder
    /// the user has just switched away from, leaves the selection pointing at nothing.
    static ScriptEntry? Find(IReadOnlyList<ScriptEntry> scripts, string? path)
    {
        if (path is null) return null;
        foreach (var script in scripts)
        {
            if (script.Path == path) return script;
        }
        return null;
    }
}
