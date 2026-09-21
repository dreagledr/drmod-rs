using System;

/// A script as the workspace sees it.
///
/// Stub shape: the real one carries a trigger, a restart policy and the command list
/// (see `docs/API.md` §4.4 in the Rust repo). `Id` is what panes and list rows key on,
/// so it has to be stable and equatable — not the display name.
sealed record ScriptEntry(string Id, string Name, int Frames);
