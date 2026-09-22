using System.IO;

namespace TasEditorCs.Tests;

/// The on-disk workspace. These tests write real files into a temporary folder: the thing under
/// test *is* the file system (what counts as a script, what a list row reads out of a file, what
/// happens when a name is taken), so there is nothing to fake here — and nothing WinRT, so the
/// headless host runs them as they are.
public class WorkspaceTests : IDisposable
{
    readonly string _folder = Path.Combine(
        Path.GetTempPath(), $"tas-editor-workspace-{Guid.NewGuid():N}");

    public WorkspaceTests() => Directory.CreateDirectory(_folder);

    public void Dispose() => Directory.Delete(_folder, recursive: true);

    /// A file that reads as a script: the mod refuses an empty command list and a trigger with
    /// neither pos nor ticks, so "valid" takes a little more than a rules line (`ScriptJson.Validate`).
    const string Valid = "! name=probe trig=ticks:0\n0 a:3\n";

    [Fact]
    public void Lists_only_tas_files_by_name()
    {
        File.WriteAllText(Path.Combine(_folder, "b.tas"), Valid);
        File.WriteAllText(Path.Combine(_folder, "a.tas"), Valid);
        File.WriteAllText(Path.Combine(_folder, "notes.txt"), "a tas script is a .tas file");
        File.WriteAllText(Path.Combine(_folder, "script.json"), "{}");

        var names = Workspace.List(_folder).Scripts.Select(script => script.Name);

        Assert.Equal(new[] { "a", "b" }, names);
    }

    [Fact]
    public void A_script_carries_the_file_its_text_and_the_frame_it_ends_on()
    {
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);

        var script = Assert.Single(Workspace.List(_folder).Scripts);

