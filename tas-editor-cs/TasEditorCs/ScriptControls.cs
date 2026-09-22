using System;
using System.Collections.Generic;
using System.Globalization;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;   // Command, Element, Theme
using Microsoft.UI.Xaml;           // TextWrapping
using static Microsoft.UI.Reactor.Factories;

/// Everything the script controls region paints and everything it can do, in one record.
///
/// Gathered instead of passed one by one because this is the headless layer's entry point: the pane
/// that hosts the region is a value (`ScriptPanelProps`), so a test renders
/// <see cref="ScriptControls.View"/> rather than the pane.
///
/// The Save command is handed in rather than made here: it carries the Ctrl+S accelerator, and the
/// `CommandHost` that registers it wraps the panes above this region (`ScriptPanel`).
internal sealed record ScriptControlsView(
    ScriptTextStatus Status,
    bool Dirty,
    Command SaveCommand,
    PlaybackRules Rules,
    string SeedText,
    GameStatus Game,
    bool Preparing,
    string? Error,
    Action<string> SeedChanged,
    Action<PlaybackRules> RulesChanged,
    Action Run,
    Action Cancel);

/// The script controls region: Save, the run, the rules the run is configured by, and what the game
/// is doing.
///
/// This is the editor's one window onto the mod (`docs/API.md` in the sibling repo): the levers here
/// are the ones the python tools set before a script goes out (`drmod_api.fixed_dt` / `fps_cap` /
/// `rng`), and what they are set to is read back from the game rather than remembered from the click
/// — a panel that shows what it *asked for* while the game says something else is worse than no panel.
///
/// ⚠️ The rules are **not part of the script**. They are the mod's state for one run, which the text
/// format says itself (`docs/SCRIPT_DSL.md` §6), so they live in the editor's settings and no `.tas`
/// file is rewritten to hold them.
///
/// ⚠️ Everything here that names a control does it in its own label (`CheckBox` takes one, the
/// `NumberBox` and `ComboBox` carry a `Header`) — a bare label above a bare control is a form field
/// nobody, human or screen reader, can name.
internal static class ScriptControls
{
    internal static Element View(ScriptControlsView view) =>
        FlexColumn(
            Buttons(view),
            Rules(view),
            Line(view),
            Note(view))
        .FlexPadding(12)
        .Flex(grow: 1);

    /// Save, Run, Cancel, and whether the text has been written back.
    ///
    /// Run is live only with something to run, the mod answering, and no run in flight — the mod holds
    /// one script slot and answers a second one `409`. Filling in the panel does not need a game, but
    /// running does, and a disabled button is a better answer than a click that can only fail. Cancel is
    /// live exactly while the mod says a script is active, whoever started it (a run from the python
    /// tools is a run this panel can stop).
    static Element Buttons(ScriptControlsView view) =>
        HStack(8,
            Button(view.SaveCommand),
            Button("Run", view.Run)
                .AutomationName("Run this script in the game")
                .IsEnabled(view.Game.Online && !view.Preparing && !view.Game.ScriptActive),
            Button("Cancel", view.Cancel)
                .AutomationName("Stop the script the mod is running")
                .IsEnabled(view.Game.ScriptActive),
            view.Preparing ? ProgressRing() : Caption(view.Dirty ? "unsaved changes" : "saved"));

    /// The three rules a run applies, in one row: the tick, the frame cap, and the seed. What the
    /// seed field holds is the *text* — the documented seeds are hex (`0x55555555`,
    /// `docs/API.md` §3.10), and a number box would have to be told that in decimal.
    ///
    /// The FPS box is live only while the cap is the editor's own: a limit nobody set, sitting next to
    /// "default", reads as if it were in force.
    static Element Rules(ScriptControlsView view) =>
        HStack(12,
            CheckBox(view.Rules.FixedTick, on => view.RulesChanged(view.Rules with { FixedTick = on }),
                    label: "Fixed tick 1/60"),
            ComboBox(CapNames, (int)view.Rules.Cap,
                    index => view.RulesChanged(view.Rules with { Cap = (FpsCapMode)index }))
                .Header("Frame cap")
                .AutomationName("Frame cap")
                .Width(CapWidth),
            NumberBox(view.Rules.CustomFps, fps => view.RulesChanged(view.Rules with { CustomFps = (uint)fps }),
                    header: "FPS")
                .Range(PlaybackRules.MinFps, PlaybackRules.MaxFps)
                .AutomationName("Frames per second")
                .IsEnabled(view.Rules.Cap == FpsCapMode.Custom)
                .Width(NumberWidth),
            CheckBox(view.Rules.PinSeed, on => view.RulesChanged(view.Rules with { PinSeed = on }),
                    label: "Freeze seed"),
            TextBox(view.SeedText, view.SeedChanged)
                .Header("Seed (0x for hex)")
                .MaxLength(SeedLength)
                .AutomationName("Seed")
                .Width(SeedWidth),
            CheckBox(view.Rules.Headless, on => view.RulesChanged(view.Rules with { Headless = on }),
                    label: "Headless run"));

