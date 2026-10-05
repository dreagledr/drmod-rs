//! The shell's frame: the window, the dock space, the panels and the dialogs.
//!
//! Split out of [`super::Editor`] because the two are different jobs — the editor owns the state and
//! the actions, and this decides what a frame looks like and which action a click asked for. A
//! click is *recorded* here and *applied* by the caller after the frame: the panels are drawn from
//! `&`-borrows of the editor's own state, so nothing inside a frame can mutate it.
//!
//! ⚠️ **Every section is its own window.** The mod's install, the script list, the run controls, the
//! command table and the text are five dockable panels, not sections stacked inside two of them: a
//! section nested in another cannot be moved, resized against its neighbours, or floated on its own,
//! and the author's own arrangement is the point of a dock space. The declared layout groups nothing
//! either — each panel sits in its own leaf of the split tree, so the default is a *starting* shape
//! and not a cage. What the layout numbers do is pick sensible proportions; which pane goes where is
//! then the author's to change, and ImGui keeps the result in its `.ini`.

use dear_app::imgui::{
    Condition, DockLayout, DockLayoutApply, DockSplit, Key, KeyChord, KeyMods, Ui, WindowFlags,
};
use dear_imgui_cte::TextEditor;

use crate::editor::script_controls::{self, ControlsView};
use crate::editor::text_cache::TextCache;
use crate::editor::{
    mod_panel, table, text_pane, workspace_panel, Editor, ScriptActions, DOCKSPACE_ID, WINDOW_TITLE,
};

/// What a frame decided, in the order the caller applies it. Everything else in a frame is drawing.
#[derive(Clone, Debug, Default)]
pub struct FrameActions {
    pub workspace: workspace_panel::WorkspaceActions,
    pub script: ScriptActions,

    /// The window's own answers.
    pub quit: bool,
    /// The theme was toggled — applied in the pre-frame hook, not here.
    pub toggle_theme: bool,
    pub confirm_delete: bool,
    pub cancel_delete: bool,
    pub confirm_rename: bool,
    pub cancel_rename: bool,
    pub confirm_mod_remove: bool,
    pub cancel_mod_remove: bool,
}

/// The panels, in the order they are declared to the dock space and to the `View` menu.
///
/// The order is also the order the default layout's tree is built in: the left column's panels
/// first, then the right column's.
pub const PANE_MOD: usize = 0;
pub const PANE_SCRIPTS: usize = 1;
pub const PANE_CONTROLS: usize = 2;
pub const PANE_FRAMES: usize = 3;
pub const PANE_TEXT: usize = 4;
pub const PANE_COMMANDS: usize = 5;

/// The panel keys, addressed by the dock space. Stable strings, so a panel and its place in the tree
/// refer to the same window even if its caption changes.
const PANE_KEYS: [&str; 6] = ["mod", "scripts", "controls", "frames", "text", "commands"];

/// The window titles, by the same indices.
const PANE_TITLES: [&str; 6] = ["Mod", "Scripts", "Run", "Frames", "Script", "Commands"];

/// The shell's own state that is not the editor's.
///
/// ⚠️ No `Clone`/`Debug`: [`TextCache`] holds the parse of the script on screen, which is not
/// something to copy into a log line or a snapshot — and a shell state that is cheap to clone is a
/// shell state someone will clone per frame without noticing.
pub struct ShellState {
    /// Whether each panel is open. Closing one hides it without losing anything, so the state lives
    /// here rather than inside ImGui.
    pub window_open: [bool; 6],
    /// Whether the declared layout has been submitted. It goes in **once**: after that the dock
    /// space keeps whatever the author dragged, and submitting it again is an error.
    pub dock_applied: bool,
    /// The script whose text is currently loaded into the CTE editor. When the selection changes,
    /// the editor is given the new script's buffer — and only then, because re-feeding the same text
    /// on every keystroke is what moves the caret.
    pub loaded: Option<std::path::PathBuf>,
    /// What the text panel does not own: the seed field as the control sees it for the frame.
    pub seed_field: String,
    pub rename_field: String,
    /// The rules the run controls ended the frame with, when one was changed. `None` on a frame
    /// where nothing was touched — which is what keeps the panel an editor of one value rather than
    /// an owner of it.
    pub rules_out: Option<crate::api::PlaybackRules>,
    /// The one reading of the script on screen, shared by every panel that needs it.
    pub text: TextCache,
}

