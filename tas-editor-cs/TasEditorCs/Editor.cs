using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;    // BackdropKind, FolderPickerOptions, Command, StandardCommand
using Microsoft.UI.Reactor.Docking; // DockManager, DockSplit, DockTabGroup, DockGroupRole
using Microsoft.UI.Xaml;            // ElementTheme, TextWrapping
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
///
/// The run lives here too, but for the opposite reason: one pane reads it and everything about it is
/// the shell's. The parse of the text on screen is made once here (the controls region runs what the
/// text region shows), the poll of the mod's state feeds one `GameStatus`, and `Run` sequences the
/// whole thing — focus, rules, script — in one place (`PlaybackRules`, `ModApi`, `GameWindow`).
sealed class Editor : Component
{
    /// How often the game is asked what it is doing. The mod's HTTP server is single-threaded and
    /// lives in the game's render loop, so this is a couple of times a second rather than per frame —
    /// and the panel is a control panel, not a TAS readout.
    static readonly TimeSpan PollInterval = TimeSpan.FromMilliseconds(500);

    public override Element Render()
    {
        // The remembered folder and run rules, read from the settings file once: `UseMemo` answers
        // with the same value for every later render, so a re-render never touches the disk again.
        // A first launch has no remembered folder, and opens on the example scripts shipped next to
        // the exe instead of on nothing — a picked folder is remembered and takes over from then on.
        var remembered = UseMemo(() => EditorSettings.Load(), []);
        var (folder, setFolder) = UseState<string?>(remembered.Folder ?? EditorSettings.FirstFolder());

        // The rules a run is configured by. They are the mod's state for one run and not part of the
        // script — the text format says so itself (`docs/SCRIPT_DSL.md` §6) — so they live here and in
        // the settings file, and no `.tas` file is ever rewritten to hold them.
        var (rules, setRules) = UseState<PlaybackRules>(remembered.Playback);
        var (seedText, setSeedText) = UseState(remembered.SeedText);

        // Bumped by every write to the folder — a save, a new file, a rename, a delete — which is what
        // makes the listing below read the files again.
        var (revision, updateRevision) = UseReducer<int>(0);
        var listing = UseMemo(() => Workspace.List(folder), folder, revision);

        var (selectedPath, setSelectedPath) = UseState<string?>(null);
        var (buffers, updateBuffers) = UseReducer<IReadOnlyDictionary<string, string>>(ScriptBuffers.Empty);
        var (message, setMessage) = UseState<string?>(null);
        var (pendingDelete, setPendingDelete) = UseState<ScriptEntry?>(null);
        var (pendingRename, setPendingRename) = UseState<ScriptEntry?>(null);
        var (renameText, setRenameText) = UseState<string?>(null);

        // What the game is doing, and what the last run action made of it. The poll below writes these;
        // the controls region only paints them.
        var (game, setGame) = UseState(GameStatus.Offline);
        var (preparing, setPreparing) = UseState(false);
        var (runError, setRunError) = UseState<string?>(null);

        // The mod client outlives every render: it owns the connection pool, so building one per render
        // would leak sockets. The cleanup at the end of this method is what closes it.
        var client = UseRef<ModApi?>(null);
        var api = client.Current ??= new ModApi();

        // The script a headless run was already applied for. The mod restores the render by itself when
        // a run ends, so each run arms it once and never again — and never on the strength of having
        // started the script (see the poll).
        var headlessRun = UseRef<uint?>(null);

        UseEffect(() => () => api.Dispose(), Array.Empty<object>());

        // The status poll. ⚠️ Headless is armed only once the script is *really* `running`: the skip
        // hooks sit on the live device's draw calls, and putting them there while a level loads is what
        // crashed the game in `d3d9.dll` (measured — `docs/HEADLESS.md` §5). The python tools apply it
        // the same way, from the loop that watches the status (`r03_baseline.run_once`).
        var headless = rules.Headless;
        UseEffect(() =>
        {
            var cts = new CancellationTokenSource();
            var token = cts.Token;
            _ = Task.Run(async () =>
            {
                using var timer = new PeriodicTimer(PollInterval);
                try
                {
                    while (true)
                    {
                        var answer = await api.StateAsync(token);
                        if (answer.Value is { } snapshot)
                        {
                            setGame(snapshot);

                            if (headless && snapshot.Script is { Phase: GameScriptPhase.Running } script
                                && headlessRun.Current != script.Id)
                            {
                                headlessRun.Current = script.Id;
                                var applied = await api.PostAsync("/render", ApiJson.Headless(true), token);
                                if (!applied.Ok)
                                {
                                    setRunError($"headless: {applied.Message}");
                                }
                            }
                        }
                        else
                        {
                            setGame(GameStatus.Offline);
                        }

                        if (!await timer.WaitForNextTickAsync(token))
                        {
                            break;
                        }
                    }
                }
                catch (OperationCanceledException)
                {
                    // The window is closing, or the rules changed and a new loop replaced this one.
                }
            });

            // Cancel only, and deliberately so: the worker owns the timer it created, and disposing the
            // source here while it is inside `WaitForNextTickAsync` can surface on the token
            // (`docs/guide/effects-scheduling.md`).
            return () => cts.Cancel();
        }, rules);

        // The folder picker, taken as a method group so that no render can open a dialog: what the
        // render captures is the helper, and the handler below is what calls it. Despite the name it
        // holds no hook slot — it looks the owning window up and returns the dialog's own task — so
        // calling it from a click is what it is for. (The library's own picker sample does the same:
        // `docs/_pipeline/apps/windows/App.cs`.)
        var pickFolder = UseFolderPickerAsync;

        var selected = Find(listing.Scripts, selectedPath);
        var dirty = selected is not null && ScriptBuffers.IsDirty(buffers, selected);

        // One parse of the text on screen for the whole window: the controls region runs it, the text
        // region's status line reads it, the command table paints it and Save is enabled by it. A
        // 3 600-frame text re-reads in about a millisecond, so there is nothing to debounce and nothing
        // to keep in step.
        var text = selected is null ? string.Empty : ScriptBuffers.Resolve(buffers, selected);
        var status = UseMemo(() => ScriptTextStatus.Of(ScriptDsl.Lines(text)), text);

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
                Rename: AskToRename,
                Delete: AskToDelete)),
        };

        // One command for the pane's Ctrl+S and the controls region's Save button: the shortcut is
        // registered around the regions (`ScriptPanel`), and what decides whether there is anything to
        // write back is this one `dirty`.
        var saveCommand = StandardCommand.Save(Save, dirty);

        var scriptPane = new Document
        {
            Title = selected?.Name ?? "No script",
            Key = ScriptPaneKey,
            // The pane shows whichever script is selected, so there is nothing to close — a file
            // leaves the workspace by being deleted from the list, not by closing a tab.
            CanClose = false,
            Content = Component<ScriptPanel, ScriptPanelProps>(new ScriptPanelProps(
                selected,
                text,
                status,
                saveCommand,
                new ScriptControlsView(
                    status,
                    dirty,
                    saveCommand,
                    rules,
                    seedText,
                    game,
                    preparing,
                    runError,
                    SeedChanged,
                    RulesChanged,
                    Run,
                    Cancel),
                // A keystroke can only come from the text region, which exists only while a script
                // is selected, so the script is the selected one by construction.
                typed => updateBuffers(current => ScriptBuffers.Typed(current, selected!, typed)))),
        };

        var layout = new DockSplit(Orientation.Horizontal, new DockNode[]
        {
            // The widths are **weights, not DIPs**: the host bootstraps a split's ratios from its
            // children's hints only when every child carries one (`BootstrapRatios`), and normalizes
            // them — so this is the shell's base ratio, one part of workspace to three of document.
            // Once the author drags the splitter, their ratio is what the host keeps: these are the
            // starting proportions, not a fixed size.
            new DockTabGroup(new DockableContent[] { workspacePane }, Width: WorkspaceWeight,
                Role: DockGroupRole.General),
            new DockTabGroup(new DockableContent[] { scriptPane }, Width: ScriptWeight,
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
                Deleted),
            RenameScript(
                pendingRename,
                renameText,
                setRenameText,
                Renamed)
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
                EditorSettings.Save(new EditorSettingsData(picked.Path, rules, seedText));
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
        void Deleted(ContentDialogResult result)
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

        /// The rename question starts from what the file is called now, so the field is a place to
        /// correct rather than a place to type the whole name.
        void AskToRename()
        {
            if (selected is null) return;

            setRenameText(selected.Name);
            setPendingRename(selected);
        }

        /// Moves the file and takes the selection with it. The buffer travels by the new path too: a
        /// rename must not throw away text that has not been written back yet (`ScriptBuffers.Renamed`).
        void Renamed(ContentDialogResult result)
        {
            var renamed = pendingRename;
            var wanted = renameText;
            setPendingRename(null);
            if (result != ContentDialogResult.Primary || renamed is null || wanted is null) return;

            var (path, error) = Workspace.Rename(renamed.Path, wanted);
            setMessage(error);
            if (error is not null || path is null) return;

            updateBuffers(current => ScriptBuffers.Renamed(current, renamed.Path, path));
            if (selectedPath == renamed.Path) setSelectedPath(path);
            updateRevision(current => current + 1);
        }

        /// A rule was edited: it is what the *next* run will set, and it is remembered. Nothing is sent
        /// to the game here on purpose — the levers belong to a run, not to the filling in of a panel,
        /// and the status line already shows what the game is actually set to.
        void RulesChanged(PlaybackRules next)
        {
            setRules(next);
            EditorSettings.Save(new EditorSettingsData(folder, next, seedText));
        }

        /// The seed field holds text until it reads as a number: the documented seeds are hex
        /// (`0x55555555`, `docs/API.md` §3.10), so the spelling is what is kept, and a half-typed one
        /// leaves the last value that parsed in place.
        void SeedChanged(string typed)
        {
            setSeedText(typed);
            if (!PlaybackRules.TrySeed(typed, out var seed)) return;

            var next = rules with { Seed = seed };
            setRules(next);
            EditorSettings.Save(new EditorSettingsData(folder, next, typed));
        }

        /// Starts the text on screen in the game.
        ///
        /// The order is the one the python tools established (`r03_baseline.run_once`): the game window
        /// first (the menu keys a `restart` plays arrive only while the game owns the input focus), then
        /// a menu settled out of the way, then the rules, and the seed **last** of the three — the mod
        /// freezes the LCG on the first tick of the *next* script, and the script it has to land on is
        /// the one sent right after.
        ///
        /// What goes to the mod is the text on screen, parsed again here. The file is not written first:
        /// the run is of the script the author is looking at, and `Run` is not a save.
        async void Run()
        {
            if (selected is null) return;

            var wanted = ScriptTextStatus.Of(ScriptDsl.Lines(ScriptBuffers.Resolve(buffers, selected)));
            if (wanted.Document is not { } document)
            {
                setRunError(wanted.Error);
                return;
            }

            if (!PlaybackRules.TrySeed(seedText, out var seed))
            {
                setRunError("The seed is not a number — decimal, or 0x-prefixed for hex");
                return;
            }

            var runRules = rules with { Seed = seed };
            setRunError(null);
            setPreparing(true);
            try
            {
                // A fresh read before anything is sent: the poll can be half a second old, and the menu
                // is what decides whether the script's own restart can be played at all.
                var now = await api.StateAsync(CancellationToken.None);
                if (now.Value is not { } snapshot)
                {
                    setGame(GameStatus.Offline);
                    setRunError("The mod is not answering — is the game running with the mod injected?");
                    return;
                }

                setGame(snapshot);

                if (!GameWindow.FocusAndSettle())
                {
                    // A warning, not a stop: the script runs either way, and what may be lost is the
                    // menu input the script's own restart plays.
                    setRunError("The game window did not take the foreground — menu input may be lost");
                }

                // Out of the pause menu, or out of a fail menu, before the script arms — otherwise the
                // menu swallows the keys the script's own restart plays. The window is focused above,
                // which is what the menu reads its keyboard through.
                if (await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None) is { } stuck)
                {
                    setRunError(stuck);
                    return;
                }

                if (await runRules.ApplyAsync(api, CancellationToken.None) is { } lever)
                {
                    setRunError(lever);
                    return;
                }

                var json = ScriptJson.Write(document);
                var started = await api.RunAsync(json, CancellationToken.None);
                if (started.Conflict)
                {
                    // The mod holds one script slot and answers a second run with 409. A script that ended
                    // between the poll and this click is a race the panel cannot see, so the slot is taken
                    // once, by stopping whatever holds it.
                    await api.StopAsync(CancellationToken.None);
                    started = await api.RunAsync(json, CancellationToken.None);
                }

                if (!started.Ok)
                {
                    setRunError(started.Message);
                    return;
                }

                var after = await api.StateAsync(CancellationToken.None);
                if (after.Value is { } refreshed) setGame(refreshed);
            }
            catch (ScriptFormatException refused)
            {
                // The pane's parse accepted the text, but the mod's own cross-field limits refused it —
                // the message names the command.
                setRunError(refused.FrameAware());
            }
            finally
            {
                setPreparing(false);
            }
        }

        /// Stops whatever the mod is running. The render and the frame cap come back on their own: the
        /// mod restores them when a headless run ends, cancelled or not (`headless_service`).
        async void Cancel()
        {
            setRunError(null);
            var stopped = await api.StopAsync(CancellationToken.None);
            if (!stopped.Ok)
            {
                setRunError($"stop: {stopped.Message}");
                return;
            }

            var after = await api.StateAsync(CancellationToken.None);
            if (after.Value is { } snapshot) setGame(snapshot);
        }
    }

    const string WorkspacePaneKey = "tool:workspace";
    const string ScriptPaneKey = "doc:script";

    /// The shell's base proportions, one part of workspace to three of document (see the split).
    const double WorkspaceWeight = 1;
    const double ScriptWeight = 3;

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

    /// The rename question, built the same way as <see cref="Confirm"/> and for the same reason.
    ///
    /// The primary button is live only for a name that would change something: renaming a file to
    /// what it is already called, or to nothing, is not a question worth answering — and a disabled
    /// button is what says so instead of a dialog that closes on a no-op.
    ///
    /// The name asked for is the file's, not the script's: `name=` in the rules line is a different
    /// thing and is edited in the text (see `Workspace.Rename`).
    internal static Element RenameScript(
        ScriptEntry? script,
        string? name,
        Action<string> typed,
        Action<ContentDialogResult> closed)
    {
        var wanted = (name ?? string.Empty).Trim();
        var ready = script is not null
            && wanted.Length > 0
            && !string.Equals(wanted, script.Name, StringComparison.Ordinal);

        return ContentDialog(
            "Rename script",
            VStack(6,
                TextBox(name ?? string.Empty, typed)
                    .AutomationName("File name"),
                Caption($"The file stays in this folder and keeps {Workspace.Extension}")
                    .Foreground(Theme.SecondaryText)
                    .TextWrapping(TextWrapping.Wrap)),
            "Rename") with
        {
            IsOpen = script is not null,
            CloseButtonText = "Cancel",
            IsPrimaryButtonEnabled = ready,
            OnClosed = closed,
        };
    }

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
