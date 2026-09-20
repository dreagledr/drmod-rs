//! Верхняя панель: таймлайн-матрица «кадр × действие».
//!
//! Строка — кадр, колонки — действия-флаги и числовые оси камеры/стика.
//! Флаги правятся кликом (скрипт пересобирается RLE-нормализацией), числа —
//! только для чтения: их правка идёт в полосе свойств выбранного кадра
//! (иначе на каждый кадр создаётся десяток `NumberBox`, и окно подвисает).

use crate::editor::{Editor, Msg};
use crate::matrix::NumberColumn;
use crate::model::BitAction;
use windows_reactor::*;

/// Колонка «кадр» + флаги действий + числовые оси.
pub(crate) const COLUMN_COUNT: usize = 1 + BitAction::ALL.len() + NumberColumn::ALL.len();

const BITS_FIRST: usize = 1;
const NUMBERS_FIRST: usize = 1 + BitAction::ALL.len();

/// Что стоит в ячейке строки кадра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cell {
    Frame,
    Bit(BitAction),
    Number(NumberColumn),
}

/// Состав строки: `(номер колонки, ячейка)`. Один источник правды для вёрстки и тестов —
/// именно здесь ловится регрессия «ячейки схлопнулись в колонку 0».
pub(crate) fn row_cells() -> Vec<(i32, Cell)> {
    let mut cells = Vec::with_capacity(COLUMN_COUNT);
    cells.push((0, Cell::Frame));
    for (offset, action) in BitAction::ALL.iter().enumerate() {
        cells.push(((BITS_FIRST + offset) as i32, Cell::Bit(*action)));
    }
    for (offset, number) in NumberColumn::ALL.iter().enumerate() {
        cells.push(((NUMBERS_FIRST + offset) as i32, Cell::Number(*number)));
    }
    cells
}

pub(crate) fn render(
    editor: &Editor,
    context: &mut ViewContext<Editor>,
    max_height: f64,
) -> View {
    // Строки собираем сразу: ленивый итератор удерживал бы `context` до конца функции.
    let rows: Vec<(String, View)> = (0..editor.matrix.len())
        .map(|frame| (format!("frame-{frame}"), frame_row(editor, context, frame)))
        .collect();

    // Ограничение вешаем на сам `ListView`: обёртке он не подчиняется и растёт по контенту.
    let list = ListView::new()
        .selection_mode(ListViewSelectionMode::Single)
        .selected_index(editor.selected_frame)
        .max_height(max_height)
        .on_selection_changed(context.callback(Msg::SelectFrame))
        .collection_slot(ListViewSlot::Items, rows);

    // Без внешнего `ScrollViewer`: он не ограничивает высоту контента, и нижняя
    // панель (JSON) выдавливается за край окна. Горизонтальная прокрутка — позже.
    Grid::new()
        .rows([GridLength::Auto, GridLength::Auto, GridLength::STAR])
        .children((
            Border::new()
                .padding(6.0)
                .grid_row(0)
                .content(toolbar(editor, context)),
            Border::new().grid_row(1).content(header_row()),
            Border::new().grid_row(2).content(list),
        ))
}

fn toolbar(editor: &Editor, context: &mut ViewContext<Editor>) -> View {
    let frame_label = match editor.selected_frame {
        Some(frame) => format!("выбран кадр {frame}"),
        None => "кадр не выбран".to_string(),
    };
    let (pane_caption, pane_open) = if editor.pane_open {
        ("◀ Скрипты", false)
    } else {
        ("Скрипты ▶", true)
    };

    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .children((
            Button::new()
                .on_click(context.message(Msg::TogglePane(pane_open)))
                .content(pane_caption),
            TextBlock::new().text(format!("кадров: {} · {}", editor.matrix.len(), frame_label)),
            Button::new()
                .on_click(context.message(Msg::AddFrame))
                .content("+ кадр"),
            Button::new()
                .on_click(context.message(Msg::RemoveFrame))
                .content("− кадр"),
            TextBlock::new().text("галочка переключает действие в кадре, команды пересобираются RLE"),
        ))
}

fn header_row() -> View {
    let specs = row_cells();
    let cells: [View; COLUMN_COUNT] = std::array::from_fn(|index| {
        let (column, cell) = specs[index];
        let label = match cell {
            Cell::Frame => "кадр".to_string(),
            Cell::Bit(action) => action.label().to_string(),
            Cell::Number(number) => number.label().to_string(),
        };
        TextBlock::new().text(label).grid_column(column).into()
    });

    Grid::new()
        .columns(column_widths())
        .column_spacing(2.0)
        .children(cells)
}

fn frame_row(editor: &Editor, context: &mut ViewContext<Editor>, frame: usize) -> View {
    let specs = row_cells();
    let cells: [View; COLUMN_COUNT] = std::array::from_fn(|index| {
        let (column, cell) = specs[index];
        match cell {
            Cell::Frame => TextBlock::new()
                .text(format!("{frame}"))
                .grid_column(column)
                .into(),
            Cell::Bit(action) => CheckBox::new()
                .is_checked(editor.matrix.bit(frame, action))
                .on_is_checked_changed(context.callback(move |value: bool| Msg::ToggleBit {
                    frame,
                    action,
                    on: value,
                }))
                .grid_column(column)
                // Сокращённая подпись колонки расшифровывается ключом JSON (`light_attack` и т. п.).
                .tooltip(action.key()),
            Cell::Number(number) => {
                let text = editor
                    .matrix
                    .number(frame, number)
                    .map_or_else(|| "·".to_string(), |value| value.to_string());
                TextBlock::new().text(text).grid_column(column).into()
            }
        }
    });

    Grid::new()
        .columns(column_widths())
        .column_spacing(2.0)
        .children(cells)
}

fn column_widths() -> [GridLength; COLUMN_COUNT] {
    std::array::from_fn(|column| {
        if column == 0 {
            GridLength::Pixel(48.0)
        } else if column < NUMBERS_FIRST {
            GridLength::Pixel(46.0)
        } else {
            GridLength::Pixel(60.0)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Регрессия, найденная глазами на скриншоте: без `grid_column` все ячейки
    /// схлопывались в колонку 0, и таблица выглядела как одна колонка галочек.
    #[test]
    fn row_cells_cover_every_column_in_order() {
        let cells = row_cells();

        assert_eq!(cells.len(), COLUMN_COUNT, "ячейка на каждую колонку");
        let columns: Vec<i32> = cells.iter().map(|(column, _)| *column).collect();
        assert_eq!(
            columns,
            (0..COLUMN_COUNT as i32).collect::<Vec<_>>(),
            "колонки идут подряд, без пропусков и схлопывания в 0"
        );
        assert_eq!(cells.first().map(|(_, cell)| *cell), Some(Cell::Frame));

        let bits = cells
            .iter()
            .filter(|(_, cell)| matches!(cell, Cell::Bit(_)))
            .count();
        assert_eq!(bits, BitAction::ALL.len(), "по колонке на каждое действие");

        let numbers = cells
            .iter()
            .filter(|(_, cell)| matches!(cell, Cell::Number(_)))
            .count();
        assert_eq!(numbers, NumberColumn::ALL.len(), "камера и стик по осям");
        assert_eq!(
            cells.last().map(|(_, cell)| *cell),
            Some(Cell::Number(NumberColumn::StickY))
        );
    }

    #[test]
    fn widths_match_columns() {
        assert_eq!(column_widths().len(), COLUMN_COUNT);
    }
}
