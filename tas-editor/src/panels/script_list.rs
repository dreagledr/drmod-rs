//! Левая панель: список мок-скриптов, сброс, статус.
//!
//! В мок-режиме здесь нет работы с диском — только переключение данных в памяти.

use crate::editor::{Editor, Msg, commands_label};
use windows_reactor::*;

pub(crate) fn render(editor: &Editor, context: &mut ViewContext<Editor>) -> View {
    let rows: Vec<(String, View)> = editor
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let label = format!("{} · {}", entry.name, commands_label(entry.script.commands.len()));
            let text = if editor.selected_file == Some(index) {
                format!("▸ {label}")
            } else {
                format!("   {label}")
            };
            (format!("entry-{index}"), TextBlock::new().text(text).into())
        })
        .collect();

    let list = ListView::new()
        .selection_mode(ListViewSelectionMode::Single)
        .selected_index(editor.selected_file)
        .on_selection_changed(context.callback(Msg::SelectFile))
        .collection_slot(ListViewSlot::Items, rows);

    let panel = StackPanel::new()
        .spacing(8.0)
        .children((
            TextBlock::new().text("Мок-скрипты"),
            list,
            Border::new().padding(6.0).content(
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(6.0)
                    .children((
                        Button::new()
                            .on_click(context.message(Msg::Reset))
                            .content("Сбросить мок"),
                        Button::new()
                            .on_click(context.message(Msg::TogglePane(false)))
                            .content("◀ Свернуть панель"),
                    )),
            ),
            banner(editor),
            TextBlock::new().text(editor.status.clone()),
        ));

    Border::new().padding(8.0).content(panel)
}

fn banner(editor: &Editor) -> InfoBar {
    let (severity, title, message) = if let Some(error) = &editor.parse_error {
        (InfoBarSeverity::Error, "JSON не разобран", error.clone())
    } else if let Some(issue) = editor.issues.iter().find(|issue| issue.is_error()) {
        (InfoBarSeverity::Error, "Ошибки валидации", issue.text.clone())
    } else if let Some(issue) = editor.issues.iter().find(|issue| !issue.is_error()) {
        (InfoBarSeverity::Warning, "Замечания", issue.text.clone())
    } else {
        (InfoBarSeverity::Success, "Скрипт корректен", String::new())
    };
    let open = editor.parse_error.is_some() || !editor.issues.is_empty();

    InfoBar::new()
        .is_open(open)
        .severity(severity)
        .title(title)
        .message(message)
}
