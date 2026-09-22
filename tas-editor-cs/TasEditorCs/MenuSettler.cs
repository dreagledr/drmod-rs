using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

/// Getting the game back into gameplay before a run — the editor's port of the python tools' menu
/// work (`drmod_api.ensure_gameplay` / `recover_fail` in the sibling repo).
///
/// Why it is needed at all: a script's own `restart` plays the pause menu, and a menu that is
/// already open swallows those keys — so a run started from the pause menu, or after a death left
/// the fail menu up, would arm and then not move. The python tools answer that by driving the menu
/// with short mod scripts (a `pause` bit toggles the pause menu, a `confirm` takes the fail menu's
/// preselected Retry) rather than by synthesising input, and the editor does the same.
///
/// ⚠️ This only reads as far as the *mod's* status names. A menu the mod has no script for — the
/// game's front end, a mission that has not finished loading — is not something a blind confirm
/// should be pointed at, so those come back as a message naming the state instead.
///
/// ⚠️ The steps need the game's window in the foreground (`GameWindow`): the menu reads its keyboard
/// through the game's own poll, which gives up when the window is not foreground. The caller brings
/// it forward before asking.
internal static class MenuSettler
{
    /// Long enough for a script of a handful of frames to be played and the pause to leave
    /// (`ProcessOutOfPause`), short enough that a stuck menu does not hold the click forever.
    static readonly TimeSpan ScriptTimeout = TimeSpan.FromSeconds(10);

    /// After the menu has been asked to close, the status does not turn `In Game` in the same tick —
    /// the transition runs through `ProcessOutOfPause`.
    static readonly TimeSpan Settle = TimeSpan.FromSeconds(3);

    /// How often the status is re-read while waiting.
    static readonly TimeSpan Poll = TimeSpan.FromMilliseconds(100);

    /// The menu statuses that mean the game is being played. The mod's own `GameMenuStatus::name()`
    /// produces these (`docs/API.md` §3.2).
    internal const string InGame = "In Game";

    /// The pause menu — the one a `pause` bit toggles.
    internal const string PauseMenu = "Pause Menu";

    /// Menus the player is dead in: the game does not leave them on its own, and their preselected
    /// entry is Retry, so a single confirm is the whole way out (measured 2026-09-11 in the tools).
    static readonly string[] FailMenus = ["Mission Fail", "Mission Failed", "Game Over"];

    /// The statuses the game spends a mission load in. A run cannot start here — and it does not
    /// need to be forced out of here either, because it is on its way out.
    static readonly string[] LoadingMenus =
    [
        "Loading Into Mission",
        "Loading Into Boss Mission",
        "Main Menu Load",
    ];

    /// Brings the game into gameplay, whatever menu it is in. Returns why not, or null when it is
    /// playing.
    ///
    /// The order is the tools': already playing is nothing to do; a fail menu is a confirm; anything
    /// else the mod knows how to press is the pause toggle. A status that is none of those is a
    /// refusal, because the alternative is guessing at a menu this client has never driven.
    internal static async Task<string?> EnsureGameplayAsync(ModApi api, CancellationToken ct)
    {
        var status = await StatusAsync(api, ct);
        if (status is null)
        {
            return "the mod stopped answering while the menu was being settled";
        }

        if (status == InGame)
        {
            return null;
        }

        if (Array.Exists(FailMenus, menu => string.Equals(menu, status, StringComparison.OrdinalIgnoreCase)))
        {
            return await RecoverAsync(api, ct)
                ? null
                : $"the game is stuck in “{status}” — the retry confirm did not bring it back";
        }

        if (string.Equals(status, PauseMenu, StringComparison.OrdinalIgnoreCase))
        {
            return await ClosePauseAsync(api, ct)
                ? null
                : $"the pause menu did not close (still “{await StatusAsync(api, ct) ?? "no answer"}”)";
        }

        if (Array.Exists(LoadingMenus, menu => string.Equals(menu, status, StringComparison.OrdinalIgnoreCase)))
        {
            return $"the game is loading (“{status}”) — a run needs gameplay";
        }

        return $"the game is in “{status}”, and this panel only knows how to leave the pause and fail menus";
    }