        Assert.Equal(path, script.Path);
        Assert.Equal("probe", script.Name);
        Assert.Equal(Valid, script.Text);
        // `0 a:3` is three frames wide, so the last frame is 3 — the list row's own number.
        Assert.Equal(3u, script.Frames);
        Assert.True(script.Reads);
    }

    [Fact]
    public void A_file_that_does_not_read_as_a_script_is_listed_with_its_reason()
    {
        File.WriteAllText(Path.Combine(_folder, "broken.tas"), "0 zz\n");

        var script = Assert.Single(Workspace.List(_folder).Scripts);

        // Listed, not dropped: a typo in a file is worth seeing in its row rather than as a file
        // that is mysteriously not in the workspace.
        Assert.False(script.Reads);
        Assert.Equal(0u, script.Frames);
        Assert.StartsWith("line 1: unknown token 'zz'", script.Error);
    }

    [Fact]
    public void No_folder_is_an_empty_workspace_and_a_missing_one_says_so()
    {
        Assert.Empty(Workspace.List(null).Scripts);
        Assert.Null(Workspace.List(null).Error);

        var missing = Workspace.List(Path.Combine(_folder, "gone"));

        Assert.Empty(missing.Scripts);
        Assert.StartsWith("The workspace folder is gone:", missing.Error);
    }

    [Fact]
    public void A_new_file_is_empty_and_does_not_overwrite_a_taken_name()
    {
        var (first, firstError) = Workspace.Create(_folder);
        var (second, secondError) = Workspace.Create(_folder);

        Assert.Null(firstError);
        Assert.Null(secondError);
        Assert.Equal(Path.Combine(_folder, "script.tas"), first);
        Assert.Equal(Path.Combine(_folder, "script-2.tas"), second);
        // Empty on purpose: the editor opens it on nothing, and the first line is the author's.
        Assert.Equal(string.Empty, File.ReadAllText(second!));
    }

    [Fact]
    public void A_copy_sits_beside_its_original_under_the_same_text()
    {
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);

        var (first, firstError) = Workspace.Duplicate(path);
        var (second, _) = Workspace.Duplicate(path);

        Assert.Null(firstError);
        Assert.Equal(Path.Combine(_folder, "probe-copy.tas"), first);
        Assert.Equal(Path.Combine(_folder, "probe-copy-2.tas"), second);
        // A copy of the file, not a rewrite of the document: the two are the same thing here.
        Assert.Equal(Valid, File.ReadAllText(first!));
    }

    [Fact]
    public void Writing_a_text_replaces_the_whole_file()
    {
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);

        Assert.Null(Workspace.Write(path, "! trig=ticks:5\n5 x\n"));

        // The DSL reads the first character of the first line, so the file must not start with a
        // byte-order mark — a mark would turn the rules line into a frame.
        Assert.Equal("! trig=ticks:5\n5 x\n", File.ReadAllText(path));
        Assert.Equal(6u, Workspace.List(_folder).Scripts.Single().Frames);
    }

    [Fact]
    public void A_text_written_with_any_line_break_lands_in_the_format_its_own()
    {
        // Measured: a WinUI `TextBox` separates its lines with a lone `\r` and no `\n` at all, so a
        // save that wrote the control's text through left a file the parser read as one long line —
        // the whole script became a single frame line. Whatever the caller's text uses, the file
        // gets the format's separator, and still reads as the script it was.
        var path = Path.Combine(_folder, "probe.tas");
        var typed = "! trig=ticks:0\r0 a:3\r10 x:2\r";

        Assert.Null(Workspace.Write(path, typed));

        Assert.Equal("! trig=ticks:0\n0 a:3\n10 x:2\n", File.ReadAllText(path));
        var script = Assert.Single(Workspace.List(_folder).Scripts);
        Assert.True(script.Reads);
        Assert.Equal(12u, script.Frames);
    }

    [Fact]
    public void A_file_separated_some_other_way_still_reads_as_its_script()
    {
        // Files are edited in more places than this editor, and `\r\n` is what Windows puts in one.
        // The separator is not part of the format: a line break is a line break.
        foreach (var separator in new[] { "\r\n", "\r" })
        {
            var name = separator == "\r\n" ? "crlf.tas" : "cr.tas";
            File.WriteAllText(Path.Combine(_folder, name), Valid.Replace("\n", separator));
        }

        var scripts = Workspace.List(_folder).Scripts;

        Assert.Equal(2, scripts.Count);
        Assert.All(scripts, script =>
        {
            Assert.True(script.Reads);
            Assert.Equal(3u, script.Frames);
            Assert.Equal(Valid, script.Text);
        });
    }

    [Fact]
    public void Deleting_takes_the_file_out_of_the_listing()
    {
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);

        Assert.Null(Workspace.Delete(path));
        Assert.Empty(Workspace.List(_folder).Scripts);
    }

    [Fact]
    public void A_file_it_cannot_delete_comes_back_as_a_message()
    {
        // A folder where a file is expected: `File.Delete` refuses it, and the message has to
        // reach the pane rather than the top of the stack — the caller is a click handler.
        var path = Path.Combine(_folder, "not-a-file.tas");
        Directory.CreateDirectory(path);

        var error = Workspace.Delete(path);

        Assert.NotNull(error);
        Assert.StartsWith($"Cannot delete {path}:", error);
    }

    [Fact]
    public void A_rename_moves_the_file_and_keeps_its_text()
    {
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);

        // What a user types is a file name, not a path: the whitespace is a slip, and the extension
        // belongs to the workspace.
        var (renamed, error) = Workspace.Rename(path, "  barrier-flight  ");

        Assert.Null(error);
        Assert.Equal(Path.Combine(_folder, "barrier-flight.tas"), renamed);
        Assert.False(File.Exists(path));
        Assert.Equal(Valid, File.ReadAllText(renamed!));
        Assert.Equal("barrier-flight", Assert.Single(Workspace.List(_folder).Scripts).Name);
    }

    [Fact]
    public void A_name_that_is_taken_is_refused_rather_than_suffixed()
    {
        // New and Duplicate invent a free name because they were asked for "a file"; a name that was
        // typed is a name the author wants, and `-2` would hand them something they did not ask for.
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);
        File.WriteAllText(Path.Combine(_folder, "taken.tas"), Valid);

        var (renamed, error) = Workspace.Rename(path, "taken");

        Assert.Null(renamed);
        Assert.Equal("taken.tas is already in this folder", error);
        // Both files are still there: nothing was overwritten and nothing moved behind the author's
        // back.
        Assert.Equal(2, Workspace.List(_folder).Scripts.Count);
    }

    [Theory]
    [InlineData("", "A script needs a file name")]
    [InlineData("probe.tas", "Leave .tas out of the name — the workspace adds it")]
    [InlineData("a/b", "A file name cannot hold '/'")]
    public void A_name_that_would_not_be_a_file_comes_back_as_a_message(string name, string expected)
    {
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);

        var (renamed, error) = Workspace.Rename(path, name);

        Assert.Null(renamed);
        Assert.Equal(expected, error);
        Assert.True(File.Exists(path));
    }

    [Fact]
    public void Renaming_a_file_to_the_name_it_already_has_is_not_an_error()
    {
        var path = Path.Combine(_folder, "probe.tas");
        File.WriteAllText(path, Valid);

        var (renamed, error) = Workspace.Rename(path, "probe");

        Assert.Equal(path, renamed);
        Assert.Null(error);
    }

    [Fact]
    public void The_folder_and_the_run_rules_survive_a_restart()
    {
        var settings = Path.Combine(_folder, "settings");
        var rules = new PlaybackRules(
            FixedTick: false,
            Cap: FpsCapMode.Custom,
            CustomFps: 144,
            PinSeed: true,
            Seed: 0x55555555,
            Headless: true);

        EditorSettings.Save(new EditorSettingsData(_folder, rules, "0x55555555"), settings);
        var loaded = EditorSettings.Load(settings);

        Assert.Equal(_folder, loaded.Folder);
        Assert.Equal(rules, loaded.Playback);
        // The spelling travels with the value: the notes spell seeds in hex (`docs/API.md` §3.10), and
        // a round trip through the settings file must not turn `0x55555555` into a decimal nobody
        // recognizes.
        Assert.Equal("0x55555555", loaded.SeedText);

        EditorSettings.Save(new EditorSettingsData(null, PlaybackRules.Default, "1"), settings);
        Assert.Null(EditorSettings.Load(settings).Folder);
        Assert.Equal(PlaybackRules.Default, EditorSettings.Load(settings).Playback);

        // Nothing remembered, and a file that cannot be read, both answer the same way: an editor
        // that opens on no folder beats one that refuses to start over its own settings.
        Assert.Null(EditorSettings.Load(Path.Combine(_folder, "absent")).Folder);
    }

    [Fact]
    public void A_frame_cap_is_remembered_by_the_panels_own_word()
    {
        var settings = Path.Combine(_folder, "settings");

        foreach (var cap in new[] { FpsCapMode.Default, FpsCapMode.Unlimited, FpsCapMode.Custom })
        {
            EditorSettings.Save(
                new EditorSettingsData(_folder, PlaybackRules.Default with { Cap = cap }, "1"),
                settings);

            Assert.Equal(cap, EditorSettings.Load(settings).Playback.Cap);
        }

        // The mod's own spellings stay readable: the panel used to write them before the two
        // vocabularies were separated, and a settings file is not worth an upgrade migration over a
        // word.
        File.WriteAllText(settings, "cap=off\n");
        Assert.Equal(FpsCapMode.Unlimited, EditorSettings.Load(settings).Playback.Cap);
        File.WriteAllText(settings, "cap=game\n");
        Assert.Equal(FpsCapMode.Default, EditorSettings.Load(settings).Playback.Cap);
    }

    [Fact]
    public void A_settings_file_from_before_the_run_rules_still_names_its_folder()
    {
        // The previous format was one line of path and nothing else — no `=`, no keys. Reading it as
        // a settings file nobody can parse would cost the author their workspace after an upgrade.
        var settings = Path.Combine(_folder, "settings");
        File.WriteAllText(settings, _folder);

        var loaded = EditorSettings.Load(settings);

        Assert.Equal(_folder, loaded.Folder);
        Assert.Equal(PlaybackRules.Default, loaded.Playback);
    }

    [Fact]
    public void A_seed_that_does_not_read_falls_back_to_the_default_rather_than_breaking_the_file()
    {
        var settings = Path.Combine(_folder, "settings");
        File.WriteAllText(settings, "folder=\ndt=0\nseed=not-a-number\n\n");

        var loaded = EditorSettings.Load(settings);

        Assert.Null(loaded.Folder);
        Assert.False(loaded.Playback.FixedTick);
        Assert.Equal(PlaybackRules.Default.Seed, loaded.Playback.Seed);
        // Half a number is still what the field held: it is not the editor's place to rewrite what the
        // author is in the middle of typing.
        Assert.Equal("not-a-number", loaded.SeedText);
    }
}
