using System.Text.Json;
using System.Text.Json.Serialization;

/// The API JSON of a script — the body of the mod's `POST /script/run`
/// (`docs/API.md` §4).
///
/// Serialization goes through a source-generated context on purpose: the app is
/// published with NativeAOT, where reflection-based `JsonSerializer` does not
/// work. Unknown keys are refused by the records themselves
/// (`JsonUnmappedMemberHandling.Disallow`), which mirrors the mod's
/// `deny_unknown_fields`: a typo like `light_attackk` must not pass silently here
/// either.
internal static class ScriptJson
{
    /// Mirrors `MAX_SCRIPT_FRAMES` in `src/api.rs` — 60 s at 60 FPS.
    internal const uint MaxFrames = 3600;

    /// Mirrors the mod's `name too long (max 64)` check.
    internal const int MaxNameLength = 64;

    internal static ScriptDocument Read(string json)
    {
        var document = JsonSerializer.Deserialize(json, ScriptJsonContext.Default.ScriptDocument)
            ?? throw new ScriptFormatException("script: the JSON body is null");

        // The source-generated deserializer writes `default` over every property the
        // JSON leaves out instead of leaving the initializer alone (measured on
        // `name`), so the mod's own fallbacks are restored here rather than trusted
        // to the declarations.
        document = document with
        {
            Name = document.Name ?? ScriptDocument.DefaultName,
            Commands = document.Commands ?? [],
        };

        Validate(document);
        return document;
    }

    internal static string Write(ScriptDocument document)
    {
        Validate(document);
        return JsonSerializer.Serialize(document, ScriptJsonContext.Default.ScriptDocument);
    }

    /// The checks the mod runs after serde (`parse_script` in `src/api.rs`): types
    /// and unknown keys are covered by deserialization, everything here is
    /// cross-field. The editor refuses a script the game would answer `400` on
    /// instead of storing it.
    internal static void Validate(ScriptDocument document)
    {
        if (document.Name.Length > MaxNameLength)
        {
            throw new ScriptFormatException($"name is longer than {MaxNameLength} characters");
        }

        if (document.Commands.Count == 0)
        {
            throw new ScriptFormatException("commands is empty");
        }

        if (document.Trigger is { Pos: null, Ticks: null })
        {
            throw new ScriptFormatException("trigger needs pos or ticks");
        }

        if (document.Trigger?.Pos is { } position)
        {
            ExpectCount("trigger.pos", position, 3);
        }

        for (var index = 0; index < document.Commands.Count; index++)
        {
            var command = document.Commands[index];
            var where = $"commands[{index}]";

            if (command.Input is null)
            {
                throw new ScriptFormatException($"{where}: input is missing");
            }

            if (command.Duration == 0)
            {
                throw new ScriptFormatException($"{where}: duration must be >= 1");
            }

            if (command.T > MaxFrames || command.Duration > MaxFrames)
            {
                throw new ScriptFormatException($"{where}: t/duration exceeds max {MaxFrames}");
            }

            if (command.T + command.Duration > MaxFrames)
            {
                throw new ScriptFormatException($"{where}: t+duration exceeds max {MaxFrames}");
            }

            if (command.Input.IsEmpty)
            {
                throw new ScriptFormatException($"{where}: input is empty");
            }

            if (command.Input.Camera is { } camera)
            {
                ExpectCount($"{where}: camera", camera, 2);
            }

            if (command.Input.LeftStick is { } stick)
            {
                ExpectCount($"{where}: left_stick", stick, 2);
            }
        }
    }

    /// The mod requires exactly this many numbers (`camera — ровно 2 числа`,
    /// `trigger.pos` — three).
    static void ExpectCount(string field, float[] values, int expected)
    {
        if (values.Length != expected)
        {
            throw new ScriptFormatException($"{field} needs exactly {expected} numbers, got {values.Length}");
        }
    }
}

/// Source-generated serialization for the script document — the only way to
/// serialize under NativeAOT.
///
/// Everything that is *unset* stays out of the JSON: an unset null field (a stick,
/// a raw code, a condition bound, a restart parameter) and a `false` boolean both
/// mean the same to the mod as an absent key, and writing them would bury the one
/// line that matters under 26 lines of `false`. `ScriptCommand.T` and `Duration`
/// opt out — zero is a real frame number.
///
/// `NewLine` is pinned to `\n` (from .NET 9 the writer defaults to the platform's
/// newline) so a golden file does not depend on who checked it out.
[JsonSourceGenerationOptions(
    WriteIndented = true,
    NewLine = "\n",
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingDefault)]
[JsonSerializable(typeof(ScriptDocument))]
internal sealed partial class ScriptJsonContext : JsonSerializerContext;
