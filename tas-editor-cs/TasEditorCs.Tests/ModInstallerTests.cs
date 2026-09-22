using System.IO;

namespace TasEditorCs.Tests;

/// The installer, against a real folder.
///
/// The thing under test *is* the file system — what "installed" means is a byte comparison of what
/// the game folder holds, and the answer changes when a file does — so these tests write into a
/// temporary folder rather than mocking anything. Nothing here is WinRT, so the headless host runs
/// them as they are.
///
/// ⚠️ `SteamLibrary.IsGameFolder` is the gate on every operation: a folder without the game's exe in
/// it is refused, so each test's folder has to look like an installation. That is what `GameFolder()`
/// below is for.
public class ModInstallerTests : IDisposable
{
    readonly string _folder = Path.Combine(
        Path.GetTempPath(), $"tas-editor-mod-{Guid.NewGuid():N}");

    public ModInstallerTests()
    {
        Directory.CreateDirectory(_folder);
        // The one file that decides whether a folder counts as the game.
        File.WriteAllText(Path.Combine(_folder, SteamLibrary.GameExeName), "stub");
    }

    public void Dispose() => Directory.Delete(_folder, recursive: true);

    /// A payload that is *not* the shipped one: these tests are about the installer's logic, and two
    /// small arrays make "same bytes" and "different bytes" readable instead of being a property of
    /// whatever the build happened to embed.
    static ModPayload Payload(byte[]? asi = null, byte[]? loader = null) =>
        new(asi ?? [1, 2, 3, 4], loader ?? [9, 8, 7]);

    string Plugin => ModInstaller.PluginPath(_folder);

    string LoaderFile => Path.Combine(_folder, ModInstaller.LoaderPath);

    [Fact]
    public void An_empty_game_folder_reads_as_not_installed()
    {
        Assert.Equal(ModState.NotInstalled, ModInstaller.Detect(_folder, Payload()));
    }

    [Fact]
    public void No_folder_at_all_reads_as_no_game_folder()
    {
        Assert.Equal(ModState.NoGameFolder, ModInstaller.Detect(null, Payload()));
        Assert.Equal(ModState.NoGameFolder, ModInstaller.Detect(Path.Combine(_folder, "gone"), Payload()));
        // A folder that exists but holds no game exe is the same answer: the check is the exe, not
        // the path, so a user who picks the wrong folder is refused here rather than at the write.
        Assert.Equal(ModState.NoGameFolder, ModInstaller.Detect(Path.GetTempPath(), Payload()));
    }

    [Fact]
    public void Installing_puts_the_plugin_under_plugins_and_the_loader_in_the_root()
    {
        var payload = Payload();

        var result = ModInstaller.Install(_folder, payload);

        Assert.True(result.Ok, result.Message);
        Assert.Equal(payload.Asi, File.ReadAllBytes(Plugin));
        Assert.Equal(payload.Loader, File.ReadAllBytes(LoaderFile));
        // The layout the mod's own readme describes: an ASI loader scans `plugins\` beside itself.
        Assert.StartsWith(_folder, Plugin);
        Assert.Contains(ModInstaller.PluginDir, Plugin);
        // What the user has to do next, in the message they get.
        Assert.Contains("Restart the game", result.Message);
    }

    [Fact]
    public void After_installing_the_folder_reads_as_installed()
    {
        var payload = Payload();
        ModInstaller.Install(_folder, payload);

        Assert.Equal(ModState.Installed, ModInstaller.Detect(_folder, payload));
    }

    [Fact]
    public void A_plugin_of_another_build_reads_as_a_different_version()
    {
        var installed = Payload(asi: [1, 2, 3, 4]);
        ModInstaller.Install(_folder, installed);

        // Same length, different bytes: the comparison is content, not size, and not a timestamp.
        var other = Payload(asi: [1, 2, 3, 5]);
        Assert.Equal(ModState.OtherVersion, ModInstaller.Detect(_folder, other));

        // A different length is the same answer — and the likely one in practice, since builds differ.
        var longer = Payload(asi: [1, 2, 3, 4, 5]);
        Assert.Equal(ModState.OtherVersion, ModInstaller.Detect(_folder, longer));
    }

