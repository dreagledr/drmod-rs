//! The text the editor holds for the scripts of the workspace, and what still differs from the
//! files.
//!
//! A buffer is the text editor's own text, kept exactly as the control reported it. "Has unsaved
//! changes" is therefore a question about the *reading* of the two texts ([`dsl::lines`]), not about
//! a map's keys: a script whose buffer reads the same as its file is on its file, whatever the
//! separators are.
//!
//! ⚠️ **The separator is `\n`, and that is not a detail.** The Reactor sibling's buffer is a WinUI
//! `TextBox`, which hands its text back with a lone `\r` between the lines — so *its* buffers need
//! `\n` → `\r` on the way in, or the reconciler rewrites `Text` every render and drops the caret.
//! `dear-imgui-cte` is not a `TextBox`: it is a text editor for `\n`, and feeding it `\r` makes it
//! read the whole script as **one line** (measured — the first version of this port did exactly
//! that). So there is no conversion here, in either direction.
//!
//! A buffer outlives the selection on purpose — switching to another script in the list and back
//! must not throw away what is in the editor — so the shell keeps them, and the list, the editor and
//! the Save button all read the same map.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::script::dsl;
use crate::workspace::ScriptEntry;

/// The buffers, keyed by the script's path.
pub type Buffers = BTreeMap<PathBuf, String>;

/// The text the editor shows for a script: what was typed, or the file's own text.
///
/// A script nobody has typed in yet is shown as the file holds it. The editor is only ever given a
/// new text when the *selection* changes (see `editor::shell`), so there is no write-back per
/// keystroke to reconcile — which is what the WinUI sibling needs the `\r` convention for, and why
/// this port neither needs nor wants one.
pub fn resolve(buffers: &Buffers, script: &ScriptEntry) -> String {
    match buffers.get(&script.path) {
        Some(typed) => typed.clone(),
        None => script.text.clone(),
    }
}

/// Whether the editor holds something a save would change.
pub fn is_dirty(buffers: &Buffers, script: &ScriptEntry) -> bool {
    buffers
        .get(&script.path)
        .is_some_and(|typed| dsl::lines(typed) != dsl::lines(&script.text))
}

/// The buffers with one script's text replaced, as the control reported it.
pub fn typed(buffers: &mut Buffers, script: &ScriptEntry, text: String) {
    buffers.insert(script.path.clone(), text);
}

/// Drops a script's buffer: after a delete, which leaves nothing for the text to be about.
pub fn without(buffers: &mut Buffers, path: &Path) {
    buffers.remove(path);
}

/// Moves a script's buffer to the name its file was just renamed to.
///
/// A buffer is what the editor holds for a *path*, so a rename has to carry the text across or the
/// typed-but-unsaved script would come back as the file's own text — the rename would silently throw
/// work away. A script that was never typed into has nothing to move.
pub fn renamed(buffers: &mut Buffers, from: &Path, to: &Path) {
    if let Some(text) = buffers.remove(from) {
        buffers.insert(to.to_path_buf(), text);
    }
}
