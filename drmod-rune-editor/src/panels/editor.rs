//! The script text: the `.tas` on screen, coloured, with what it currently says above it.
//!
//! The text editor is Rune's `TextArea`, whose field type comes from the kit — so the editor gets a
//! kit of its own with its `ui` face swapped for the mono one. Everything else about the field is
//! Rune's: wrapping, the caret, selection, undo, IME and scrolling.

use rune::{El, col, row, text};
use rune_kit::TextArea;

use crate::app::App;

pub fn panel(app: &App) -> El {
    let k = &app.kit;
    let region_h =
        (app.body_h - super::clamp_table(app.table_h, app.body_h) - super::DIVIDER).max(140.0);
    let text_h = (region_h - 62.0).max(80.0);
    let line = 17.5;
    let rows = ((text_h - 16.0) / line).max(6.0) as usize;

    let name = app
        .selected_entry()
        .map(|entry| format!("{}.tas", entry.name))
        .unwrap_or_else(|| "Нет скрипта".to_owned());
    let dirty = app.dirty();

    let header = row()
        .w_full()
        .h(30.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .child(
            text(name)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate(),
        )
        .child(
            text(if dirty {
                "есть изменения"
            } else {
                "сохранено"
            })
            .font(k.ty.small)
            .color(if dirty {
                k.c.text_accent
            } else {
                k.c.text_faint
            }),
        )
        .child(rune::spacer())
        .child(
            text(format!("{} строк", app.line_count()))
                .font(k.ty.small)
                .color(k.c.text_faint),
        );

    let status_color = if app.status.is_ok() {
        k.c.text_muted
    } else {
        k.c.danger.fg
    };
    let status = row().w_full().px(10.0).pb(4.0).child(
        text(app.script_summary())
            .font(k.ty.small)
            .color(status_color)
            .wrap()
            .w_full(),
    );

    let body = if app.selected.is_none() {
        col().w_full().h(text_h).center().child(
            text("Выберите скрипт слева или создайте новый.")
                .font(k.ty.ui)
                .color(k.c.text_muted),
        )
    } else {
        // The field's own kit: the same palette and controls, with its body face replaced by the
        // mono one. `TextArea` takes its style from `ty.ui`, so this is the whole change.
        let mut code = k.clone();
        code.ty.ui = k.ty.mono;
        code.ty.ui_medium = k.ty.mono;

        TextArea::new(&app.text)
            .label("Текст скрипта")
            .width(None)
            .auto_grow(rows, rows)
            .max_height(text_h)
            .decorations(app.spans.clone())
            .on_edit(|s: &mut App| &mut s.text)
            .on_change(|s: &mut App, _tx| s.text_changed())
            .on_key(|s: &mut App, event: &rune::KeyEvent, _tx: &mut rune::Tx| {
                if event.is_command('s') {
                    s.save();
                    true
                } else {
                    false
                }
            })
            .build(&code)
            .key("script-text")
    };

    col()
        .key("editor")
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(k.c.layer_01)
        .child(header)
        .child(status)
        .child(body)
}
