//! The installable mod, and what installing it would do — a panel of its own, like every other
//! section of the shell: two panes in one column are just a splitter apart, and the install is a
//! different job from the script list beside it (a write into the game folder, not a list of files).
//!
//! The panel takes its facts through [`crate::editor::workspace_panel::WorkspaceActions`] — the one
//! action record the shell passes around — and reads them from the editor, so the drawing code and
//! any caller see the same values.
//!
//! ⚠️ The button's word follows the state rather than being one fixed label: `Install` when the game
//! has no plugin, `Reinstall` when it has this same build, `Update` when it has a different one. A
//! button that says `Install` over a folder that already holds the mod is a button that does not say
//! what it will do.

use dear_app::imgui::Ui;

use crate::editor::workspace_panel::WorkspaceActions;
use crate::editor::Editor;
use crate::mod_install::{ModState, ASI_NAME, LOADER_PATH, LOADER_URL, PLUGIN_DIR};

/// The install panel: where the game is, what its folder holds, and the one button that changes it.
pub fn draw(ui: &Ui, editor: &Editor, actions: &mut WorkspaceActions) {
    let view = editor.mod_view();

    ui.text("Mod");

    if ui.button("Game folder…") {
        actions.mod_choose_folder = true;
    }

    ui.same_line();
    {
        let _disabled = ui.begin_disabled_with_cond(view.busy);
        if ui.button("Detect") {
            actions.mod_detect = true;
        }
    }

    match view.game_folder {
        Some(folder) => ui.text_wrapped(&folder.display().to_string()),
        None => ui.text_wrapped("No game folder found"),
    }

    ui.text_wrapped(&view.state_note());

    let loader = view.loader_note();
    if !loader.is_empty() {
        ui.text_wrapped(&loader);
    }

    ui.separator();
    {
        let _disabled = ui.begin_disabled_with_cond(!view.can_install());
        if ui.button(view.install_label()) {
            actions.mod_install = true;
        }
    }

    ui.same_line();
    {
        let _disabled = ui.begin_disabled_with_cond(!(view.can_remove && !view.busy));
        if ui.button("Uninstall") {
            actions.mod_uninstall = true;
        }
    }

    if view.busy {
        ui.same_line();
        ui.text("working…");
    }

    if let Some(message) = view.message {
        ui.text_wrapped(message);
    } else {
        ui.text_wrapped(&format!(
            "Installing copies {PLUGIN_DIR}\\{ASI_NAME} ({} KiB) and {LOADER_PATH} ({} KiB) into the \
             game; the game loads them on its next start.",
            view.payload_asi_kib, view.payload_loader_kib
        ));
        ui.text_wrapped(LOADER_URL);
    }
}

/// What the mod panel paints. Borrowed, not owned: it is read once per frame and never stored.
#[derive(Clone, Debug)]
pub struct ModPanelView<'a> {
    pub game_folder: Option<&'a std::path::Path>,
    pub state: ModState,
    pub loader_present: bool,
    pub loader_ours: bool,
    pub can_remove: bool,
    pub message: Option<&'a str>,
    pub busy: bool,
    /// The embedded payload's sizes, shown so an author can tell at a glance whether the build they
    /// just made is the one the editor is carrying.
    pub payload_asi_kib: usize,
    pub payload_loader_kib: usize,
}

impl ModPanelView<'_> {
    /// Whether an install click can do anything: there is a folder to write to and no install already
    /// in flight.
    ///
    /// There is no "payload missing" case to check: the payload is embedded by the build script, so a
    /// running editor always has one.
    ///
    /// `Installed` is still live: the same button is how a user re-lays the files after deleting
    /// one of them by hand, and the word says `Reinstall` so the click is honest.
    pub fn can_install(&self) -> bool {
        !self.busy && self.state != ModState::NoGameFolder
    }

    /// The install button's word, which follows the state.
    pub fn install_label(&self) -> &'static str {
        match self.state {
            ModState::Installed => "Reinstall",
            ModState::OtherVersion => "Update",
            _ => "Install",
        }
    }

    /// The loader's own line: a separate fact from the plugin's, because the plugin does not need
    /// *our* loader — so a foreign `d3d9.dll` is news, not a problem.
    pub fn loader_note(&self) -> String {
        use crate::mod_install::LOADER_PATH;

        if self.state == ModState::NoGameFolder {
            return String::new();
        }

        if !self.loader_present {
            return format!("No {LOADER_PATH} in the game folder — installing adds one.");
        }

        if self.loader_ours {
            format!("This build's {LOADER_PATH} is already there.")
        } else {
            format!(
                "A different {LOADER_PATH} is there (ReShade, an ENB, another mod). \
                 It will be kept — any ASI loader loads this plugin."
            )
        }
    }

    /// What the folder holds, as the one line the pane exists for.
    pub fn state_note(&self) -> String {
        match self.state {
            ModState::NoGameFolder => format!(
                "Point the editor at the folder holding {}.",
                crate::steam::GAME_EXE_NAME
            ),
            ModState::NotInstalled => "The mod is not installed.".to_owned(),
            ModState::Installed => "The mod is installed and up to date.".to_owned(),
            ModState::OtherVersion => "A different build of the mod is installed.".to_owned(),
        }
    }
}
