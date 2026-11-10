//! The script list: the workspace's `.tas` files, and the file operations over them.

use rune::{El, ScrollSpec, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use crate::app::App;
use crate::app::actions;

pub fn panel(app: &App) -> El {
    let k = &app.kit;
    let mut list = col()
        .key("scripts")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(8.0)
        .gap(2.0);

    let heading = row()
        .w_full()
        .h(24.0)
        .px(4.0)
        .center_y()
        .child(
            text("СКРИПТЫ")
                .font(k.ty.small_medium)
                .color(k.c.text_faint)
                .grow(1.0),
        )
        .child(
            text(
                app.folder
                    .as_ref()
                    .and_then(|folder| folder.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "—".to_owned()),
            )
            .font(k.ty.small)
            .color(k.c.text_faint)
            .truncate(),
        );

    list = list.child(heading);

    if app.folder.is_none() {
        list = list
            .child(
                text("Папка не выбрана")
                    .font(k.ty.ui)
                    .color(k.c.text_muted)
                    .mt(8.0),
            )
            .child(
                text("Выберите папку, чтобы увидеть её .tas файлы.")
                    .font(k.ty.small)
                    .color(k.c.text_faint)
                    .wrap()
                    .w_full(),
            )
            .child(
                Button::new("Выбрать папку…")
                    .control_size(ControlSize::Small)
                    .on_press_tx(|s: &mut App, tx: &mut Tx| actions::ask_folder(s, tx))
                    .build(k)
                    .mt(10.0),
            );
        return list;
    }

    if let Some(error) = &app.listing.error {
        list = list.child(
            text(error)
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap()
                .w_full(),
        );
    }

    let mut rows = col().key("script-rows").w_full().gap(1.0);
    for entry in &app.listing.scripts {
        let selected = app.selected.as_deref() == Some(entry.path.as_path());
        let dirty = crate::buffers::is_dirty(&app.buffers, entry);
        let path = entry.path.clone();

        let marker = if entry.error.is_some() {
            Some(("!", k.c.danger.fg))
        } else if dirty {
            Some(("●", k.c.text_accent))
        } else {
            None
        };

        let mut item = row()
            .key(format!("script-{}", entry.name))
            .w_full()
            .h(34.0)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .radius(k.radius.sm)
            .fill(if selected {
                k.c.hover
            } else {
                rune::Color::TRANSPARENT
            })
            .hover(k.c.hover)
            .on_press(move |s: &mut App| s.select(path.clone()))
            .child(
                text(entry.name.clone())
                    .font(k.ty.ui)
                    .color(if selected {
                        k.c.text_base
                    } else {
                        k.c.text_muted
                    })
                    .grow(1.0)
                    .truncate(),
            );

        if let Some((glyph, color)) = marker {
            item = item.child(text(glyph).font(k.ty.small_medium).color(color));
        }

        let frames = if entry.error.is_some() {
            "ошибка".to_owned()
        } else {
            format!("{} к.", entry.frames)
        };
        item = item.child(text(frames).font(k.ty.small).color(k.c.text_faint));

        rows = rows.child(item);
    }

    if app.listing.scripts.is_empty() {
        rows = rows.child(
            text("В папке нет .tas файлов")
                .font(k.ty.small)
                .color(k.c.text_faint)
                .mt(8.0),
        );
    }

    list = list.child(
        col()
            .key("script-scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows),
    );

    let can_edit = app.folder.is_some();
    let has_selection = app.selected.is_some();

    list.child(
        row()
            .w_full()
            .gap(4.0)
            .mt(6.0)
            .child(
                Button::new("Новый")
                    .control_size(ControlSize::Small)
                    .disabled(!can_edit)
                    .on_press(|s: &mut App| s.new_script())
                    .build(k)
                    .key("new-script")
                    .grow(1.0),
            )
            .child(
                Button::new("Копия")
                    .control_size(ControlSize::Small)
                    .variant(ButtonVariant::Ghost)
                    .disabled(!has_selection)
                    .on_press(|s: &mut App| s.duplicate_script())
                    .build(k)
                    .key("duplicate-script")
                    .grow(1.0),
            ),
    )
    .child(
        row()
            .w_full()
            .gap(4.0)
            .child(
                Button::new("Имя…")
                    .control_size(ControlSize::Small)
                    .variant(ButtonVariant::Ghost)
                    .disabled(!has_selection)
                    .on_press(|s: &mut App| s.begin_rename())
                    .build(k)
                    .key("rename-script")
                    .grow(1.0),
            )
            .child(
                Button::new("Удалить…")
                    .control_size(ControlSize::Small)
                    .variant(ButtonVariant::Danger)
                    .disabled(!has_selection)
                    .on_press(|s: &mut App| s.begin_delete())
                    .build(k)
                    .key("delete-script")
                    .grow(1.0),
            ),
    )
}

/// A short folder name for a heading, or the whole path when there is no last component.
#[allow(unused)]
fn folder_label(folder: &std::path::Path) -> String {
    folder.display().to_string()
}
