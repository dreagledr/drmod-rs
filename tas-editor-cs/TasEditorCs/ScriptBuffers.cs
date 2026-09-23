using System;
using System.Collections.Generic;

/// The text the editor holds for the scripts of the workspace, and what still differs from the
/// files.
///
/// A buffer is the text box's own text, kept exactly as the control reported it (its lines end in
/// a lone `\r` — measured) because that is what stops the reconciler from writing the text back on
/// every keystroke and moving the caret with it. "Has unsaved changes" is therefore a question
/// about the *reading* of the two texts (`ScriptDsl.Lines`), not about a map's keys: a script whose
/// buffer reads the same as its file is on its file, whatever the separators are.
///
/// A buffer outlives the selection on purpose — switching to another script in the list and back
/// must not throw away what is in the editor — so the shell keeps them, and the list, the editor
/// and the Save button all read the same map.
internal static class ScriptBuffers
{
    internal static readonly IReadOnlyDictionary<string, string> Empty =
        new Dictionary<string, string>(StringComparer.Ordinal);

    /// The text the editor shows for a script: what was typed, or the file's own text.
    ///
    /// A script nobody has typed in yet is shown in the separator a text box reports (`\r`, see
    /// <see cref="ScriptDsl.Lines"/>), not the file's own `\n`. The two read the same, but the
    /// control is what the reconciler compares against: handed a `\n` text it would write it back
    /// on every render — and setting `Text` drops the caret to the start, which is a caret that
    /// jumps every time the game is polled while the author is reading the script.
    internal static string Resolve(IReadOnlyDictionary<string, string> buffers, ScriptEntry script) =>
        buffers.TryGetValue(script.Path, out var typed) ? typed : Boxed(script.Text);

    /// A text as a text box holds it: one `\n`, one `\r`. The file's line separator put back is
    /// <see cref="ScriptDsl.Lines"/>, and this is the same question asked the other way.
    internal static string Boxed(string text) => text.Replace("\n", "\r");

    /// Whether the editor holds something a save would change. A save leaves the buffer where it
    /// is — the text box already holds it, and handing the file's text back would move the caret —
    /// so this is asked of the texts themselves: what the buffer reads against what the file reads.
    internal static bool IsDirty(IReadOnlyDictionary<string, string> buffers, ScriptEntry script) =>
        buffers.TryGetValue(script.Path, out var typed)
        && ScriptDsl.Lines(typed) != ScriptDsl.Lines(script.Text);

    /// The buffers with one script's text replaced, as the control reported it.
    ///
    /// The map handed in is left alone: it is a piece of state a render is already holding, and a
    /// Reactor state value the setter does not see as a different instance is a render that does
    /// not happen.
    internal static IReadOnlyDictionary<string, string> Typed(
        IReadOnlyDictionary<string, string> buffers,
        ScriptEntry script,
        string text) =>
        Replaced(buffers, script.Path, text);

    /// Drops a script's buffer: after a delete, which leaves nothing for the text to be about.
    ///
    /// The map handed in comes back untouched when it has nothing to drop — the delete path calls
    /// this for every script whether or not it was ever typed into.
    internal static IReadOnlyDictionary<string, string> Without(
        IReadOnlyDictionary<string, string> buffers,
        string path) =>
        buffers.ContainsKey(path) ? Replaced(buffers, path, null) : buffers;

    /// Moves a script's buffer to the name its file was just renamed to.
    ///
    /// A buffer is what the editor holds for a *path*, so a rename has to carry the text across or the
    /// typed-but-unsaved script would come back as the file's own text — the rename would silently
    /// throw work away. A script that was never typed into has nothing to move and comes back
    /// untouched.
    internal static IReadOnlyDictionary<string, string> Renamed(
        IReadOnlyDictionary<string, string> buffers,
        string from,
        string to)
    {
        if (!buffers.TryGetValue(from, out var text))
        {
            return buffers;
        }

        var next = new Dictionary<string, string>(buffers.Count, StringComparer.Ordinal);
        foreach (var pair in buffers)
        {
            if (pair.Key != from)
            {
                next[pair.Key] = pair.Value;
            }
        }

        next[to] = text;
        return next;
    }

    /// The map with one key replaced, or removed when the value is `null`. Never the instance that
    /// came in: it is a piece of state a render is already holding.
    static IReadOnlyDictionary<string, string> Replaced(
        IReadOnlyDictionary<string, string> buffers,
        string path,
        string? text)
    {
        var next = new Dictionary<string, string>(buffers.Count + 1, StringComparer.Ordinal);
        foreach (var pair in buffers)
        {
            if (pair.Key != path)
            {
                next[pair.Key] = pair.Value;
            }
        }

        if (text is not null)
        {
            next[path] = text;
        }

        return next;
    }
}
