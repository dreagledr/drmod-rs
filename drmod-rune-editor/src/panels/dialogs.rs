//! The modal surfaces: rename, delete, the DSL reference, and the mod's own panel.

use drmod_script::commands;
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit, TextInput};

use crate::app::App;

pub fn build(app: &App, k: &Kit, layer: &mut Vec<El>) {
    rename(app, k, layer);
    delete(app, k, layer);
    commands(app, k, layer);
    mod_panel(app, k, layer);
}

fn rename(app: &App, k: &Kit, layer: &mut Vec<El>) {
    let body = col()
        .w_full()
        .padding(16.0)
        .gap(10.0)
        .child(
            TextInput::new(&app.rename_text)
                .label("Имя файла без .tas")
                .width(None)
                .on_edit(|s: &mut App| &mut s.rename_text)
                .on_submit(|s: &mut App, _tx| s.confirm_rename())
                .build(k),
        )
        .child(
            row()
                .w_full()
                .gap(6.0)
                .justify_end()
                .child(
                    Button::new("Отмена")
                        .control_size(ControlSize::Small)
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut App| s.rename_target = None)
                        .build(k)
                        .key("rename-cancel"),
                )
                .child(
                    Button::new("Переименовать")
                        .control_size(ControlSize::Small)
                        .variant(ButtonVariant::Submit)
                        .on_press(|s: &mut App| s.confirm_rename())
                        .build(k)
                        .key("rename-confirm"),
                ),
        );

    Dialog::new("rename", "Переименовать скрипт")
        .description("Меняется имя файла; имя внутри скрипта остаётся своим.")
        .width(440.0)
        .fit()
        .open(app.rename_target.is_some())
        .on_close(|s: &mut App| s.rename_target = None)
        .child(body)
        .build(k, layer);
}

fn delete(app: &App, k: &Kit, layer: &mut Vec<El>) {
    let name = app
        .delete_target
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let body = col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(
            text(format!("Удалить «{name}»? Это необратимо."))
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .wrap()
                .w_full(),
        )
        .child(
            row()
                .w_full()
                .gap(6.0)
                .justify_end()
                .child(
                    Button::new("Отмена")
                        .control_size(ControlSize::Small)
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut App| s.delete_target = None)
                        .build(k)
                        .key("delete-cancel"),
                )
                .child(
                    Button::new("Удалить")
                        .control_size(ControlSize::Small)
                        .variant(ButtonVariant::Danger)
                        .on_press(|s: &mut App| s.confirm_delete())
                        .build(k)
                        .key("delete-confirm"),
                ),
        );

    Dialog::new("delete", "Удалить скрипт")
        .description("Файл будет удалён с диска.")
        .width(420.0)
        .fit()
        .open(app.delete_target.is_some())
        .on_close(|s: &mut App| s.delete_target = None)
        .child(body)
        .build(k, layer);
}

fn commands(app: &App, k: &Kit, layer: &mut Vec<El>) {
    let mut list = col().w_full().gap(2.0);

    list = list
        .child(section(k, "Строка правил"))
        .child(reference_row(k, commands::FRAME));
    for command in commands::RULES {
        list = list.child(reference_row(k, command));
    }

    list = list.child(section(k, "Стики"));
    for command in commands::STICKS {
        list = list.child(reference_row(k, command));
    }

    list = list.child(section(k, "Кнопки"));
    for command in commands::frame() {
        list = list.child(reference_row(k, command));
    }

    let body = col()
        .key("commands-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(list);

    Dialog::new("commands", "Команды DSL")
        .description("Токены формата .tas: что пишется и что это значит.")
        .width(720.0)
        .open(app.commands_open)
        .on_close(|s: &mut App| s.commands_open = false)
        .child(body)
        .build(k, layer);
}

fn reference_row(k: &Kit, command: commands::ScriptCommandHelp) -> El {
    row()
        .w_full()
        .gap(10.0)
        .py(3.0)
        .child(
            text(command.spelling)
                .font(k.ty.mono)
                .color(k.c.text_accent)
                .w(190.0)
                .truncate(),
        )
        .child(
            text(command.help)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .grow(1.0),
        )
}

fn section(k: &Kit, label: &str) -> El {
    text(label)
        .font(k.ty.small_medium)
        .color(k.c.text_faint)
        .mt(10.0)
        .mb(2.0)
}

fn mod_panel(app: &App, k: &Kit, layer: &mut Vec<El>) {
    let body = col()
        .w_full()
        .padding(16.0)
        .gap(10.0)
        .child(
            text(
                "Этот редактор — эксперимент на Rune. Он открывает папку со скриптами, правит и \
                 сохраняет .tas, показывает кадры и запускает скрипты через HTTP API мода. \
                 Установка мода, поиск игры в Steam и поднятие окна игры перенесены в разной \
                 степени: на Linux окна игры нет, а установка пока остаётся за ImGui-редактором.",
            )
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap()
            .w_full(),
        )
        .child(
            text(format!(
                "Папка игры: {}",
                app.settings
                    .game_folder
                    .as_ref()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_else(|| "не выбрана".to_owned())
            ))
            .font(k.ty.small)
            .color(k.c.text_faint)
            .wrap()
            .w_full(),
        )
        .child(
            row().w_full().justify_end().child(
                Button::new("Закрыть")
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut App| s.mod_open = false)
                    .build(k)
                    .key("mod-close"),
            ),
        );

    Dialog::new("mod", "Мод")
        .width(560.0)
        .fit()
        .open(app.mod_open)
        .on_close(|s: &mut App| s.mod_open = false)
        .child(body)
        .build(k, layer);
}
