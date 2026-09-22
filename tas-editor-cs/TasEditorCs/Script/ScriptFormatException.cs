using System;

/// Text or a script the converters refuse.
///
/// Thrown with the offending line (DSL) or field (JSON) named: the editor has to
/// say *what* is wrong before it hands a script to the mod, which answers a bad
/// one with `400` and a message of its own.
///
/// `Frame` is the frame the parse had reached — the last one it read out of a line before the
/// offending one, `null` while nothing had been parsed yet (an error on the rules line, or a first
/// line that is not a frame at all). A line number is how the file is navigated and a frame number
/// is how the script is: the two together are what makes a typo findable without counting lines,
/// and the frame is added to the message by <see cref="FrameAware"/>.
internal sealed class ScriptFormatException(string message, uint? frame = null) : Exception(message)
{
    /// The frame the parse had reached, if any had been.
    internal uint? Frame { get; } = frame;

    /// The message as the editor shows it: the parser's own wording, and the frame behind it while
    /// that is known. Built here rather than at every throw site so no message can forget the frame
    /// it was thrown with.
    internal string FrameAware() =>
        Frame is { } frame ? $"{Message} · up to frame {frame}" : Message;
}