    /// Toggles the pause menu shut — the same one-frame `pause` bit the tools send
    /// (`drmod_api.ensure_gameplay`), played by the mod itself.
    internal static async Task<bool> ClosePauseAsync(ModApi api, CancellationToken ct)
    {
        var played = await PlayAsync(api, "close-menu", command => command with { Pause = true }, ct);
        if (!played)
        {
            return false;
        }

        return await WaitForAsync(api, InGame, Settle, ct);
    }

    /// Takes the fail menu's preselected Retry — `recover_fail` in the tools.
    internal static async Task<bool> RecoverAsync(ModApi api, CancellationToken ct)
    {
        var played = await PlayAsync(api, "fail-retry", command => command with { Confirm = true }, ct);
        if (!played)
        {
            return false;
        }

        return await WaitForAsync(api, InGame, ScriptTimeout, ct);
    }

    /// Runs a one-command script through the mod and waits for it to finish (or be stopped).
    ///
    /// It goes out as a script rather than as a keystroke because that is how the mod delivers menu
    /// input at all — the pause menu does not tick the input unit, so frames are read through the
    /// `isKeyDown`/`isKeyPressed` detours, which is exactly what a script's menu bits feed
    /// (`docs/API.md` §5).
    static async Task<bool> PlayAsync(
        ModApi api,
        string name,
        Func<ScriptInput, ScriptInput> input,
        CancellationToken ct)
    {
        var script = new ScriptDocument
        {
            Name = name,
            Commands =
            [
                // Three frames, as in the tools: a menu bit has to be down across the frame the menu
                // reads, and one frame is what the poll can miss.
                new ScriptCommand { T = 0, Duration = 3, Input = input(new ScriptInput()) },
            ],
        };

        var started = await api.RunAsync(ScriptJson.Write(script), ct);
        if (started.Conflict)
        {
            // The slot is held by whatever is running. Only a script that is *not* the menu step is
            // stopped here — and the caller has already refused a run, so there is nothing else a
            // held slot could be about.
            await api.StopAsync(ct);
            started = await api.RunAsync(ScriptJson.Write(script), ct);
        }

        if (!started.Ok)
        {
            return false;
        }

        var id = started.Value!.ScriptId;
        return await WaitForAsync(
            api,
            null,
            ScriptTimeout,
            ct,
            script: id,
            finished: status => status is "done" or "stopped" or null);
    }

    /// The current menu status, or null when the mod did not answer.
    static async Task<string?> StatusAsync(ModApi api, CancellationToken ct)
    {
        var answer = await api.StateAsync(ct);
        return answer.Value?.MenuStatus;
    }

    /// Waits until the menu status is <paramref name="menu"/>, or until the script
    /// <paramref name="script"/> is over.
    ///
    /// One loop for both waits on purpose: what a step is waiting for is either "the game came back"
    /// or "the script I sent is no longer running", and both are read from the same snapshot.
    static async Task<bool> WaitForAsync(
        ModApi api,
        string? menu,
        TimeSpan timeout,
        CancellationToken ct,
        uint? script = null,
        Func<string?, bool>? finished = null)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline)
        {
            var answer = await api.StateAsync(ct);
            if (answer.Value is not { } snapshot)
            {
                // No answer is not a failure to report here: the game blocks its render loop while a
                // mission loads, and the caller's own poll will say so if it stays that way.
                await Task.Delay(Poll, ct);
                continue;
            }

            if (menu is not null && string.Equals(snapshot.MenuStatus, menu, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }

            if (finished is not null && snapshot.Script?.Id != script)
            {
                // The mod has no script under that id any more: it finished, or it was stopped.
                return finished(null);
            }

            if (finished is not null && snapshot.Script is { } running
                && running.Id == script && finished(running.Phase.ToString().ToLowerInvariant()))
            {
                return true;
            }

            await Task.Delay(Poll, ct);
        }

        return false;
    }
}
