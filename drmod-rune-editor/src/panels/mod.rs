//! The window: a toolbar, the script sidebar, the editor with its frames table, the run inspector,
//! and the dialogs.

mod dialogs;
mod editor;
mod frames;
mod run;
mod sidebar;

use rune::Cx;
use rune::{Cursor, Drag, El, Phase, Tx, col, el};
use rune_kit::shell::{SplitView, Toolbar};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit};

use crate::app::App;
use crate::app::actions;

pub fn view(app: &App, cx: &mut Cx<App>) -> El {
    let k = &app.kit;
    let mut overlays = Vec::new();

    let split = SplitView::new(main(app))
        .toolbar(toolbar(app, k))
        .sidebar(
            &app.sidebar,
            |s: &mut App| &mut s.sidebar,
            sidebar::panel(app),
        )
        .inspector(
            &app.inspector,
            |s: &mut App| &mut s.inspector,
            run::panel(app),
        )
        .titlebar_inset(cx.titlebar_inset())
        .reduced_motion(cx.is_reduced_motion())
        .build(k);

    dialogs::build(app, k, &mut overlays);
    rune_kit::root(split, overlays)
}

fn toolbar(app: &App, k: &Kit) -> Toolbar {
    Toolbar::new()
        .leading(
            Button::new("Папка…")
                .control_size(ControlSize::Small)
                .variant(ButtonVariant::Ghost)
                .on_press_tx(|s: &mut App, tx: &mut Tx| actions::ask_folder(s, tx))
                .build(k)
                .key("open-folder"),
        )
        .leading(
            Button::new("Сохранить")
                .control_size(ControlSize::Small)
                .disabled(!app.dirty())
                .on_press(|s: &mut App| s.save())
                .build(k)
                .key("save"),
        )
        .trailing(
            Button::new("Команды")
                .control_size(ControlSize::Small)
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| s.commands_open = true)
                .build(k)
                .key("open-commands"),
        )
        .trailing(
            Button::new("Мод")
                .control_size(ControlSize::Small)
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| s.mod_open = true)
                .build(k)
                .key("open-mod"),
        )
        .trailing(
            Button::new(if app.dark {
                "Светлая"
            } else {
                "Тёмная"
            })
            .control_size(ControlSize::Small)
            .variant(ButtonVariant::Ghost)
            .on_press(|s: &mut App| s.toggle_theme())
            .build(k)
            .key("toggle-theme"),
        )
}

fn main(app: &App) -> El {
    let k = &app.kit;
    let table_h = clamp_table(app.table_h, app.body_h);

    col()
        .key("main")
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(k.c.bg_base)
        .child(
            col()
                .key("main-body")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .on_layout_tx(|s: &mut App, rect: [f32; 4], tx: &mut Tx| {
                    let height = rect[3];
                    if (s.body_h - height).abs() > 0.5 {
                        s.body_h = height;
                        s.table_h = clamp_table(s.table_h, height);
                    } else {
                        tx.live_only();
                    }
                })
                .child(
                    col()
                        .key("editor-slot")
                        .w_full()
                        .grow(1.0)
                        .min_h(140.0)
                        .child(editor::panel(app)),
                )
                .child(divider(app))
                .child(frames::panel(app, table_h)),
        )
}

const DIVIDER: f32 = 7.0;

fn clamp_table(table: f32, body: f32) -> f32 {
    table.clamp(120.0, (body - 220.0).max(120.0))
}

fn divider(app: &App) -> El {
    let k = &app.kit;
    el().key("table-divider")
        .w_full()
        .h(DIVIDER)
        .cursor(Cursor::ResizeRow)
        .on_drag(
            |s: &mut App, drag: &mut Drag, tx: &mut Tx| match drag.phase {
                Phase::Down => drag.data = s.table_h,
                Phase::Move => {
                    let max = (s.body_h - 220.0).max(120.0);
                    s.table_h = (drag.data - drag.dy).clamp(120.0, max);
                    tx.instant();
                }
                Phase::Up => {}
            },
        )
        .child(
            el().key("line")
                .absolute(0.0, 3.0)
                .w_full()
                .h(1.0)
                .fill(k.c.border_base),
        )
        .child(
            el().key("grip")
                .absolute(0.0, 2.0)
                .w_full()
                .h(3.0)
                .radius(1.5)
                .fill(k.c.border_muted)
                .opacity(0.35),
        )
}

/// The editor's accessible/window title, shared by the shell and the title bar.
pub const TITLE: &str = "TAS Editor (Rune)";

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn the_shell_builds_and_its_panels_are_there() {
        let mut rt = Runtime::new(App::new(), view, (1440.0, 900.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt.frame(1.0 / 60.0);

        for key in ["script-rows", "editor", "frames-panel", "run-panel"] {
            assert!(rt.find(key).is_some(), "{key} is in the tree");
        }
    }
}
