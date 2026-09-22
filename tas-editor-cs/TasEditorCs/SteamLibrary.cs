using System;
using System.Collections.Generic;
using System.IO;
using System.Text.RegularExpressions;
using Microsoft.Win32;

/// Where the game is installed, found the way Steam records it.
///
/// A Steam library path is not guessable: `steamapps\common` under the Steam root is only the
/// default, and anyone with a second drive has the game somewhere else entirely. So the search
/// follows Steam's own bookkeeping instead of a hard-coded path —
/// `HKCU\Software\Valve\Steam\SteamPath` names the installation, and each
/// `steamapps\libraryfolders.vdf` names the libraries it knows about (the file has been both a list
/// of paths and a map of objects over the years, so both spellings are read).
///
/// Nothing here throws at its caller: a missing key, an unreadable registry hive and a vdf nobody
/// can parse all end the same way — an empty answer, and the pane offers the folder picker instead
/// (`ModInstaller.Discover`).
///
/// ⚠️ Link2EA / "Steam is not installed" is not worth distinguishing here. The caller only needs
/// either a folder it can write to or the news that it has none.
internal static partial class SteamLibrary
{
    /// The game's folder name inside a library's `steamapps\common`. Both the Steam folder and the
    /// process's own name differ in case from this one, which is why every comparison below is
    /// ordinal-ignore-case.
    internal const string GameFolderName = "METAL GEAR RISING REVENGEANCE";

    /// The executable that decides whether a candidate folder really is the game.
    internal const string GameExeName = "METAL GEAR RISING REVENGEANCE.exe";

    /// Every library Steam knows about, the installation root first. Empty when Steam cannot be
    /// found at all — the caller treats that as "no game folder", not as a failure.
    internal static IReadOnlyList<string> Libraries()
    {
        var roots = new List<string>();
        var steam = SteamRoot();
        if (steam is null)
        {
            return roots;
        }

        roots.Add(steam);

        var vdf = Path.Combine(steam, "steamapps", "libraryfolders.vdf");
        try
        {
            if (File.Exists(vdf))
            {
                foreach (var path in ParseLibraryPaths(File.ReadAllText(vdf)))
                {
                    // The installation root is already in the list, and one library can be named
                    // twice once a second account's config is involved.
                    if (!Contains(roots, path))
                    {
                        roots.Add(path);
                    }
                }
            }
        }
        catch (Exception)
        {
            // A vdf nobody can read costs the extra libraries, not the search: the installation
            // root alone is right for most installs.
        }

        return roots;
    }

    /// The folder the game is installed in, or null.
    internal static string? GameFolder()
    {
        foreach (var library in Libraries())
        {
            var candidate = Path.Combine(library, "steamapps", "common", GameFolderName);
            if (IsGameFolder(candidate))
            {
                return candidate;
            }
        }

        return null;
    }

    /// Whether a folder really holds the game. The exe is the test rather than the folder name:
    /// a user-chosen folder is treated exactly like a discovered one, and an empty `common` entry
    /// from a vdf must not pass for an installation.
    internal static bool IsGameFolder(string? folder) =>
        !string.IsNullOrEmpty(folder)
        && File.Exists(Path.Combine(folder, GameExeName));

    /// The Steam installation root, from the per-user registry key Steam writes on every run.
    internal static string? SteamRoot()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(@"Software\Valve\Steam");
            if (key?.GetValue("SteamPath") is string path && path.Length > 0)
            {
                return Directory.Exists(path) ? path : null;
            }
        }
        catch (Exception)
        {
            // No key, no permission, a redirected hive: all the same answer.
        }

        return null;
    }

    /// The library paths inside a `libraryfolders.vdf`.
    ///
    /// The file's shape has changed twice (`"1" "D:\\Games"` once, `"1" { "path" "D:\\Games" }`
    /// since), so both are read rather than betting on one. A regex over the text is deliberate:
    /// this needs paths, not a VDF parser, and a real parser would be a dependency the app carries
    /// for one string per library.
    ///
    /// ⚠️ The values are VDF strings, so a backslash is written escaped (`D:\\Games`) and has to be
    /// unescaped — a path left as `D:\Games` with the doubled separators is a folder that does not
    /// exist.
    internal static IEnumerable<string> ParseLibraryPaths(string vdf)
    {
        var found = new List<string>();

        // `"path"  "D:\\Games"` — the modern, object-shaped entry.
        foreach (Match match in PathEntry().Matches(vdf))
        {
            Add(found, Unescape(match.Groups["path"].Value));
        }

        // `"1"  "D:\\Games"` — the older, plain-list entry. The key is a number, which is what
        // separates a library from every other `"key" "value"` pair in the file.
        foreach (Match match in NumberedEntry().Matches(vdf))
        {
            Add(found, Unescape(match.Groups["path"].Value));
        }

        return found;
    }

    static void Add(List<string> found, string path)
    {
        if (path.Length > 0 && Directory.Exists(path) && !Contains(found, path))
        {
            found.Add(path);
        }
    }

    static bool Contains(List<string> paths, string path)
    {
        foreach (var known in paths)
        {
            if (string.Equals(known, path, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        return false;
    }

    /// VDF escapes a backslash, and Windows paths are nothing but backslashes. Quotes and tabs are
    /// escaped the same way; nothing else appears in a library path.
    static string Unescape(string value) =>
        value.Replace(@"\\", @"\").Replace("\\\"", "\"").Replace(@"\t", "\t").Trim();

    [GeneratedRegex(@"""path""\s+""(?<path>(?:\\.|[^""])*)""", RegexOptions.IgnoreCase)]
    private static partial Regex PathEntry();

    [GeneratedRegex(@"""(?<key>\d+)""\s+""(?<path>(?:\\.|[^""])*)""")]
    private static partial Regex NumberedEntry();
}
