//! The script formats, and the hub between them.
//!
//! A script has three representations, and this module is where they meet:
//!
//! ```text
//! .tas text  ⇄  ScriptDocument  ⇄  API JSON   (the mod's POST /script/run body)
//!
//! CommandRow frames    (the text converter's own intermediate — expand / collapse)
//! ScriptFrame frames   (the command table's view, read straight off the text)
//! ```
//!
//! [`model::ScriptDocument`] is the hub and the JSON is the source of truth: it is the only
//! representation that carries the whole format. `raw_key`, `dik_key` and `when_enemy` have no
//! text spelling and no table column, so writing such a command out as a `.tas` file is an error
//! naming the command rather than a silent loss. The format itself is specified in
//! `docs/SCRIPT_DSL.md`.

pub mod commands;
pub mod dsl;
pub mod error;
pub mod frames;
pub mod json;
pub mod keys;
pub mod model;
pub mod projection;

pub use model::{ScriptCommand, ScriptDocument, ScriptInput, ScriptTrigger};

/// What a script text currently says: the document it reads as, or the reason it reads as
/// nothing.
///
/// This is the whole of the text region's live status. The region parses on every keystroke — the
/// format is the mod's own — so a text that names its line is what the user sees while typing,
/// before anything is handed to the game.
#[derive(Clone, Debug)]
pub struct ScriptTextStatus {
    pub document: Option<ScriptDocument>,
    pub error: Option<String>,
}

impl ScriptTextStatus {
    /// Reads a text, keeping the parser's own wording when it refuses.
    pub fn of(text: &str) -> Self {
        match dsl::parse(text) {
            Ok(document) => Self {
                document: Some(document),
                error: None,
            },
            // `frame_aware` and not the bare message: the line names where in the file the text
            // broke, and the frame the parse had reached names where in the *script* that was.
            Err(refused) => Self {
                document: None,
                error: Some(refused.frame_aware()),
            },
        }
    }

    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }

    /// The last frame the script touches: `t + duration` of its longest command. Zero for a
    /// document with nothing to measure.
    pub fn last_frame(document: &ScriptDocument) -> u32 {
        document
            .commands
            .iter()
            .map(|command| command.t + command.duration)
            .max()
            .unwrap_or(0)
    }
}
