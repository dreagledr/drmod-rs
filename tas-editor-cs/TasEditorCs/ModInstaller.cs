using System;
using System.IO;

/// What the game already has, and what installing would change.
///
/// The question the panel asks before offering a button, and the answer it paints: a whole-folder
/// presence check by content, so "installed" means *these* bytes rather than a file that happens to
/// have the right name.
internal enum ModState
{
    /// No game folder to look in — discovered or picked. Everything else is unknown until one is.
    NoGameFolder,

    /// The game folder is there and holds no plugin.
    NotInstalled,

    /// The plugin is there and is the one this build carries.
    Installed,

    /// The plugin is there but is a different build (older, newer, or hand-placed).
    OtherVersion,
}

/// What an install did, in a line the panel can paint. Never an exception: every caller is a click
/// handler or a render, and the only thing either can do with a failure is show it.
internal sealed record InstallResult(bool Ok, string Message)
{
    internal static InstallResult Done(string message) => new(true, message);

    internal static InstallResult Failed(string message) => new(false, message);
}

/// Installs the mod into the game's root, the way the mod's own `readme.txt` describes it: the ASI
/// loader as `d3d9.dll` and the mod as `plugins\drmod_rs_lib.asi`.
///
/// Nothing here touches the running game. The files are the whole install — the game loads the
/// plugin itself on its next start — so there is no injection, no process to kill, and nothing to
/// undo but the two files. That is also why the panel says "restart the game" rather than acting.
///
/// ⚠️ A `d3d9.dll` that is already there belongs to somebody else until proven otherwise. Almost
/// every machine with a ReShade, an ENB or another ASI mod has one, and overwriting it would break
/// that mod to install this one. The plugin is loaded by any ASI loader, so the loader is only ever
/// *added* when the folder has none.
internal static class ModInstaller
{
    /// Where the loader goes inside the game folder — DX9 picked up by the game's own `d3d9.dll`
    /// import, which is the whole mechanism of an ASI loader.
    internal const string LoaderPath = ModPayload.LoaderName;

    /// Where the plugin goes: an ASI loader scans `plugins\` beside itself.
    internal const string PluginDir = "plugins";

    /// The upstream loader, for the message about an existing `d3d9.dll` that is not ours. The mod's
    /// own readme points here too (`docs/asi-readme.txt`).
    internal const string LoaderUrl = "https://github.com/ThirteenAG/Ultimate-ASI-Loader/releases";

    /// The full path of the plugin inside a game folder.
    internal static string PluginPath(string gameFolder) =>
        Path.Combine(gameFolder, PluginDir, ModPayload.AsiName);

    /// What the game folder holds now, by content. The payload is what "installed by us" means:
    /// same bytes → this build; different bytes → somebody else's, and worth asking about.
    internal static ModState Detect(string? gameFolder, ModPayload payload)
    {
        if (!SteamLibrary.IsGameFolder(gameFolder))
        {
            return ModState.NoGameFolder;
        }

        var plugin = PluginPath(gameFolder!);
        if (!File.Exists(plugin))
        {
            return ModState.NotInstalled;
        }

        return SameBytes(SafeRead(plugin), payload.Asi) ? ModState.Installed : ModState.OtherVersion;
    }

    /// Whether the game already has a `d3d9.dll` at all — and whether the one it has is ours.
    ///
    /// Two separate questions on purpose: a foreign loader is not a reason to refuse the install
    /// (the plugin does not need *our* loader), it is a reason not to touch that file and to say so.
    internal static (bool Present, bool Ours) Loader(string? gameFolder, ModPayload payload)
    {
        if (!SteamLibrary.IsGameFolder(gameFolder))
        {
            return (false, false);
        }

        var path = Path.Combine(gameFolder!, LoaderPath);
        if (!File.Exists(path))
        {
            return (false, false);
        }

        return (true, SameBytes(SafeRead(path), payload.Loader));
    }

    /// Writes the mod into the game folder.
    ///
    /// The order is deliberate — the loader first, the plugin last. A half-finished install must not
    /// leave a plugin whose loader is missing (the game would silently load nothing), and the plugin
    /// is the file whose presence `Detect` reads, so writing it last means an interrupted install
    /// reads as "not installed" rather than as "installed and broken".
    ///
    /// ⚠️ Both writes go through a temporary file beside the target and a move over it. A direct
    /// `File.WriteAllBytes` onto the plugin can be interrupted by a full disk or a crash mid-write
    /// and leave a truncated DLL that the game fails to load — the one failure that looks like a
    /// broken mod rather than like a failed install.
    internal static InstallResult Install(string? gameFolder, ModPayload payload)
    {
        if (!SteamLibrary.IsGameFolder(gameFolder))
        {
            return InstallResult.Failed(
                "No game folder — point the editor at the folder holding "
                + $"{SteamLibrary.GameExeName}.");
        }

        var folder = gameFolder!;
        var notes = new System.Collections.Generic.List<string>();

        var (loaderPresent, loaderOurs) = Loader(folder, payload);
        if (!loaderPresent)
        {
            if (Write(Path.Combine(folder, LoaderPath), payload.Loader) is { } loaderError)
            {
                return InstallResult.Failed(loaderError);
            }

            notes.Add($"added {LoaderPath}");
        }
        else if (!loaderOurs)
        {
            // Not ours, and not ours to replace: almost certainly ReShade, an ENB, or another mod's
            // loader. Any ASI loader loads this plugin, so the install is complete without ours.
            notes.Add($"kept the existing {LoaderPath} — this plugin works with any ASI loader");
        }

        var pluginDir = Path.Combine(folder, PluginDir);
        try
        {
            Directory.CreateDirectory(pluginDir);
        }
        catch (Exception error)
        {
            return InstallResult.Failed($"Cannot create {pluginDir}: {error.Message}");
        }

        if (Write(PluginPath(folder), payload.Asi) is { } pluginError)
        {
            return InstallResult.Failed(pluginError);
        }

        notes.Insert(0, $"installed {PluginDir}\\{ModPayload.AsiName}");
        var message = string.Join("; ", notes) + ". Restart the game for the mod to load.";

        if (loaderPresent && !loaderOurs)
        {
            message += $" No ASI loader of its own? Take the Win32 d3d9.dll from {LoaderUrl}";
        }

        return InstallResult.Done(message);
    }