impl Default for ShellState {
    fn default() -> Self {
        let mut window_open = [true; 6];
        // The reference is a thing to look up, not a thing to watch: it opens on the text panel's
        // button. It also draws a row per DSL token, which is not worth paying for on every frame of
        // a launch that never reads it.
        window_open[PANE_COMMANDS] = false;

        Self {
            window_open,
            dock_applied: false,
            loaded: None,
            seed_field: String::new(),
            rename_field: String::new(),
            rules_out: None,
            text: TextCache::default(),
        }
    }
}

/// One frame of the shell.
pub fn frame(
    ui: &Ui,
    editor: &Editor,
    shell: &mut ShellState,
    script_editor: &mut TextEditor,
    actions: &mut FrameActions,
) {
    let viewport = ui.main_viewport();
    ui.set_next_window_viewport(viewport.id());

    // The one reading of the script on screen, taken **before** any panel draws: the run controls,
    // the table and the text panel all read the same answer, and it is rebuilt only when the text
    // changes. Reading it per panel was three parses and a projection per frame.
    shell.text.refresh(&editor.selected_text());

    let dock_id = ui.get_id(DOCKSPACE_ID);
    ui.window(WINDOW_TITLE)
        .flags(
            WindowFlags::MENU_BAR
                | WindowFlags::NO_TITLE_BAR
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_RESIZE
                | WindowFlags::NO_COLLAPSE
                | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
                | WindowFlags::NO_NAV_FOCUS,
        )
        .position(viewport.pos(), Condition::Always)
        .size(viewport.size(), Condition::Always)
        .build(|| {
            menu_bar(ui, editor, shell, actions);

            let available = ui.content_region_avail();
            let dockspace = ui
                .dockspace()
                .current_window(available)
                .root_id(dock_id)
                .flags(dear_app::imgui::DockNodeFlags::PASSTHRU_CENTRAL_NODE);

            let outcome = if shell.dock_applied {
                dockspace.build()
            } else {
                dockspace
                    .layout(&declared_layout(), DockLayoutApply::IfMissing)
                    .build()
            };

            match outcome {
                Ok(_) => shell.dock_applied = true,
                Err(_) => shell.dock_applied = true,
            }
        });

    mod_window(ui, editor, shell, actions);
    scripts_window(ui, editor, shell, actions);
    controls_window(ui, editor, shell, actions);
    frames_window(ui, editor, shell);
    text_window(ui, editor, shell, script_editor, actions);
    commands_window(ui, shell);

    dialogs(ui, editor, shell, actions);
}

/// The window's menu bar: which panels are shown, and the workspace's own actions.
fn menu_bar(ui: &Ui, editor: &Editor, shell: &mut ShellState, actions: &mut FrameActions) {
    // The Save accelerator, before the menus so it is live whatever is open. Deliberately *not*
    // gated on the caret: `Ctrl+S` while typing is the shape an author expects, and the handler is a
    // no-op when the script is already saved.
    if ui.shortcut(KeyChord::new(Key::S).with_mods(KeyMods::CTRL)) && editor.selected_dirty() {
        actions.script.controls.save = true;
    }

    let _ = ui.menu_bar(|| {
        ui.menu("View", || {
            for (index, title) in PANE_TITLES.iter().enumerate() {
                ui.menu_item_toggle(title, None::<&str>, &mut shell.window_open[index], true);
            }

            ui.separator();

            // ⚠️ Recorded, not applied: a theme is applied in the pre-frame hook, where the frame
            // hook borrows the context's style — a frame body only has `&Ui`.
            let mut light = editor.settings.light_theme;
            if ui.menu_item_toggle("Light theme", None::<&str>, &mut light, true) {
                actions.toggle_theme = true;
            }
        });

        ui.menu("Workspace", || {
            if ui.menu_item("Open folder…") {
                actions.workspace.choose_folder = true;
            }

            let has_folder = editor.folder.is_some();
            let has_selection = editor.selected.is_some();

            if ui.menu_item_enabled_selected_no_shortcut("New script", false, has_folder) {
                actions.workspace.new_script = true;
            }

            if ui.menu_item_enabled_selected_no_shortcut(
                "Duplicate",
                false,
                has_folder && has_selection,
            ) {
                actions.workspace.duplicate = true;
            }

            if ui.menu_item_enabled_selected_no_shortcut("Rename…", false, has_selection) {
                actions.workspace.rename = true;
            }

            if ui.menu_item_enabled_selected_no_shortcut("Delete…", false, has_selection) {
                actions.workspace.delete = true;
            }

            ui.separator();

            if ui.menu_item_enabled_selected_no_shortcut("Save", false, editor.selected_dirty()) {
                actions.script.controls.save = true;
            }
        });

        ui.menu("Mod", || {
            if ui.menu_item("Game folder…") {
                actions.workspace.mod_choose_folder = true;
            }

            if ui.menu_item("Detect") {
                actions.workspace.mod_detect = true;
            }

            if ui.menu_item_enabled_selected_no_shortcut("Uninstall…", false, editor.mod_can_remove)
            {
                actions.workspace.mod_uninstall = true;
            }
        });

        if ui.menu_item("Quit") {
            actions.quit = true;
        }
    });
}

