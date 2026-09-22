using System;
using System.Text.Json;
using System.Text.Json.Serialization;

/// The JSON of the mod's HTTP API (`docs/API.md` in the sibling Rust repo) — the shapes the editor
/// reads and the request bodies it sends.
///
/// Serialization goes through a source-generated context for the same reason the script document
/// does (`Script/ScriptJson.cs`): the app is published with NativeAOT, where reflection-based
/// `JsonSerializer` does not work. Unlike the script JSON, unknown keys are *ignored* here — the
/// mod's `/state` carries a player, a camera and a frame ring the editor has no use for, and a new
/// field on the mod's side must not break the editor.
///
/// Request bodies leave unset fields out (`WhenWritingNull`): the mod treats an absent key as its
/// own default, and writing `"ms": null` would be a field it has to guess about.
internal static class ApiJson
{
    /// `POST /dt` — the fixed tick (`docs/API.md` §3.8). `ticks` rides along only when the tick is
    /// being turned on: with `fixed: false` the mod restores its own measured delta either way.
    internal static string Dt(bool on) =>
        Write(new DtRequest { Fixed = on, Ticks = on ? true : null });

    /// `POST /fps` — `"game"` is the cap the game itself keeps, `"off"` lifts it (`docs/API.md`
    /// §3.13). What the editor sends for its own limit is <see cref="Fps"/> instead.
    internal static string Cap(string cap) => Write(new FpsRequest { Cap = cap });

    internal static string Fps(uint fps) => Write(new FpsRequest { Fps = fps });

    /// `POST /rng` — `"freeze"` with a seed is the reproducible pin (`docs/API.md` §3.10);
    /// `"off"` hands the decision back to the game.
    internal static string Seed(string pin, uint? seed = null) =>
        Write(new RngRequest { Pin = pin, Seed = pin == "freeze" ? seed : null });

    /// `POST /render` — a headless run, and the way back to a rendered frame.
    internal static string Headless(bool on) => Write(new RenderRequest { Headless = on });

    internal static string Reset() => Write(new RenderRequest { Reset = true });

    /// `GET /state` — the snapshot the editor's status line is built from.
    internal static StateResponse? ReadState(string payload) =>
        JsonSerializer.Deserialize(payload, ApiJsonContext.Default.StateResponse);

    /// `POST /script/run`.
    internal static RunResponse? ReadRun(string payload) =>
        JsonSerializer.Deserialize(payload, ApiJsonContext.Default.RunResponse);

    /// `{ "error": "..." }` — the mod's own wording for a refusal, which already names the field or
    /// the reason, so the editor paints it rather than inventing one. Null when the body is not an
    /// error at all (or is not JSON) — the caller falls back to the HTTP code.
    internal static string? ReadError(string payload)
    {
        try
        {
            return JsonSerializer.Deserialize(payload, ApiJsonContext.Default.ErrorResponse)?.Error;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    static string Write<T>(T request) =>
        JsonSerializer.Serialize(request, typeof(T), ApiJsonContext.Default);

    /// `GET /state` — the whole snapshot. Only the fields the editor paints are modelled; everything
    /// else the mod sends is ignored.
    internal sealed record StateResponse
    {
        [JsonPropertyName("mission_id")] public int MissionId { get; init; }

        [JsonPropertyName("mission_name")] public string? MissionName { get; init; }

        [JsonPropertyName("menu_status")] public string? MenuStatus { get; init; }

        [JsonPropertyName("fps")] public float Fps { get; init; }

        [JsonPropertyName("script")] public ScriptResponse? Script { get; init; }

        [JsonPropertyName("dt")] public DtSnapshot? Dt { get; init; }

        [JsonPropertyName("rng_pin")] public string? RngPin { get; init; }

        [JsonPropertyName("rng_seed")] public uint RngSeed { get; init; }

        [JsonPropertyName("fps_cap")] public FpsCapSnapshot? FpsCap { get; init; }

        [JsonPropertyName("render")] public RenderSnapshot? Render { get; init; }
    }

    /// The script the mod is about: the running one, or the last one it ran. `status` is a word, not
    /// a number (`ScriptStatus` is `#[serde(rename_all = "lowercase")]`), so it is read as a string
    /// and mapped — a value the editor does not know must not make the whole snapshot unreadable.
    internal sealed record ScriptResponse
    {
        [JsonPropertyName("id")] public uint Id { get; init; }

        [JsonPropertyName("name")] public string? Name { get; init; }

        [JsonPropertyName("status")] public string? Status { get; init; }

        [JsonPropertyName("frame")] public uint Frame { get; init; }

        [JsonPropertyName("total_frames")] public uint TotalFrames { get; init; }
    }

    internal sealed record DtSnapshot
    {
        [JsonPropertyName("fixed")] public bool Fixed { get; init; }

        [JsonPropertyName("fixed_ms")] public float FixedMs { get; init; }

        [JsonPropertyName("frame_ms")] public float FrameMs { get; init; }
    }

    internal sealed record FpsCapSnapshot
    {
        [JsonPropertyName("cap")] public string? Cap { get; init; }

        /// Null means there is no cap — a real state, not a missing field.
        [JsonPropertyName("limit")] public uint? Limit { get; init; }
    }

    internal sealed record RenderSnapshot
    {
        [JsonPropertyName("headless")] public bool Headless { get; init; }

        [JsonPropertyName("hold")] public bool Hold { get; init; }
    }

    /// `POST /script/run` — what the mod answered with: the id the status is read by, and whether it
    /// is already running or armed on its trigger.
    internal sealed record RunResponse
    {
        [JsonPropertyName("script_id")] public uint ScriptId { get; init; }

        [JsonPropertyName("name")] public string? Name { get; init; }

        [JsonPropertyName("total_frames")] public uint TotalFrames { get; init; }

        [JsonPropertyName("status")] public string? Status { get; init; }
    }

    internal sealed record ErrorResponse
    {
        [JsonPropertyName("error")] public string? Error { get; init; }
    }

    internal sealed record DtRequest
    {
        [JsonPropertyName("fixed")] public bool Fixed { get; init; }

        [JsonPropertyName("ms")] public float? Ms { get; init; }

        [JsonPropertyName("ticks")] public bool? Ticks { get; init; }
    }

    internal sealed record FpsRequest
    {
        [JsonPropertyName("cap")] public string? Cap { get; init; }

        [JsonPropertyName("fps")] public uint? Fps { get; init; }
    }

    internal sealed record RngRequest
    {
        [JsonPropertyName("pin")] public string Pin { get; init; } = "off";

        [JsonPropertyName("seed")] public uint? Seed { get; init; }
    }

    internal sealed record RenderRequest
    {
        [JsonPropertyName("headless")] public bool? Headless { get; init; }

        [JsonPropertyName("hold")] public bool? Hold { get; init; }

        [JsonPropertyName("reset")] public bool? Reset { get; init; }
    }
}

/// Source-generated serialization for the API shapes — the only way to serialize under NativeAOT.
[JsonSourceGenerationOptions(
    WriteIndented = false,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(ApiJson.StateResponse))]
[JsonSerializable(typeof(ApiJson.RunResponse))]
[JsonSerializable(typeof(ApiJson.ErrorResponse))]
[JsonSerializable(typeof(ApiJson.DtRequest))]
[JsonSerializable(typeof(ApiJson.FpsRequest))]
[JsonSerializable(typeof(ApiJson.RngRequest))]
[JsonSerializable(typeof(ApiJson.RenderRequest))]
internal sealed partial class ApiJsonContext : JsonSerializerContext;
