using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;

/// What the editor remembers between runs: the folder the workspace is on, and how a run is
/// configured.
///
/// A file of its own rather than `UsePersisted` — that hook is a process-lifetime cache (spec 033
/// §2), so it holds a value across a re-render but not across a restart, and both settings have to
/// survive one. `%LOCALAPPDATA%` is where the Rust sibling keeps its own things.
///
/// The file is plain `key=value` lines, parsed by hand: there is nothing to serialize, so there is
/// nothing for the AOT compiler to miss (the JSON of a script goes through a source-generated
/// context — `Script/ScriptJson.cs` — and a settings record would need one too).
///
/// ⚠️ A file written before the run rules existed is a bare folder path with no `=` in it. It is read
/// as the folder rather than as a broken settings file — the alternative is an editor that opens on
/// no workspace because of its own upgrade.
///
/// The seed travels as the *text* the field held, not as a number: the documented seeds are hex
/// (`0x55555555`, `docs/API.md` §3.10), and writing the parsed decimal would quietly turn the
/// author's spelling into a different-looking one.
internal sealed record EditorSettingsData(string? Folder, PlaybackRules Playback, string SeedText)
{
    internal static readonly EditorSettingsData Empty = new(
        null,
        PlaybackRules.Default,
        PlaybackRules.Default.Seed.ToString(CultureInfo.InvariantCulture));
}

internal static class EditorSettings
{
    /// Where the editor reads its settings from: `%LOCALAPPDATA%\tas-editor-cs\settings`.
    internal static string DefaultPath { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "tas-editor-cs",
        "settings");

    /// The keys the file holds. A line whose key is not one of these is not a settings line at all —
    /// which is how the old, single-line format is recognised (`Load`).
    static readonly string[] Keys = ["folder", "dt", "cap", "cap_fps", "pin_seed", "seed", "headless"];

    /// The remembered settings. A file that cannot be read, is empty, or holds values nobody can
    /// parse, answers with the defaults: an editor that opens on an empty workspace beats one that
    /// refuses to start over its own settings. `null` comes back in the `Folder`, which is also a
    /// legitimate state — no folder was ever picked.
    internal static EditorSettingsData Load(string? path = null)
    {
        string text;
        try
        {
            text = File.ReadAllText(path ?? DefaultPath);
        }
        catch (Exception)
        {
            return EditorSettingsData.Empty;
        }

        var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var line in text.Split('\n'))
        {
            var entry = line.Trim();
            var separator = entry.IndexOf('=');
            if (separator <= 0)
            {
                continue;
            }

            var key = entry[..separator].Trim();
            if (Array.Exists(Keys, known => string.Equals(known, key, StringComparison.OrdinalIgnoreCase)))
            {
                values[key] = entry[(separator + 1)..].Trim();
            }
        }

        if (values.Count == 0)
        {
            // No settings line in the file: the folder as the previous format wrote it — one line of
            // path and nothing else.
            var folder = text.Trim();
            return EditorSettingsData.Empty with { Folder = folder.Length == 0 ? null : folder };
        }

        var defaults = PlaybackRules.Default;
        var seedText = Text(values, "seed") ?? defaults.Seed.ToString(CultureInfo.InvariantCulture);
        return new EditorSettingsData(
            Folder: Text(values, "folder"),
            Playback: new PlaybackRules(
                FixedTick: Flag(values, "dt", defaults.FixedTick),
                Cap: Cap(values, "cap", defaults.Cap),
                CustomFps: Number(values, "cap_fps", defaults.CustomFps),
                PinSeed: Flag(values, "pin_seed", defaults.PinSeed),
                Seed: PlaybackRules.TrySeed(seedText, out var seed) ? seed : defaults.Seed,
                Headless: Flag(values, "headless", defaults.Headless)),
            SeedText: seedText);
    }

    /// Remembers the settings. A write that fails is not worth telling anyone about: it costs the
    /// user one folder pick and one rules edit next launch, and the workspace itself is unaffected.
    internal static void Save(EditorSettingsData settings, string? path = null)
    {
        var file = path ?? DefaultPath;
        try
        {
            var parent = Path.GetDirectoryName(file);
            if (!string.IsNullOrEmpty(parent))
            {
                Directory.CreateDirectory(parent);
            }

            var rules = settings.Playback;
            string[] lines =
            [
                $"folder={settings.Folder ?? string.Empty}",
                $"dt={(rules.FixedTick ? 1 : 0)}",
                $"cap={Spelling(rules.Cap)}",
                $"cap_fps={rules.CustomFps.ToString(CultureInfo.InvariantCulture)}",
                $"pin_seed={(rules.PinSeed ? 1 : 0)}",
                $"seed={settings.SeedText}",
                $"headless={(rules.Headless ? 1 : 0)}",
            ];

            File.WriteAllText(file, string.Join('\n', lines) + "\n");
        }
        catch (Exception)
        {
            // Deliberately silent — see above.
        }
    }

    static string? Text(Dictionary<string, string> values, string key) =>
        values.TryGetValue(key, out var value) && value.Length > 0 ? value : null;

    static bool Flag(Dictionary<string, string> values, string key, bool fallback) =>
        values.TryGetValue(key, out var value) ? value != "0" : fallback;

    static uint Number(Dictionary<string, string> values, string key, uint fallback) =>
        values.TryGetValue(key, out var value)
        && uint.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var number)
            ? number
            : fallback;

    static FpsCapMode Cap(Dictionary<string, string> values, string key, FpsCapMode fallback) =>
        values.TryGetValue(key, out var value)
            ? value.ToLowerInvariant() switch
            {
                // The panel's own words first, the mod's own spellings kept readable for files written
                // while the two used to be the same.
                "default" or "game" => FpsCapMode.Default,
                "unlimited" or "off" or "uncapped" => FpsCapMode.Unlimited,
                "custom" => FpsCapMode.Custom,
                _ => fallback,
            }
            : fallback;

    static string Spelling(FpsCapMode cap) => cap switch
    {
        FpsCapMode.Unlimited => "unlimited",
        FpsCapMode.Custom => "custom",
        _ => "default",
    };
}