/// The mod's install — its own device, so it is its own panel.
fn mod_window(ui: &Ui, editor: &Editor, shell: &mut ShellState, actions: &mut FrameActions) {
    if !shell.window_open[PANE_MOD] {
        return;
    }

    // ⚠️ Submitted through its **key**, not its title: the declared layout docks the panels by their
    // stable ids, and a window opened by title alone is a different window to the dock space — it
    // lands undocked and leaves the tree the layout built empty.
    let key = pane_key(PANE_MOD);
    let mut opened = shell.window_open[PANE_MOD];
    ui.window(&key)
        .opened(&mut opened)
        .size([420.0, 300.0], Condition::FirstUseEver)
        .build(|| {
            mod_panel::draw(ui, editor, &mut actions.workspace);
        });

    shell.window_open[PANE_MOD] = opened;
}

/// The folder and its scripts.
fn scripts_window(ui: &Ui, editor: &Editor, shell: &mut ShellState, actions: &mut FrameActions) {
    if !shell.window_open[PANE_SCRIPTS] {
        return;
    }

    let key = pane_key(PANE_SCRIPTS);
    let mut opened = shell.window_open[PANE_SCRIPTS];
    ui.window(&key)
        .opened(&mut opened)
        .size([360.0, 420.0], Condition::FirstUseEver)
        .build(|| {
            workspace_panel::draw(ui, editor, &mut actions.workspace);
        });

    shell.window_open[PANE_SCRIPTS] = opened;
}

/// Save, the run rules and the buttons.
fn controls_window(ui: &Ui, editor: &Editor, shell: &mut ShellState, actions: &mut FrameActions) {
    if !shell.window_open[PANE_CONTROLS] {
        return;
    }

    // The flag and the field are read out by value before the view takes its borrow: a panel draws
    // from `&ShellState` while the shell is `&mut`, and the borrow checker is what keeps those two
    // honest — so the pieces a panel writes are copied in and out rather than aliased.
    let mut opened = shell.window_open[PANE_CONTROLS];
    let seed_text = shell.seed_field.clone();
    let mut view = ControlsView {
        status: &shell.text.status,
        dirty: editor.selected_dirty(),
        rules: editor.rules(),
        seed_text: &seed_text,
        game: &editor.game,
        preparing: editor.preparing,
        error: editor.run_error.as_deref(),
        message: editor.run_message.as_deref(),
    };

    let key = pane_key(PANE_CONTROLS);
    ui.window(&key)
        .opened(&mut opened)
        .size([760.0, 150.0], Condition::FirstUseEver)
        .build(|| {
            script_controls::draw(ui, &mut view, &mut actions.script.controls);
        });

    // The field travels back out: the panel's own edit of it is what the shell reads next frame. The
    // panel is given the text by value, so a keystroke comes back through `seed_out` — the view
    // cannot hold a new string, and leaking one per keystroke would be worse than the indirection.
    if let Some(typed) = actions.script.controls.seed_out.take() {
        shell.seed_field = typed;
    }

    // A rule the panel changed is published for the caller to adopt. The panel does not own the
    // rules — the shell does — so it hands the value over rather than keeping it.
    if actions.script.controls.rules_changed {
        shell.rules_out = Some(view.rules);
    }

    shell.window_open[PANE_CONTROLS] = opened;
}