    [Fact]
    public void Installing_over_another_build_replaces_the_plugin()
    {
        var other = Payload(asi: [1, 2, 3, 5]);
        ModInstaller.Install(_folder, other);

        var mine = Payload();
        var result = ModInstaller.Install(_folder, mine);

        Assert.True(result.Ok, result.Message);
        Assert.Equal(mine.Asi, File.ReadAllBytes(Plugin));
        Assert.Equal(ModState.Installed, ModInstaller.Detect(_folder, mine));
    }

    [Fact]
    public void The_loader_is_added_when_the_folder_has_none()
    {
        var payload = Payload();

        var result = ModInstaller.Install(_folder, payload);

        Assert.True(result.Ok, result.Message);
        Assert.True(File.Exists(LoaderFile));
        // The message says what it did, so the user can tell an added loader from a kept one.
        Assert.Contains("added d3d9.dll", result.Message);
    }

    [Fact]
    public void A_foreign_loader_is_kept_untouched()
    {
        // Almost every machine with a ReShade, an ENB or another ASI mod has one of these. It is not
        // ours to replace: the plugin works with any ASI loader, so the install is complete without
        // overwriting somebody else's file.
        var theirs = new byte[] { 0xAA, 0xBB, 0xCC };
        File.WriteAllBytes(LoaderFile, theirs);

        var result = ModInstaller.Install(_folder, Payload());

        Assert.True(result.Ok, result.Message);
        Assert.Equal(theirs, File.ReadAllBytes(LoaderFile));
        Assert.Contains("kept the existing d3d9.dll", result.Message);
        // And the reader is pointed at the upstream loader, because that file may not be a loader at
        // all — a `d3d9.dll` from a wrapper is not guaranteed to scan `plugins\`.
        Assert.Contains(ModInstaller.LoaderUrl, result.Message);
    }

    [Fact]
    public void Our_own_loader_is_not_reported_as_a_foreign_one()
    {
        var payload = Payload();
        ModInstaller.Install(_folder, payload);

        var (present, ours) = ModInstaller.Loader(_folder, payload);

        Assert.True(present);
        Assert.True(ours);
        // No URL and no "kept the existing" in the second install's message: there was nothing to keep.
        var second = ModInstaller.Install(_folder, payload);
        Assert.DoesNotContain("kept", second.Message);
        Assert.DoesNotContain(ModInstaller.LoaderUrl, second.Message);
    }

    [Fact]
    public void A_loader_of_a_different_build_is_not_ours()
    {
        File.WriteAllBytes(LoaderFile, [1, 2, 3]);

        var (present, ours) = ModInstaller.Loader(_folder, Payload(loader: [4, 5, 6]));

        Assert.True(present);
        Assert.False(ours);
    }

    [Fact]
    public void Installing_into_a_folder_that_is_not_the_game_is_refused()
    {
        var notTheGame = Path.Combine(_folder, "elsewhere");
        Directory.CreateDirectory(notTheGame);

        var result = ModInstaller.Install(notTheGame, Payload());

        Assert.False(result.Ok);
        Assert.Contains(SteamLibrary.GameExeName, result.Message);
        // Nothing was written anywhere: a refused install leaves no half-state behind.
        Assert.False(Directory.Exists(Path.Combine(notTheGame, ModInstaller.PluginDir)));
    }

    [Fact]
    public void Installing_with_no_folder_at_all_is_refused()
    {
        var result = ModInstaller.Install(null, Payload());

        Assert.False(result.Ok);
        Assert.Contains("No game folder", result.Message);
    }

    [Fact]
    public void Installing_twice_leaves_no_temporary_behind()
    {
        // The writes go through a `.tmp` beside the target and a move over it, so an interrupted
        // write cannot leave a truncated plugin. A clean run must not leave the temporary either.
        ModInstaller.Install(_folder, Payload());

        Assert.Empty(Directory.GetFiles(_folder, "*.tmp"));
        Assert.Empty(Directory.GetFiles(Path.Combine(_folder, ModInstaller.PluginDir), "*.tmp"));
    }

