/// A script as the workspace sees it: one `.tas` file of the open folder.
///
/// `Path` is the identity — the list rows, the selection and the text buffers key on it —
/// because a file is the one thing here with a stable, equatable name: the text format's own
/// `name` may repeat across files and is edited freely.
///
/// `Text` is what the file held when the folder was listed, and a save writes the buffer back
/// over it. `Frames` is the last frame that text touches, and `Error` the reason it does not
/// read as a script at all — I/O or the parser's own refusal. A file the mod would turn down is
/// listed with its error rather than dropped, so a typo shows up in the row instead of a file
/// that silently is not there.
sealed record ScriptEntry(string Path, string Name, string Text, uint Frames, string? Error)
{
    /// Whether the text reads as a script the mod would take — a row with an error is still a
    /// file the editor opens and saves, it just is not one the game would run.
    internal bool Reads => Error is null;
}