    /// Whether there is anything of ours to take out: the plugin, or a loader that is ours. The
    /// caller uses this to decide whether the uninstall is a live action — a button that can only
    /// report "nothing to remove" is a click that should not have been offered.
    internal static bool CanRemove(string? gameFolder, ModPayload payload) =>
        Detect(gameFolder, payload) is ModState.Installed or ModState.OtherVersion
        || Loader(gameFolder, payload).Ours;

    /// Takes the mod out of the game folder.
    ///
    /// The mirror of <see cref="Install"/>, and the same rule about somebody else's file: the plugin
    /// is ours and always goes, but a `d3d9.dll` is only removed when it is byte-for-byte the one we
    /// would have written. A ReShade, an ENB or another ASI mod needs its loader, and uninstalling
    /// this mod must not take that away — the message says which of the two it did.
    ///
    /// ⚠️ The plugin goes **first**, the loader second, which is the reverse of the install order and
    /// deliberate for the same reason: if the removal is interrupted, what is left is a loader with no
    /// plugin (harmless) rather than a plugin the game still tries to load.
    ///
    /// Nothing is deleted from the `plugins\` folder itself. Other mods live there, and an ASI loader
    /// does not care whether the directory outlives its contents.
    internal static InstallResult Remove(string? gameFolder, ModPayload payload)
    {
        if (!SteamLibrary.IsGameFolder(gameFolder))
        {
            return InstallResult.Failed(
                "No game folder — point the editor at the folder holding "
                + $"{SteamLibrary.GameExeName}.");
        }

        var folder = gameFolder!;
        var notes = new System.Collections.Generic.List<string>();

        var plugin = PluginPath(folder);
        if (File.Exists(plugin))
        {
            if (Delete(plugin) is { } pluginError)
            {
                return InstallResult.Failed(pluginError);
            }

            notes.Add($"removed {PluginDir}\\{ModPayload.AsiName}");
        }

        var (loaderPresent, loaderOurs) = Loader(folder, payload);
        if (loaderPresent && loaderOurs)
        {
            if (Delete(Path.Combine(folder, LoaderPath)) is { } loaderError)
            {
                return InstallResult.Failed(loaderError);
            }

            notes.Add($"removed {LoaderPath}");
        }
        else if (loaderPresent)
        {
            notes.Add($"kept {LoaderPath} — it is not this build's, and another mod may need it");
        }

        if (notes.Count == 0)
        {
            return InstallResult.Done("Nothing to remove — the mod was not installed.");
        }

        return InstallResult.Done(
            string.Join("; ", notes) + ". The game no longer loads the mod on its next start.");
    }

    /// One file, written through a temporary beside it. The temporary shares the target's directory
    /// on purpose: a move across volumes is a copy, and would lose the atomicity this is for.
    static string? Write(string path, byte[] bytes)
    {
        var temporary = path + ".tmp";
        try
        {
            File.WriteAllBytes(temporary, bytes);
            File.Move(temporary, path, overwrite: true);
            return null;
        }
        catch (Exception error)
        {
            TryDelete(temporary);
            return $"Cannot write {path}: {error.Message}";
        }
    }

    static string? Delete(string path)
    {
        try
        {
            File.Delete(path);
            return null;
        }
        catch (Exception error)
        {
            return $"Cannot delete {path}: {error.Message}";
        }
    }

    static void TryDelete(string path)
    {
        try
        {
            File.Delete(path);
        }
        catch (Exception)
        {
            // A stray .tmp costs nothing; failing to clean it up must not mask the real error.
        }
    }

    static byte[]? SafeRead(string path)
    {
        try
        {
            return File.ReadAllBytes(path);
        }
        catch (Exception)
        {
            // Unreadable reads as "not ours", which is the safe side: the install will overwrite it
            // only after the user has been asked (the panel asks on anything but a byte match).
            return null;
        }
    }

    static bool SameBytes(byte[]? left, byte[] right)
    {
        if (left is null || left.Length != right.Length)
        {
            return false;
        }

        return left.AsSpan().SequenceEqual(right);
    }
}
