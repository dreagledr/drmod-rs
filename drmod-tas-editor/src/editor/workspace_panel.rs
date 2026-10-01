//! The scripts panel: the `.tas` files of the open folder, and the operations that manage them.
//!
//! The port of the Reactor sibling's `WorkspacePanel.cs`. The panel holds no state of its own — the
//! selection, the unsaved markers and the folder all come from the shell, which is what lets the
//! list, the editor and Save agree about one script.
//!
//! ⚠️ The mod's install is **not** here: it is a panel of its own
//! ([`crate::editor::mod_panel`]), because a section nested in another cannot be moved or resized
//! against its neighbours, and the two are different jobs — one is a list of files, the other is a
//! write into the game folder.

use dear_app::imgui::Ui;

use crate::buffers::{self, Buffers};
use crate::editor::{mod_panel::ModPanelView, Editor, PendingFolder};
use crate::workspace::Listing;

/// What a frame of this panel decided. Returned rather than applied because the list is taken by
/// `&self.listing` while adopting a script needs `&mut` — and a click has to be one edit per frame,
/// not one per row.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceActions {
    pub select: Option<usize>,
    pub choose_folder: bool,
    pub new_script: bool,
    pub duplicate: bool,
    pub rename: bool,
    pub delete: bool,

    /// The mod panel's own answers, recorded by [`crate::editor::mod_panel::draw`] into the same
    /// struct: the two panels are drawn by one shell pass and their clicks land in one place.
    pub mod_choose_folder: bool,
    pub mod_detect: bool,
    pub mod_install: bool,
    pub mod_uninstall: bool,
}

pub fn draw(ui: &Ui, editor: &Editor, actions: &mut WorkspaceActions) {
    ui.text("Scripts");

    if ui.button("Open folder…") {
        actions.choose_folder = true;
    }

    match &editor.folder {
        Some(folder) => ui.text_wrapped(&folder.display().to_string()),
        None => ui.text_wrapped("No folder open"),
    }

    let listing = &editor.listing;
    let count = listing.scripts.len();
    let list_height = (ui.content_region_avail()[1] - 76.0).max(80.0);

    ui.child_window("##scripts")
        .size([0.0, list_height])
        .build(ui, || {
            if listing.scripts.is_empty() {
                ui.text("No scripts in the folder.");
                return;
            }

            for (index, script) in listing.scripts.iter().enumerate() {
                let selected = editor.selected.as_deref() == Some(script.path.as_path());
                let unsaved = buffers::is_dirty(&editor.buffers, script);
                let label = row_label(script, unsaved);

                // The file name is the row's identity in ImGui: two scripts can share a visible
                // name across folders, and a selectable's id must not.
                let id = format!("{}##script-row-{index}", label);

                if ui.selectable_config(id.as_str()).selected(selected).build() {
                    actions.select = Some(index);
                }
            }
        });

    let has_folder = editor.folder.is_some();
    let has_selection = editor.selected.is_some();

    ui.separator();
    {
        let _disabled = ui.begin_disabled_with_cond(!has_folder);
        if ui.button("New") {
            actions.new_script = true;
        }
    }

    ui.same_line();

    // The buttons are still drawn when they cannot act, so the row does not reflow; ImGui's
    // disabled state is what says they are not live, rather than the control vanishing.
    {
        let _disabled = ui.begin_disabled_with_cond(!(has_folder && has_selection));
        if ui.button("Duplicate") {
            actions.duplicate = true;
        }

        ui.same_line();
        if ui.button("Rename") {
            actions.rename = true;
        }

        ui.same_line();
        if ui.button("Delete") {
            actions.delete = true;
        }
    }

    ui.separator();
    match &editor.message {
        Some(message) => ui.text_wrapped(message),
        None => match &listing.error {
            Some(error) => ui.text_wrapped(error),
            None => {
                if editor.folder.is_none() {
                    ui.text_wrapped("Pick a folder to work on. Only .tas files are scripts here.");
                } else if count == 1 {
                    ui.text_wrapped("1 script in the folder");
                } else {
                    ui.text_wrapped(&format!("{count} scripts in the folder"));
                }
            }
        },
    }
}

/// One row: the script's name, then what the file says — its frame count, the parser's refusal, or
/// the unsaved marker.
///
/// The frame count is what the *listing* read out of the file; the unsaved marker says the editor
/// holds something the file does not. Both together are the row's whole story, in the two lines the
/// panel has for it.
fn row_label(script: &crate::workspace::ScriptEntry, unsaved: bool) -> String {
    let mark = if unsaved { "unsaved · " } else { "" };

    match &script.error {
        Some(error) => format!("{}\n{mark}{error}", script.name),
        None => format!("{}\n{mark}{} frames", script.name, script.frames),
    }
}

/// The scripts panel as a value — the facts one frame of it paints, gathered so the drawing code and
/// any caller read the same thing. [`draw`] takes the editor directly; this is the record a caller
/// builds when it wants the facts without the editor.
pub struct WorkspacePaneView<'a> {
    pub folder: Option<&'a std::path::Path>,
    pub listing: &'a Listing,
    pub buffers: &'a Buffers,
    pub selected: Option<&'a std::path::Path>,
    pub message: Option<&'a str>,
    pub mod_panel: ModPanelView<'a>,
    pub pending_folder: PendingFolder,
}