    [Fact]
    public void An_unwritable_game_folder_comes_back_as_a_message()
    {
        // A directory where the plugin file is expected: `File.Move` onto it refuses, and the answer
        // has to reach the pane rather than the top of the stack — the caller is a click handler.
        var plugin = Plugin;
        Directory.CreateDirectory(Path.GetDirectoryName(plugin)!);
        Directory.CreateDirectory(plugin);

        var result = ModInstaller.Install(_folder, Payload());

        Assert.False(result.Ok);
        Assert.StartsWith($"Cannot write {plugin}:", result.Message);
    }

    [Fact]
    public void Removing_takes_both_files_out()
    {
        var payload = Payload();
        ModInstaller.Install(_folder, payload);

        var result = ModInstaller.Remove(_folder, payload);

        Assert.True(result.Ok, result.Message);
        Assert.False(File.Exists(Plugin));
        Assert.False(File.Exists(LoaderFile));
        // The folder itself stays: other mods live in it, and a loader does not care whether the
        // directory outlives its contents.
        Assert.True(Directory.Exists(Path.Combine(_folder, ModInstaller.PluginDir)));
        Assert.Equal(ModState.NotInstalled, ModInstaller.Detect(_folder, payload));
    }

    [Fact]
    public void Removing_keeps_a_foreign_loader()
    {
        // The mirror of the install rule: a loader that is not ours belongs to something else, and
        // taking the mod out must not take that with it.
        var theirs = new byte[] { 0xAA, 0xBB, 0xCC };
        File.WriteAllBytes(LoaderFile, theirs);
        ModInstaller.Install(_folder, Payload());

        var result = ModInstaller.Remove(_folder, Payload());

        Assert.True(result.Ok, result.Message);
        Assert.False(File.Exists(Plugin));
        Assert.Equal(theirs, File.ReadAllBytes(LoaderFile));
        Assert.Contains("kept d3d9.dll", result.Message);
    }

    [Fact]
    public void Removing_when_nothing_is_installed_says_so_without_failing()
    {
        // Not an error: the folder is a game folder and stays usable, there is simply nothing of ours
        // in it. The pane disables the button for this case anyway; this is the belt to its braces.
        var result = ModInstaller.Remove(_folder, Payload());

        Assert.True(result.Ok, result.Message);
        Assert.Contains("Nothing to remove", result.Message);
    }

    [Fact]
    public void Removing_a_plugin_that_is_not_ours_still_takes_it_out()
    {
        // A different build of the mod is still this mod — `Detect` says `OtherVersion`, not "somebody
        // else's", and the uninstall is exactly how a user replaces it by hand.
        File.WriteAllBytes(LoaderFile, [1, 2, 3]);
        ModInstaller.Install(_folder, Payload(loader: [1, 2, 3], asi: [9, 9, 9]));

        var result = ModInstaller.Remove(_folder, Payload(asi: [1, 2, 3], loader: [4, 5, 6]));

        Assert.True(result.Ok, result.Message);
        Assert.False(File.Exists(Plugin));
        // The loader that is there is not the one this build carries, so it is left alone.
        Assert.True(File.Exists(LoaderFile));
    }

    [Fact]
    public void Removing_from_a_folder_that_is_not_the_game_is_refused()
    {
        var notTheGame = Path.Combine(_folder, "elsewhere");
        Directory.CreateDirectory(notTheGame);

        var result = ModInstaller.Remove(notTheGame, Payload());

        Assert.False(result.Ok);
        Assert.Contains(SteamLibrary.GameExeName, result.Message);
    }

    [Fact]
    public void There_is_something_to_remove_exactly_when_we_put_something_there()
    {
        var payload = Payload();

        // Nothing installed, nothing of ours to take out.
        Assert.False(ModInstaller.CanRemove(_folder, payload));

        // A foreign loader alone does not count: it is not ours to remove.
        File.WriteAllBytes(LoaderFile, [0xAA, 0xBB]);
        Assert.False(ModInstaller.CanRemove(_folder, payload));

        // Our plugin does, and so does our loader on its own (a user who deleted the plugin by hand).
        ModInstaller.Install(_folder, payload);
        Assert.True(ModInstaller.CanRemove(_folder, payload));
    }

    [Fact]
    public void A_folder_that_is_not_the_game_has_nothing_to_remove()
    {
        Assert.False(ModInstaller.CanRemove(null, Payload()));
        Assert.False(ModInstaller.CanRemove(Path.Combine(_folder, "gone"), Payload()));
    }
}
