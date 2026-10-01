//! The one reading of the script on screen, kept across frames.
//!
//! ⚠️ **This is a cache because the work is not free and it is asked for several times per frame.**
//! The run controls' status line, the table, Save's enabled state and the text panel's own status all
//! read the same answer, and both halves of it cost O(text): the parse builds a command list, the
//! projection a row for every frame a token touches. Asking for them per panel meant three parses and
//! one projection per frame — measured at 0.75 ms of UI for six panels, against 0.27 ms for four
//! before the panels were split apart — and a script of a few thousand frames is exactly the one the
//! editor is for.
//!
//! So the text is read once a frame, and the reading is rebuilt only when the *text* changes. That
//! makes the cost proportional to what the author types rather than to how long the window is open.

use crate::script::projection::{self, ScriptFrame};
use crate::script::ScriptTextStatus;

/// The parse of a text and the frames it projects to, kept together because the panels want both.
pub struct TextCache {
    /// The text these were read from. The identity of the cache: a different text is a different
    /// reading, and the comparison is what saves the work.
    text: String,
    /// What the text says — a document, or the reason it does not read as one.
    pub status: ScriptTextStatus,
    /// The command table's rows: one per frame a token touches, empty when the text does not parse.
    ///
    /// Empty rather than projected-on-best-effort, because the text panel already carries the
    /// parser's message and a table built from a half-parsed text would say something else again.
    pub frames: Vec<ScriptFrame>,
}

impl Default for TextCache {
    fn default() -> Self {
        Self {
            text: String::new(),
            status: ScriptTextStatus::of(""),
            frames: Vec::new(),
        }
    }
}

impl TextCache {
    /// Reads a text, whether or not it has changed.
    fn read(text: &str) -> Self {
        let status = ScriptTextStatus::of(text);
        let frames = if status.document.is_some() {
            projection::project(text)
        } else {
            Vec::new()
        };

        Self {
            text: text.to_owned(),
            status,
            frames,
        }
    }

    /// Brings the reading up to date with `text`, and says whether it had to do any work.
    ///
    /// The comparison is a string compare, which is O(text) but allocates nothing and touches
    /// contiguous memory — next to the parse and the projection it is the cheap way to find out
    /// whether they are needed.
    pub fn refresh(&mut self, text: &str) -> bool {
        if self.text == text {
            return false;
        }

        *self = Self::read(text);
        true
    }
}
