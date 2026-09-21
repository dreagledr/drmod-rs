/// What a script text currently says: the document it reads as, or the reason it reads as
/// nothing.
///
/// This is the whole of the text region's live status. The region parses on every keystroke — the
/// format is the mod's own (`docs/SCRIPT_DSL.md` in the sibling repo) — so a text that names its
/// line is what the user sees while typing, before anything is handed to the game.
internal sealed record ScriptTextStatus(ScriptDocument? Document, string? Error)
{
    internal static ScriptTextStatus Of(string text)
    {
        try
        {
            return new ScriptTextStatus(ScriptDsl.Parse(text), null);
        }
        catch (ScriptFormatException refused)
        {
            return new ScriptTextStatus(null, refused.Message);
        }
    }

    internal bool IsOk => Error is null;

    /// The last frame the script touches: `t + duration` of its longest command. Zero for a
    /// document with nothing to measure.
    internal static uint LastFrame(ScriptDocument document)
    {
        var last = 0u;
        foreach (var command in document.Commands)
        {
            if (command.T + command.Duration > last)
            {
                last = command.T + command.Duration;
            }
        }

        return last;
    }
}
