namespace TasEditorCs.Tests;

/// Reading Steam's library list.
///
/// The parser is the one part of the install path that reads somebody else's file format, and it has
/// to survive the format's age: `libraryfolders.vdf` has been a list of paths and is now a map of
/// objects, and both spellings are still in the wild. The paths themselves are the interesting part
/// — VDF escapes a backslash, and Windows paths are nothing but backslashes.
///
/// ⚠️ `ParseLibraryPaths` filters by `Directory.Exists` — a library on an unplugged drive is dropped
/// — so a test asserting on a path has to use one that is really there. These tests pass
/// `Path.GetTempPath()`, which is, rather than a plausible-looking `D:\Games` that a build machine
/// may not have.
public class SteamLibraryTests
{
    [Fact]
    public void The_modern_object_shape_names_its_libraries()
    {
        var library = Path.GetTempPath();
        var escaped = library.Replace(@"\", @"\\");

        var vdf = $$"""
            "libraryfolders"
            {
                "0"
                {
                    "path"		"C:\\Program Files (x86)\\Steam"
                    "label"		""
                    "apps"
                    {
                        "235460"		"123"
                    }
                }
                "1"
                {
                    "path"		"{{escaped}}"
                    "label"		""
                    "apps"
                    {
                        "235460"		"456"
                    }
                }
            }
            """;

        Assert.Contains(library, SteamLibrary.ParseLibraryPaths(vdf));
    }

    [Fact]
    public void The_older_list_shape_still_names_its_libraries()
    {
        // The format before the objects: a numbered key and a bare path. A file that old is a file
        // a user still has if Steam has not rewritten it — and one Steam no longer writes.
        var library = Path.GetTempPath();
        var escaped = library.Replace(@"\", @"\\");

        var vdf = $$"""
            "LibraryFolders"
            {
                "TimeNextStatsReport"	"1234567890"
                "ContentStatsID"		"-1234567890123456789"
                "1"		"{{escaped}}"
            }
            """;

        Assert.Contains(library, SteamLibrary.ParseLibraryPaths(vdf));
    }

    [Fact]
    public void Timestamps_and_ids_are_not_mistaken_for_paths()
    {
        // This is the trap the two patterns exist for: `"TimeNextStatsReport" "1234567890"` is a
        // `"key" "value"` pair like any other, and the old list format's keys are *numbers*. Only a
        // numeric key counts, and a numeric value under a named key must not.
        var vdf = """
            "LibraryFolders"
            {
                "TimeNextStatsReport"	"1234567890"
                "ContentStatsID"		"1"
            }
            """;

        Assert.Empty(SteamLibrary.ParseLibraryPaths(vdf));
    }

    [Fact]
    public void A_library_that_is_not_there_is_dropped()
    {
        // A path in the file is not a folder on the disk: a drive that is unplugged, a library that
        // was deleted, a path edited by hand. Handing one on would make the search look in a folder
        // that cannot hold the game and then report "not installed".
        var vdf = """
            "libraryfolders"
            {
                "0"
                {
                    "path"		"Z:\\nowhere\\at\\all"
                }
            }
            """;

        Assert.Empty(SteamLibrary.ParseLibraryPaths(vdf));
    }

    [Fact]
    public void A_path_is_unescaped_on_the_way_out()
    {
        // VDF writes a backslash as `\\`. Left that way the path is a folder that does not exist —
        // and would be silently dropped rather than reported, which is the failure that hides.
        var library = Path.GetTempPath().TrimEnd('\\');
        var escaped = library.Replace(@"\", @"\\") + @"\\";

        var vdf = "\"libraryfolders\"\n{\n    \"0\"\n    {\n        \"path\"\t\t\"" + escaped + "\"\n    }\n}\n";

        Assert.Contains(library + @"\", SteamLibrary.ParseLibraryPaths(vdf));
    }

    [Fact]
    public void One_library_named_twice_is_kept_once()
    {
        var library = Path.GetTempPath();
        var escaped = library.Replace(@"\", @"\\");

        var vdf = $$"""
            "libraryfolders"
            {
                "0"
                {
                    "path"		"{{escaped}}"
                }
                "1"
                {
                    "path"		"{{escaped}}"
                }
            }
            """;

        Assert.Single(SteamLibrary.ParseLibraryPaths(vdf));
    }

    [Fact]
    public void Garbage_is_no_libraries_rather_than_a_crash()
    {
        // The file is on somebody else's disk, in somebody else's format, and may be truncated
        // halfway through a write. An answer of "no extra libraries" costs the search one library;
        // an exception costs the window.
        Assert.Empty(SteamLibrary.ParseLibraryPaths(string.Empty));
        Assert.Empty(SteamLibrary.ParseLibraryPaths("\0\0\0 not a vdf at all"));
        Assert.Empty(SteamLibrary.ParseLibraryPaths("\"libraryfolders\" { \"0\" { \"path\""));
    }

    [Fact]
    public void A_folder_counts_as_the_game_only_with_its_exe()
    {
        var folder = Path.Combine(Path.GetTempPath(), $"tas-editor-game-{Guid.NewGuid():N}");
        Directory.CreateDirectory(folder);
        try
        {
            // The folder name is the game's, but nothing is in it: a library entry can outlive the
            // installation, and the name is not evidence.
            Assert.False(SteamLibrary.IsGameFolder(folder));
            Assert.False(SteamLibrary.IsGameFolder(null));
            Assert.False(SteamLibrary.IsGameFolder(string.Empty));

            File.WriteAllText(Path.Combine(folder, SteamLibrary.GameExeName), "stub");
            Assert.True(SteamLibrary.IsGameFolder(folder));
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }
}
