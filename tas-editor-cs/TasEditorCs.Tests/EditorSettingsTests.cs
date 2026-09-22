using System.IO;

namespace TasEditorCs.Tests;

/// What the editor remembers between runs, and what it falls back to when it remembers nothing.
///
/// These tests touch the file system (the settings file *is* a file, and the example folder is
/// decided by whether a directory exists), so they work in a temporary folder. Nothing here is
/// WinRT, so the headless host runs them as they are.
public class EditorSettingsTests : IDisposable
{
    readonly string _folder = Path.Combine(
        Path.GetTempPath(), $"tas-editor-settings-{Guid.NewGuid():N}");

    public EditorSettingsTests() => Directory.CreateDirectory(_folder);

    public void Dispose() => Directory.Delete(_folder, recursive: true);

    [Fact]
    public void A_first_launch_opens_on_the_shipped_examples()
    {
        // What the distribution ships: `examples/` next to the exe. A first launch has no
        // remembered folder, and a workspace with the example scripts in it beats an empty pane.
        var examples = Path.Combine(_folder, "examples");
        Directory.CreateDirectory(examples);

        Assert.Equal(examples, EditorSettings.FirstFolder(_folder));
    }

    [Fact]
    public void No_examples_folder_is_no_workspace_not_an_error()
    {
        // The dev loop builds without copying `examples/`, and so does a publish that was not made
        // by `pack.ps1`. The editor then opens on no workspace and says so — `Open folder…` is the
        // answer, and this is the same state the editor was in before the examples existed.
        Assert.Null(EditorSettings.FirstFolder(_folder));
    }

    [Fact]
    public void A_picked_folder_wins_over_the_examples()
    {
        // The examples are a first-launch default, not a pinned workspace: once a folder is
        // remembered, nothing about `examples/` matters, even when it is still on disk.
        Directory.CreateDirectory(Path.Combine(_folder, "examples"));
        var settingsPath = Path.Combine(_folder, "settings");
        var picked = Path.Combine(_folder, "work");

        EditorSettings.Save(EditorSettingsData.Empty with { Folder = picked }, settingsPath);

        Assert.Equal(picked, EditorSettings.Load(settingsPath).Folder);
    }

    [Fact]
    public void A_missing_settings_file_reads_as_nothing_was_ever_picked()
    {
        var loaded = EditorSettings.Load(Path.Combine(_folder, "settings"));

        Assert.Null(loaded.Folder);
        Assert.Equal(PlaybackRules.Default, loaded.Playback);
        Assert.Equal("1", loaded.SeedText);
    }

    [Fact]
    public void The_rules_survive_a_round_trip()
    {
        // Every rule, written and read back: this file is what makes a run reproduce between
        // launches, so a value the writer spells and the reader cannot parse is a run that
        // silently goes out configured differently.
        var settingsPath = Path.Combine(_folder, "settings");
        var rules = new PlaybackRules(
            FixedTick: false,
            Cap: FpsCapMode.Custom,
            CustomFps: 144,
            PinSeed: false,
            Seed: 0x55555555,
            Headless: true);

        EditorSettings.Save(
            new EditorSettingsData(_folder, rules, "0x55555555"),
            settingsPath);

        var loaded = EditorSettings.Load(settingsPath);

        Assert.Equal(_folder, loaded.Folder);
        Assert.Equal(rules, loaded.Playback);
        // The seed travels as the spelling the field held, so a hex seed stays hex.
        Assert.Equal("0x55555555", loaded.SeedText);
    }

    [Fact]
    public void A_file_from_before_the_rules_existed_reads_as_the_folder()
    {
        // The old format was one line: the folder, and nothing else. It has no `=`, so it is read
        // as a folder rather than as a settings file nobody can parse — an editor that opens on no
        // workspace because of its own upgrade is the failure this avoids.
        var settingsPath = Path.Combine(_folder, "settings");
        File.WriteAllText(settingsPath, _folder + "\n");

        var loaded = EditorSettings.Load(settingsPath);

        Assert.Equal(_folder, loaded.Folder);
        Assert.Equal(PlaybackRules.Default, loaded.Playback);
    }

    [Fact]
    public void A_file_nobody_can_parse_answers_with_the_defaults()
    {
        // Garbage, and a settings line whose values are nonsense: an editor that refuses to start
        // over its own settings file is worse than one that opens with defaults.
        var settingsPath = Path.Combine(_folder, "settings");
        File.WriteAllText(settingsPath, "folder=\ndt=maybe\ncap=nonsense\nseed=zz\n");

        var loaded = EditorSettings.Load(settingsPath);

        Assert.Null(loaded.Folder);
        Assert.Equal(PlaybackRules.Default, loaded.Playback);
    }
}
