//! Нижняя панель: текст скрипта (JSON) — вторая половина двусторонней связи
//! с таймлайном.

use crate::editor::{Editor, Msg, commands_label, lines_label};
use windows_reactor::*;

pub(crate) fn render(editor: &Editor, context: &mut ViewContext<Editor>, height: f64) -> View {
    let caption = format!(
        "JSON · {} · {}",
        lines_label(editor.text.lines().count()),
        commands_label(editor.script.commands.len()),
    );

    Grid::new()
        .row_spacing(4.0)
        .rows([GridLength::Auto, GridLength::Auto])
        .children((
            Border::new().padding(6.0).content(TextBlock::new().text(caption)),
            RichEditBox::new()
                .text(editor.text.clone())
                // Высоту задаём контролу: обёртка его не ограничивает.
                .height(height)
                .on_text_changed(context.callback(Msg::TextChanged))
                .grid_row(1),
        ))
}