    /// The live status line: the game, then what the mod says about the levers — read from `/state`,
    /// never from what the panel asked for. An offline game has no state to read, so it gets the one
    /// sentence that explains why nothing else here can be trusted.
    static Element Line(ScriptControlsView view) =>
        Caption(view.Game.Online ? Status(view.Game) : Offline)
            .Foreground(Theme.SecondaryText)
            .TextWrapping(TextWrapping.Wrap);

    /// What the region says when the mod does not answer at all.
    internal const string Offline = "Offline — the game is not running the mod, or the API is not reachable";

    /// The last word: what went wrong with the last action, or what a run of this script would be.
    /// A failure keeps the mod's own wording — it already names the field it refused — and is painted
    /// in the error color, so a refused run never reads like a note.
    static Element Note(ScriptControlsView view)
    {
        if (view.Error is { } error)
        {
            return Caption(error).Foreground(Theme.SystemCritical).TextWrapping(TextWrapping.Wrap);
        }

        return Caption(Starts(view.Status)).Foreground(Theme.SecondaryText).TextWrapping(TextWrapping.Wrap);
    }

    /// The live status, as one line: the game, then what the mod says about the levers — read from
    /// `/state`, never from what the panel asked for.
    internal static string Status(GameStatus game)
    {
        var parts = new List<string> { game.MenuStatus };

        if (game.MissionName.Length > 0)
        {
            parts.Add(game.MissionName);
        }
        else if (game.MissionId != 0)
        {
            parts.Add($"mission {game.MissionId}");
        }

        parts.Add($"{game.Fps.ToString("0.0", CultureInfo.InvariantCulture)} fps");

        if (game.Script is { } script)
        {
            parts.Add($"{script.Name} {script.Frame}/{script.TotalFrames} ({Phase(script.Phase)})");
        }

        parts.Add(game.FixedTick ? "tick fixed 1/60" : "tick as in the game");
        parts.Add(game.CapLimit is { } limit ? $"cap {limit} fps" : "cap unlimited");
        parts.Add(game.RngPin == "off" ? "rng off" : $"rng {game.RngPin} {game.RngSeed}");
        if (game.Headless)
        {
            parts.Add("headless");
        }

        return string.Join(" · ", parts);
    }

    /// What a run of the text on screen would be. Read from the document the pane parsed rather than
    /// from the file: the rules line is edited as text, and the trigger and the restart are what the
    /// run is about.
    internal static string Starts(ScriptTextStatus status)
    {
        if (status.Document is not { } document)
        {
            return "Not a script the mod would run — the text region below says why";
        }

        var start = document.Trigger switch
        {
            { Ticks: { } ticks } => $"starts after {ticks} ticks of gameplay",
            { Pos: { Length: 3 } at } => $"starts at {at[0]:0.##}, {at[1]:0.##}, {at[2]:0.##}",
            _ => "starts at once",
        };

        var restart = document.Restart is null ? string.Empty : " · restarts the mission first";

        return $"“{document.Name}” {start}{restart}";
    }

    static string Phase(GameScriptPhase phase) => phase switch
    {
        GameScriptPhase.Restarting => "restarting",
        GameScriptPhase.Armed => "armed",
        GameScriptPhase.Running => "running",
        GameScriptPhase.Done => "done",
        GameScriptPhase.Stopped => "stopped",
        _ => "unknown",
    };

    /// The frame cap's own spellings, indexed by <see cref="FpsCapMode"/> — the enum's order is the
    /// combo's order.
    static readonly string[] CapNames = ["default", "unlimited", "custom"];

    /// Wide enough for "unlimited" whole.
    const double CapWidth = 152;
    const double NumberWidth = 90;
    const double SeedWidth = 130;

    /// `0x` plus eight hex digits.
    const int SeedLength = 10;
}
