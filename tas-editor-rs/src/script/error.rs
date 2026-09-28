//! Text or a script the converters refuse.
//!
//! Thrown with the offending line (DSL) or field (JSON) named: the editor has to say *what* is
//! wrong before it hands a script to the mod, which answers a bad one with `400` and a message
//! of its own.
//!
//! `frame` is the frame the parse had reached — the last one it read out of a line before the
//! offending one, `None` while nothing had been parsed yet (an error on the rules line, or a
//! first line that is not a frame at all). A line number is how the file is navigated and a
//! frame number is how the script is: the two together are what makes a typo findable without
//! counting lines.

use std::fmt;

/// A refusal from any of the script converters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFormatException {
    message: String,
    frame: Option<u32>,
}

impl ScriptFormatException {
    /// A refusal with only its own wording.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            frame: None,
        }
    }

    /// A refusal that also carries the frame the parse had reached.
    pub fn at_frame(message: impl Into<String>, frame: Option<u32>) -> Self {
        Self {
            message: message.into(),
            frame,
        }
    }

    /// The parser's own wording, without the frame.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The frame the parse had reached, if any had been.
    pub fn frame(&self) -> Option<u32> {
        self.frame
    }

    /// The message as the editor shows it: the parser's own wording, and the frame behind it
    /// while that is known. Built here rather than at every throw site so no message can
    /// forget the frame it was thrown with.
    pub fn frame_aware(&self) -> String {
        match self.frame {
            Some(frame) => format!("{} · up to frame {frame}", self.message),
            None => self.message.clone(),
        }
    }
}

impl fmt::Display for ScriptFormatException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.frame_aware())
    }
}

impl std::error::Error for ScriptFormatException {}

/// The refusal every converter returns.
pub type ScriptResult<T> = Result<T, ScriptFormatException>;
