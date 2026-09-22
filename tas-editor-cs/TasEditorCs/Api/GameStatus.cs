using System;

/// What the editor knows about the game right now: the state it polls, not the state it sends.
///
/// This is the reader's half of the API — <see cref="PlaybackRules"/> is the writer's. It carries
/// only what the status line and the buttons are decided by: the snapshot the mod keeps answering
/// with is much larger (player, camera, frame ring), and none of that belongs in a control panel.
internal sealed record GameStatus(
    bool Online,
    string MenuStatus,
    int MissionId,
    string MissionName,
    GameScript? Script,
    float Fps,
    bool FixedTick,
    uint? CapLimit,
    string RngPin,
    uint RngSeed,
    bool Headless)
{
    /// The menu status that means the game is being played — the one state `Run` requires, and the
    /// string the mod's own `GameMenuStatus::name()` produces (`docs/API.md` §3.2).
    internal const string InGame = "In Game";

    /// No answer from the API: nothing is injected, or the game is not running at all. The editor
    /// keeps working (the workspace is on disk) — only the run controls go quiet.
    internal static readonly GameStatus Offline = new(
        Online: false,
        MenuStatus: "—",
        MissionId: 0,
        MissionName: string.Empty,
        Script: null,
        Fps: 0f,
        FixedTick: false,
        CapLimit: null,
        RngPin: "off",
        RngSeed: 0,
        Headless: false);

    /// The mod's `/state` as the panel's model. A snapshot that misses a sub-object is answered for
    /// with the mod's own defaults rather than refused: half a status line beats a blank panel.
    internal static GameStatus Of(ApiJson.StateResponse state) => new(
        Online: true,
        MenuStatus: state.MenuStatus ?? "—",
        MissionId: state.MissionId,
        MissionName: state.MissionName ?? string.Empty,
        Script: state.Script is { } script ? GameScript.Of(script) : null,
        Fps: state.Fps,
        FixedTick: state.Dt?.Fixed ?? false,
        CapLimit: state.FpsCap?.Limit,
        RngPin: state.RngPin ?? "off",
        RngSeed: state.RngSeed,
        Headless: state.Render?.Headless ?? false);

    /// Whether the game is in gameplay, which is what a run needs: the script's own restart plays
    /// the pause menu, and a menu already open would swallow those keys.
    internal bool InGameplay => Online && MenuStatus == InGame;

    /// Whether a script holds the mod's one slot — the same set the mod refuses a second run with
    /// (`is_active` in `src/api.rs`). Cancel is live exactly while this is true, whoever started it.
    internal bool ScriptActive => Script is { Active: true };
}

/// The script slot, as the status line reads it. `Active` is the mod's own `is_active` set.
internal sealed record GameScript(uint Id, string Name, GameScriptPhase Phase, uint Frame, uint TotalFrames)
{
    internal bool Active => Phase is GameScriptPhase.Restarting or GameScriptPhase.Armed
        or GameScriptPhase.Running;

    internal static GameScript Of(ApiJson.ScriptResponse script) =>
        new(script.Id, script.Name ?? string.Empty, Read(script.Status), script.Frame, script.TotalFrames);

    /// The word the mod sent, as a value. An unknown word is <see cref="GameScriptPhase.Unknown"/>
    /// rather than a failure: a new status on the mod's side must not blind the whole panel.
    static GameScriptPhase Read(string? status) => status switch
    {
        "restarting" => GameScriptPhase.Restarting,
        "armed" => GameScriptPhase.Armed,
        "running" => GameScriptPhase.Running,
        "done" => GameScriptPhase.Done,
        "stopped" => GameScriptPhase.Stopped,
        _ => GameScriptPhase.Unknown,
    };
}

internal enum GameScriptPhase
{
    Unknown,
    Restarting,
    Armed,
    Running,
    Done,
    Stopped,
}
