using System;

/// Text or a script the converters refuse.
///
/// Thrown with the offending line (DSL) or field (JSON) named: the editor has to
/// say *what* is wrong before it hands a script to the mod, which answers a bad
/// one with `400` and a message of its own.
internal sealed class ScriptFormatException(string message) : Exception(message);