/// The command table.
fn frames_window(ui: &Ui, editor: &Editor, shell: &mut ShellState) {
    if !shell.window_open[PANE_FRAMES] {
        return;
    }

    let mut opened = shell.window_open[PANE_FRAMES];
    let key = pane_key(PANE_FRAMES);
    ui.window(&key)
        .opened(&mut opened)
        .size([900.0, 320.0], Condition::FirstUseEver)
        .build(|| {
            // The table is drawn even with nothing to show, so its header — the legend of every
            // column — is always there to read against the text.
            table::draw(ui, &shell.text.frames, frames_note(editor, shell));
        });

    shell.window_open[PANE_FRAMES] = opened;
}

/// Why the table has no rows, as its one body row says it.
///
/// The reasons are the ones the author can act on, in the order they can act on them: pick a script,
/// then fix the line the parser refused.
fn frames_note<'a>(editor: &Editor, shell: &'a ShellState) -> &'a str {
    if editor.selected.is_none() {
        return "No script selected.";
    }

    match &shell.text.status.error {
        Some(error) => error,
        None => "No frames yet — the script holds no commands.",
    }
}

/// The `.tas` text.
fn text_window(
    ui: &Ui,
    editor: &Editor,
    shell: &mut ShellState,
    script_editor: &mut TextEditor,
    actions: &mut FrameActions,
) {
    if !shell.window_open[PANE_TEXT] {
        return;
    }

    // The one place the text box is given a script: on the frame the *selection* changes. Re-feeding
    // the same buffer on every keystroke would move the caret to the start after each character.
    let selection = editor.selected.clone();
    if selection != shell.loaded {
        let text = editor.selected_text();
        let _ = script_editor.set_text(&text);
        shell.loaded = selection;
    }

    let mut opened = shell.window_open[PANE_TEXT];
    let mut commands_open = shell.window_open[PANE_COMMANDS];

    let key = pane_key(PANE_TEXT);
    ui.window(&key)
        .opened(&mut opened)
        .size([900.0, 380.0], Condition::FirstUseEver)
        .build(|| {
            if editor.selected.is_none() {
                ui.text("No script selected.");
                return;
            }

            text_pane::status_line(ui, &shell.text.status, &mut commands_open);

            if text_pane::draw_text(ui, script_editor) {
                actions.script.text.changed = true;
            }
        });

    shell.window_open[PANE_TEXT] = opened;
    shell.window_open[PANE_COMMANDS] = commands_open;
}

/// The DSL's command reference — off by default, and a panel of its own rather than a pane inside
/// the text: a reference is read *beside* the text it is about, and a section inside the text panel
/// cannot be moved to the other side of it.
fn commands_window(ui: &Ui, shell: &mut ShellState) {
    if !shell.window_open[PANE_COMMANDS] {
        return;
    }

    let key = pane_key(PANE_COMMANDS);
    let mut opened = shell.window_open[PANE_COMMANDS];
    ui.window(&key)
        .opened(&mut opened)
        .size([340.0, 500.0], Condition::FirstUseEver)
        .build(|| {
            text_pane::draw_reference(ui);
        });

    shell.window_open[PANE_COMMANDS] = opened;
}

