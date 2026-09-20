//! Нижняя панель: текст скрипта (JSON) — вторая половина двусторонней связи
//! с таймлайном.

use crate::editor::{Editor, Msg, commands_label, lines_label};
use windows_reactor::*;

pub(crate) fn render(editor: &Editor, context: &mut ViewContext<Editor>) -> View {
    let caption = format!(
        "JSON · {} · {}",
        lines_label(editor.text.lines().count()),
        commands_label(editor.script.commands.len()),
    );

    // Подаём ровно ту форму, которую контрол вернул в последнем событии
    // (`editor.text_for_control`): иначе `RichEditBox` дописывает завершающий CR,
    // reconciler видит отличие и переустанавливает текст каждый кадр — ввод затирается.
    // Высоту даёт внешняя обёртка (`Border.height`).
    let editor_box = RichEditBox::new()
        .text(editor.text_for_control.clone())
        .on_text_changed(context.callback(Msg::TextChanged));

    // `STAR`-строка: редактор растягивается на всю высоту панели (её задаёт `Border.height`),
    // иначе он подстраивается под число строк и внизу остаётся пустота.
    Grid::new()
        .row_spacing(4.0)
        .rows([GridLength::Auto, GridLength::STAR])
        .children((
            Border::new().padding(6.0).content(TextBlock::new().text(caption)),
            editor_box.grid_row(1),
        ))
}
