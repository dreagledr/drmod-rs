//! The script text panel: the selected script as `.tas` text, with what that text currently says
//! above it.
//!
//! The text is edited in place and read back on every keystroke — the status line is the converter's
//! own answer, either the document's name, command count and last frame or the line the parser
//! refused, with the frame the parse had reached behind it.
//!
//! ⚠️ What is typed is whatever the control reported. There is deliberately no `set_text` after a
//! keystroke: the text editor *is* the buffer, and feeding the text back on every change would move
//! the caret. The control is only given a new text when a *different* script is selected — see
//! `editor::shell`, which is the one place that happens.
//!
//! ⚠️ The text is `\n`-separated, like the format itself. `dear-imgui-cte` is not a WinUI `TextBox`
//! — it does not take a lone `\r` as a line break, and feeding it one makes it show a whole script
//! as a single line.
//!
//! The command reference is a panel of its own ([`super::mod_panel`]'s sibling, drawn by the shell):
//! a plain list and nothing more — it does not follow the caret, does not complete anything and does
//! not colour the text. Colouring a token means walking the document on every keystroke, which costs
//! the editor its responsiveness on a text of a few hundred frames, and `script/commands.rs` is read
//! by that panel only.

use dear_app::imgui::Ui;
use dear_imgui_cte::{CteUiExt, TextEditor};

use crate::script::commands;
use crate::script::{dsl, ScriptTextStatus};

/// What a frame of this panel decided.
#[derive(Clone, Debug, Default)]
pub struct TextActions {
    /// The text box reported an edit; the caller reads the editor's text back.
    pub changed: bool,
}

/// The text panel's status line: what the text currently says, and the button that shows or hides
/// the command reference.
///
/// The reference is a panel of its own ([`super::shell`]), so the button does not open anything
/// *inside* this one — it flips the other panel's visibility, and `open` is that flag. One flag, so
/// the button's word and the panel's presence cannot disagree.
pub fn status_line(ui: &Ui, status: &ScriptTextStatus, open: &mut bool) {
    match &status.document {
        Some(document) => ui.text_wrapped(&summary(document)),
        None => ui.text_wrapped(status.error.as_deref().unwrap_or("")),
    }

    ui.same_line();
    if ui.button(if *open { "Hide commands" } else { "Commands" }) {
        *open = !*open;
    }
}

/// The document's own summary: its name, how many commands it holds and the last frame it touches.
/// The `name · N commands · last frame M` shape is the C# sibling's, and it is the shape an author
/// reads a script's size off.
pub fn summary(document: &crate::script::ScriptDocument) -> String {
    format!(
        "{} · {} commands · last frame {}",
        document.name,
        document.commands.len(),
        ScriptTextStatus::last_frame(document)
    )
}

/// The text box itself. `editor` is the CTE editor the shell owns; the returned `bool` is the CTE
/// "changed" flag.
///
/// The box, the palette, the Python language (whose `#` comment marker is the DSL's own) and the
/// token autocomplete are configured once, in the shell's `configure_imgui` — this only submits it
/// into its pane.
pub fn draw_text(ui: &Ui, editor: &mut TextEditor) -> bool {
    ui.text_editor(editor, "Script text")
        .size([-1.0, -1.0])
        .build()
        .unwrap_or(false)
}

/// The command reference: one row per token — how it is spelled, then what it means.
///
/// A row is not clickable and nothing here follows the caret: this is a reference, so a row is text.
pub fn draw_reference(ui: &Ui) {
    ui.text("Frame");
    ui.text_wrapped(&format!("{}  —  {}", commands::FRAME.spelling, commands::FRAME.help));

    ui.text("Commands");
    ui.text_wrapped(":frames holds a token that long — 1 by default");

    ui.separator();
    for command in commands::frame() {
        ui.text_wrapped(&format!("{}  —  {}", command.spelling, command.help));
    }

    ui.separator();
    ui.text("Rules line");
    for rule in commands::RULES {
        ui.text_wrapped(&format!("{}  —  {}", rule.spelling, rule.help));
    }
}

/// The DSL's own vocabulary, as the autocomplete offers it: every token a frame line accepts, plus
/// the three attributes of the rules line.
///
/// The stick tokens and the rule attributes are added by hand because they are not flags of the
/// input table — a stick is the shape of a whole command and a rule is not a frame token at all.
pub fn autocomplete_tokens() -> Vec<String> {
    let mut tokens: Vec<String> = dsl::vocabulary()
        .into_iter()
        .map(|(token, _)| token.to_owned())
        .collect();

    for stick in &commands::STICKS {
        tokens.push(stick.spelling_prefix().to_owned());
    }

    for rule in &commands::RULES {
        tokens.push(rule.spelling_prefix().to_owned());
    }

    tokens
}
