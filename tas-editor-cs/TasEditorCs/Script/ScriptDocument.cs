using System.Collections.Generic;
using System.Text.Json.Serialization;

/// A script as the editor holds it: the whole body of the mod's `POST /script/run`
/// (`docs/API.md` §4 in the sibling Rust repo).
///
/// The document is the hub between the three representations — DSL text
/// (<see cref="ScriptDsl"/>), API JSON (<see cref="ScriptJson"/>) and the command
/// table's frames (<see cref="ScriptFrames"/>). Only the JSON carries the whole
/// format: `raw_key`, `dik_key` and `when_enemy` have no DSL spelling, and the
/// frames show neither of them.
[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
internal sealed record ScriptDocument
{
    /// Overrides the JSON's default only when the field is absent: the mod falls
    /// back to the same name (`default_script_name` in `src/api.rs`).
    internal const string DefaultName = "script";

    [JsonPropertyName("name")]
    public string Name { get; init; } = DefaultName;

    /// Arms the script (starts it on a position or a tick count) instead of
    /// running it at once.
    [JsonPropertyName("trigger")]
    public ScriptTrigger? Trigger { get; init; }

    /// Plays the pause menu and restarts the mission before arming.
    [JsonPropertyName("restart")]
    public RestartPolicy? Restart { get; init; }

    [JsonPropertyName("commands")]
    public List<ScriptCommand> Commands { get; init; } = [];
}

/// `trigger`: where — or after how many simulation ticks — an armed script starts
/// (`docs/API.md` §3.3). At least one of the two is required, which is a limit the
/// mod enforces itself (`ScriptJson.Validate`).
[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
internal sealed record ScriptTrigger
{
    /// The player's spawn area: ±0.1 m on X/Z and ±1.0 m on Y, exactly the
    /// tolerance record/playback uses.
    [JsonPropertyName("pos")]
    public float[]? Pos { get; init; }

    /// Simulation ticks of gameplay after arming — the reproducible start (a
    /// positional trigger fires within a tick of loading, and one tick of drift
    /// changes the enemy's phase).
    [JsonPropertyName("ticks")]
    public ulong? Ticks { get; init; }
}

/// `restart`: the pause-menu sequence the mod plays before arming (`RestartSpec`
/// in `src/api.rs`).
///
/// A parameter is nullable on purpose, and `null` means "the mod's default" — the
/// one the property initializer also carries. The distinction is not cosmetic: the
/// source-generated deserializer writes `default` over every property a JSON
/// leaves out (measured), so an absent `hold` arrives as `null` rather than as the
/// initializer, and a plain `uint` would read it as a literal 0 — "hold the arrow
/// for no frames at all" instead of six.
[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
internal sealed record RestartPolicy
{
    /// Menu steps up — in the pause menu Restart is the bottom entry.
    [JsonPropertyName("ups")]
    public uint? Ups { get; init; } = 1;

    [JsonPropertyName("downs")]
    public uint? Downs { get; init; }

    /// Frames an arrow key is held.
    [JsonPropertyName("hold")]
    public uint? Hold { get; init; } = 6;

    /// Pause after `pause`: the menu has to open first, or the arrow lands in the
    /// opening animation and is lost.
    [JsonPropertyName("open_gap")]
    public uint? OpenGap { get; init; } = 20;

    /// Pause between the arrows and the confirmation.
    [JsonPropertyName("gap")]
    public uint? Gap { get; init; } = 10;

    /// Confirmations in a row: the Restart entry, then the dialog's preselected YES.
    [JsonPropertyName("confirms")]
    public uint? Confirms { get; init; } = 2;

    /// Pause between confirmations — the dialog has to appear.
    [JsonPropertyName("confirm_gap")]
    public uint? ConfirmGap { get; init; } = 25;

    /// Frames after the last confirmation.
    [JsonPropertyName("tail")]
    public uint? Tail { get; init; } = 15;
}
