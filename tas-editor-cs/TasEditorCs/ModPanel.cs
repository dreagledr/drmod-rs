using System;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;            // TextWrapping
using static Microsoft.UI.Reactor.Factories;

/// The installable mod, and what installing it would do — in one record, like every other pane's
/// view: the pane takes its facts rather than reaching for them, so the headless layer can render
/// it (`ModPanel.View`).
internal sealed record ModPanelView(
    string? GameFolder,
    ModState State,
    bool LoaderPresent,
    bool LoaderOurs,
    bool CanRemove,
    string? PayloadError,
    string? Message,
    bool Busy,
    Action ChooseFolder,
    Action Install,
    Action Uninstall,
    Action Detect);

/// The mod's install, painted at the top of the workspace pane: where the editor puts the mod into
/// the game, and the whole of the install UX.
///
/// The mod is two files (`ModInstaller`), and the game loads them itself on its next start — so the
/// pane's job is to say where the game is, offer the two buttons, and say what happened. There is no
/// "start the game" here on purpose: the editor does not launch, kill or inject anything (`Run` in
/// the script controls talks to a game already running, and this one talks to a game that is not).
///
/// ⚠️ The button's word follows the state rather than being one fixed label: `Install` when the game
/// has no plugin, `Reinstall` when it has this same build, `Update` when it has a different one. A
/// button that says `Install` over a folder that already holds the mod is a button that does not say
/// what it will do.
///
/// ⚠️ A static class, not a `Component` with props: it is *content* of the workspace pane and not a
/// pane of its own, so `WorkspacePanel.View` calls this directly (the way `ScriptPanel` calls
/// `ScriptControls`) and nothing reconciles it through the docking host.
internal static class ModPanel
{
    internal static Element View(ModPanelView view) =>
        FlexColumn(
            Heading("Mod"),
            Where(view),
            State(view),
            Buttons(view),
            Note(view))
        .FlexPadding(12)
        // ⚠️ No `grow`: the install is content-sized and takes only the height its lines need, and the
        // pane's other child — the script list — is the one with `grow: 1`. Giving this one a `grow`
        // too split the column down the middle, so a two-line install reserved half the pane and the
        // list of scripts scrolled in what was left.
        .Flex(shrink: 0);

    /// The game's folder: the path it was found at, the button that picks another, and the button
    /// that looks again. A path wraps — a pane is 340 DIP wide and a real Steam path is longer.
    static Element Where(ModPanelView view) =>
        VStack(6,
            HStack(8,
                Button("Game folder…", view.ChooseFolder),
                Button("Detect", view.Detect).IsEnabled(!view.Busy)),
            Caption(view.GameFolder ?? "No game folder found")
                .Foreground(Theme.SecondaryText)
                .TextWrapping(TextWrapping.Wrap));

    /// What the game folder holds. The one line the pane exists for: whether the mod is already
    /// there, and — when the folder has a `d3d9.dll` that is not the one this editor would write —
    /// that a foreign loader is present and will be left alone.
    static Element State(ModPanelView view)
    {
        var (text, error) = view.State switch
        {
            ModState.NoGameFolder => (
                "Point the editor at the folder holding METAL GEAR RISING REVENGEANCE.exe.",
                false),
            ModState.NotInstalled => ("The mod is not installed.", false),
            ModState.Installed => ("The mod is installed and up to date.", false),
            _ => ("A different build of the mod is installed.", false),
        };

        var second = Loader(view);
        return VStack(4,
            Caption(text).Foreground(error ? Theme.SystemCritical : Theme.SecondaryText),
            Caption(second).Foreground(Theme.SecondaryText).TextWrapping(TextWrapping.Wrap));
    }

    /// The loader's own line, which is a separate fact from the plugin's: the plugin does not need
    /// *our* loader, so a foreign `d3d9.dll` is news, not a problem.
    static string Loader(ModPanelView view)
    {
        if (view.State == ModState.NoGameFolder)
        {
            return string.Empty;
        }

        if (!view.LoaderPresent)
        {
            return $"No {ModInstaller.LoaderPath} in the game folder — installing adds one.";
        }

        return view.LoaderOurs
            ? $"This build's {ModInstaller.LoaderPath} is already there."
            : $"A different {ModInstaller.LoaderPath} is there (ReShade, an ENB, another mod). It will be kept — any ASI loader loads this plugin.";
    }

    static Element Buttons(ModPanelView view) =>
        HStack(8,
            Button(Label(view), view.Install)
                .AutomationName("Install the mod into the game folder")
                .IsEnabled(CanInstall(view)),
            // Live only when there is really something of ours to take out — a button that can only
            // answer "nothing to remove" is a click that should not have been offered. The question
            // is asked before this (`ModInstaller.CanRemove`) and arrives as a value.
            Button("Uninstall", view.Uninstall)
                .AutomationName("Remove the mod from the game folder")
                .IsEnabled(view.CanRemove && !view.Busy),
            view.Busy ? ProgressRing() : Caption(string.Empty));

    /// The install is live when there is a folder to write to and a payload to write, and no run
    /// already in flight. `Installed` is still live: the same button is how a user re-lays the files
    /// after deleting one of them by hand, and the word says `Reinstall` so the click is honest.
    static bool CanInstall(ModPanelView view) =>
        !view.Busy && view.PayloadError is null && view.State != ModState.NoGameFolder;

    static string Label(ModPanelView view) => view.State switch
    {
        ModState.Installed => "Reinstall",
        ModState.OtherVersion => "Update",
        _ => "Install",
    };

    /// The last word: the payload's absence, or what the last install did. The payload reason is an
    /// error and is painted as one — it is a broken build, not a note about the game.
    static Element Note(ModPanelView view)
    {
        if (view.PayloadError is { } payload)
        {
            return Caption(payload).Foreground(Theme.SystemCritical).TextWrapping(TextWrapping.Wrap);
        }

        if (view.Message is { } message)
        {
            return Caption(message).Foreground(Theme.SecondaryText).TextWrapping(TextWrapping.Wrap);
        }

        return Caption("Installing copies two files into the game; the game loads them on its next start.")
            .Foreground(Theme.SecondaryText)
            .TextWrapping(TextWrapping.Wrap);
    }
}
