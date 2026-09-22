using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

/// The on-disk workspace: the `.tas` files of one folder, and the file operations the script list
/// offers over them.
///
/// Nothing here throws at its caller. Every call comes either from a click handler or from a
/// render, and the only thing either can do with an exception is paint it — so a folder that is
/// gone, a file that cannot be read or written, comes back as a message instead.
///
/// Only `.tas` is a script, and only at the folder's top level: the format has three
/// representations and the text is the one that lives on disk here (the JSON an API script is
/// written in stays in the API, `docs/SCRIPT_DSL.md` in the sibling repo).
internal static class Workspace
{
    /// A script text is a `.tas` file — the DSL's own extension, so the two cannot drift apart.
    internal const string Extension = ScriptDsl.Extension;

    /// What a new file is called when there is nothing in the folder to name it after, and what a
    /// copy gets appended to the name it copies.
    const string NewStem = "script";
    const string CopySuffix = "-copy";

    /// UTF-8 without a byte-order mark: the DSL reads the first character of the first line, and a
    /// mark would turn the rules line into a frame the parser refuses.
    static readonly UTF8Encoding Utf8NoBom = new(encoderShouldEmitUTF8Identifier: false);

    /// The scripts of a folder, and why the folder itself could not be read.
    internal sealed record Listing(IReadOnlyList<ScriptEntry> Scripts, string? Error);

    /// The folder's scripts by name, each one read and parsed: the list shows a script's frame
    /// count, and a file that does not read as a script says so in its own row.
    ///
    /// Read on listing rather than on selection because a row has to show a frame count, and that
    /// count only exists once the text has been parsed. A workspace is a handful of files, so the
    /// cost is one pass over them per listing — a listing that the shell memoizes, not one per
    /// frame.
    internal static Listing List(string? folder)
    {
        if (string.IsNullOrEmpty(folder))
        {
            return new Listing([], null);
        }

        if (!Directory.Exists(folder))
        {
            return new Listing([], $"The workspace folder is gone: {folder}");
        }

        string[] files;
        try
        {
            files = Directory.GetFiles(folder);
        }
        catch (Exception error)
        {
            return new Listing([], $"Cannot read {folder}: {error.Message}");
        }

        var scripts = new List<ScriptEntry>(files.Length);
        foreach (var file in files)
        {
            if (IsScript(file))
            {
                scripts.Add(Read(file));
            }
        }

        scripts.Sort((left, right) =>
            string.Compare(left.Name, right.Name, StringComparison.OrdinalIgnoreCase));
        return new Listing(scripts, null);
    }

    /// A new, empty script in the folder. Empty on purpose: the file name is not the script's name
    /// (the text never renames the file), so there is nothing to write into it that the author
    /// would not immediately have to take back — and every line of the format that could stand in
    /// for "empty" (a rules line, a first frame) is a script somebody else would rather not have.
    internal static (string? Path, string? Error) Create(string folder)
    {
        var path = Unique(folder, NewStem);
        var error = Write(path, string.Empty);
        return error is null ? (path, null) : (null, error);
    }

    /// A copy of a script, beside it, under `<name>-copy`. What is copied is the file, not the
    /// document: the two are the same thing here, and the copy is a place to start editing from.
    internal static (string? Path, string? Error) Duplicate(string path)
    {
        var folder = Path.GetDirectoryName(path);
        var stem = Path.GetFileNameWithoutExtension(path);
        if (string.IsNullOrEmpty(folder) || string.IsNullOrEmpty(stem))
        {
            return (null, $"Cannot tell where {path} lives");
        }

        var target = Unique(folder, stem + CopySuffix);
        try
        {
            File.Copy(path, target);
            return (target, null);
        }
        catch (Exception error)
        {
            return (null, $"Cannot copy {path}: {error.Message}");
        }
    }

    /// Overwrites a script's file with a text. The whole file, because the whole file is the
    /// script: there is no part of it the editor holds somewhere else.
    ///
    /// The text is written in the format's own separator (`\n`) whatever the caller's text uses —
    /// the editor's buffer is the text box's, and a text box separates its lines with a lone `\r`
    /// (measured). A file written through here therefore still reads as the script it was.
    internal static string? Write(string path, string text)
    {
        try
        {
            File.WriteAllText(path, ScriptDsl.Lines(text), Utf8NoBom);
            return null;
        }
        catch (Exception error)
        {
            return $"Cannot write {path}: {error.Message}";
        }
    }

    internal static string? Delete(string path)
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

    /// A free `<stem>.tas` in the folder: `stem-2`, `stem-3`, … — a file that is already there is
    /// never overwritten silently.
    static string Unique(string folder, string stem)
    {
        var candidate = Path.Combine(folder, stem + Extension);
        for (var counter = 2; File.Exists(candidate); counter++)
        {
            candidate = Path.Combine(folder, $"{stem}-{counter}{Extension}");
        }

        return candidate;
    }

    static bool IsScript(string path) =>
        string.Equals(Path.GetExtension(path), Extension, StringComparison.OrdinalIgnoreCase);

    /// One file as an entry. Both the read and the parse are wrapped, not just the parse: the
    /// listing is built during a render (the shell memoizes it), and a render that throws takes
    /// the window with it — a file nobody can read is a row that says so.
    ///
    /// The text is kept as the format reads it (`ScriptDsl.Lines`): a file whose lines are
    /// separated by something other than `\n` would otherwise be listed as one broken line, while
    /// the editor — which normalises before parsing — shows it as the script it is. Two answers for
    /// the same file is the one thing this must not do.
    static ScriptEntry Read(string path)
    {
        var name = Path.GetFileNameWithoutExtension(path);
        try
        {
            var text = ScriptDsl.Lines(File.ReadAllText(path));
            var status = ScriptTextStatus.Of(text);
            return status.Document is { } document
                ? new ScriptEntry(path, name, text, ScriptTextStatus.LastFrame(document), null)
                : new ScriptEntry(path, name, text, 0, status.Error);
        }
        catch (Exception error)
        {
            return new ScriptEntry(path, name, string.Empty, 0, $"Cannot read: {error.Message}");
        }
    }
}
