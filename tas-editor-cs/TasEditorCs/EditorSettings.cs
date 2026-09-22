using System;
using System.IO;

/// What the editor remembers between runs: the folder the workspace was last opened on.
///
/// A file of its own rather than `UsePersisted` — that hook is a process-lifetime cache (spec 033
/// §2), so it holds a value across a re-render but not across a restart, and the folder is the one
/// setting that has to survive one. `%LOCALAPPDATA%` is where the Rust sibling keeps the same
/// thing.
///
/// The file is one line: the folder path. Nothing is serialized, so there is nothing for the AOT
/// compiler to miss (the JSON representation of a script goes through a source-generated context —
/// `Script/ScriptJson.cs` — and a settings record would need one too).
internal static class EditorSettings
{
    /// Where the editor reads its settings from: `%LOCALAPPDATA%\tas-editor-cs\settings`.
    internal static string DefaultPath { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "tas-editor-cs",
        "settings");

    /// The remembered folder, or `null` when nothing was ever opened. A file that cannot be read
    /// or is empty is the same as no file at all: the editor opens on an empty workspace rather
    /// than refusing to start over its own settings.
    internal static string? Load(string? path = null)
    {
        try
        {
            var text = File.ReadAllText(path ?? DefaultPath).Trim();
            return text.Length == 0 ? null : text;
        }
        catch (Exception)
        {
            return null;
        }
    }

    /// Remembers a folder. A write that fails is not worth telling anyone about: it costs the
    /// user one folder pick next launch, and the workspace itself is unaffected.
    internal static void Save(string? folder, string? path = null)
    {
        var file = path ?? DefaultPath;
        try
        {
            var parent = Path.GetDirectoryName(file);
            if (!string.IsNullOrEmpty(parent))
            {
                Directory.CreateDirectory(parent);
            }

            File.WriteAllText(file, folder ?? string.Empty);
        }
        catch (Exception)
        {
            // Deliberately silent — see above.
        }
    }
}