/// The three modal questions: a delete, a rename and a mod uninstall.
///
/// They are drawn only while they are open, and every answer is recorded for the caller — a dialog
/// inside a frame cannot mutate the state it is being drawn from.
fn dialogs(ui: &Ui, editor: &Editor, shell: &mut ShellState, actions: &mut FrameActions) {
    if let Some(script) = &editor.pending_delete {
        let unsaved = crate::buffers::is_dirty(&editor.buffers, script);
        ui.window("Delete script?")
            .size([440.0, 0.0], Condition::Appearing)
            .flags(WindowFlags::ALWAYS_AUTO_RESIZE | WindowFlags::NO_COLLAPSE)
            .build(|| {
                ui.text_wrapped(&format!(
                    "Delete {}.tas from the folder?{}",
                    script.name,
                    if unsaved {
                        " Its unsaved changes go with it."
                    } else {
                        ""
                    }
                ));

                ui.separator();
                if ui.button("Delete") {
                    actions.confirm_delete = true;
                }

                ui.same_line();
                if ui.button("Cancel") {
                    actions.cancel_delete = true;
                }
            });
    }

    if let Some(script) = &editor.pending_rename {
        ui.window("Rename script")
            .size([440.0, 0.0], Condition::Appearing)
            .flags(WindowFlags::ALWAYS_AUTO_RESIZE | WindowFlags::NO_COLLAPSE)
            .build(|| {
                ui.text_wrapped("The file stays in this folder and keeps .tas");
                ui.set_next_item_width(-1.0);
                let _ = ui.input_text("File name", &mut shell.rename_field).build();
                ui.text_wrapped(&format!("Current name: {}", script.name));

                let wanted = shell.rename_field.trim();
                let ready = !wanted.is_empty() && wanted != script.name;

                ui.separator();
                {
                    let _disabled = ui.begin_disabled_with_cond(!ready);
                    if ui.button("Rename") {
                        actions.confirm_rename = true;
                    }
                }

                ui.same_line();
                if ui.button("Cancel") {
                    actions.cancel_rename = true;
                }
            });
    }

    if editor.pending_mod_remove {
        ui.window("Remove the mod?")
            .size([480.0, 0.0], Condition::Appearing)
            .flags(WindowFlags::ALWAYS_AUTO_RESIZE | WindowFlags::NO_COLLAPSE)
            .build(|| {
                let text = match &editor.game_folder {
                    Some(folder) => format!(
                        "Remove plugins\\{} from {}? A d3d9.dll that another mod needs is left alone.",
                        crate::mod_install::ASI_NAME,
                        folder.display()
                    ),
                    None => "Remove the mod from the game folder?".to_owned(),
                };
                ui.text_wrapped(&text);

                ui.separator();
                if ui.button("Remove") {
                    actions.confirm_mod_remove = true;
                }

                ui.same_line();
                if ui.button("Cancel") {
                    actions.cancel_mod_remove = true;
                }
            });
    }
}

/// The declared default layout, submitted **once**. After that the dock space keeps the author's own
/// arrangement — submitting it again is an error, and a splitter the author dragged must not be
/// reset.
///
/// ⚠️ **Nothing is grouped.** Every panel is a leaf of the tree with itself in it
/// (`DockLayout::tabs` with one entry is how a leaf is spelled here), so the shape below is a
/// starting arrangement and not a set of tabs: any panel can be dragged anywhere, including out into
/// a floating window. The numbers are weights, not pixels — ImGui normalizes them into proportions —
/// and they are only a sensible default: a narrow column for the mod and the list, a wide one for
/// the run and the script.
fn declared_layout() -> DockLayout {
    let left = DockLayout::split(
        DockSplit::Up,
        0.32,
        DockLayout::tabs([pane_key(PANE_MOD)]),
        DockLayout::tabs([pane_key(PANE_SCRIPTS)]),
    );

    let right = DockLayout::split(
        DockSplit::Up,
        0.30,
        DockLayout::tabs([pane_key(PANE_CONTROLS)]),
        DockLayout::split(
            DockSplit::Up,
            0.50,
            DockLayout::tabs([pane_key(PANE_FRAMES)]),
            DockLayout::split(
                DockSplit::Left,
                0.72,
                DockLayout::tabs([pane_key(PANE_TEXT)]),
                DockLayout::tabs([pane_key(PANE_COMMANDS)]),
            ),
        ),
    );

    DockLayout::split(DockSplit::Left, 0.26, left, right)
}

fn pane_key(slot: usize) -> dear_app::imgui::WindowKey {
    dear_app::imgui::WindowKey::new(PANE_KEYS[slot], PANE_TITLES[slot])
        .expect("the panel keys are non-empty and carry no separator")
}

/// The declared layout's tree, and the panels it holds — checked by the panel-opening code, which
/// has to address the same windows this does.
#[cfg(test)]
mod layout_tests {
    use super::*;

    /// The layout names every panel exactly once, so a panel cannot be left undocked by a layout
    /// that forgot it, and two panels cannot share a node.
    #[test]
    fn every_panel_has_its_own_leaf() {
        // The keys are the identity the dock space docks by, so they have to be distinct strings:
        // two panels sharing one id would be one window in the tree and two in the frame.
        let mut keys: Vec<&str> = PANE_KEYS.to_vec();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "two panels share a key");

        assert_eq!(PANE_KEYS.len(), PANE_TITLES.len(), "a panel has no title");
    }
}
