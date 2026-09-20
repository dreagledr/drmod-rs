//! Верхняя панель: таймлайн-матрица «кадр × действие».
//!
//! Строка — кадр, колонки — действия-флаги и числовые оси камеры/стика.
//! Флаги правятся кликом (скрипт пересобирается RLE-нормализацией), числа —
//! только для чтения: их правка идёт в полосе свойств выбранного кадра.
//!
//! Колонок — всегда полный набор (кадр + все действия + числовые оси). А вот строк
//! показываем ровно столько, сколько влезает в отведённую таймлайну высоту: контейнеры
//! в этом рендере не клипуют, и «лишние» строки списка рисуются поверх нижней панели.
//! Остальные кадры листаются кнопками.

use crate::editor::{Editor, Msg};
use crate::matrix::NumberColumn;
use crate::model::BitAction;
use windows_reactor::*;

/// Что стоит в колонке.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cell {
    Frame,
    Bit(BitAction),
    Number(NumberColumn),
}

impl Cell {
    fn label(self) -> String {
        match self {
            Cell::Frame => "кадр".to_string(),
            Cell::Bit(action) => action.label().to_string(),
            Cell::Number(number) => number.label().to_string(),
        }
    }

    fn width(self) -> f64 {
        match self {
            Cell::Frame => 56.0,
            Cell::Bit(_) => 46.0,
            Cell::Number(_) => 60.0,
        }
    }
}

/// Полный набор колонок: кадр, все действия, все числовые оси.
pub(crate) fn all_cells() -> Vec<Cell> {
    let mut cells = vec![Cell::Frame];
    cells.extend(BitAction::ALL.iter().copied().map(Cell::Bit));
    cells.extend(NumberColumn::ALL.iter().copied().map(Cell::Number));
    cells
}

pub(crate) fn render(editor: &Editor, context: &mut ViewContext<Editor>) -> View {
    let cells = all_cells();
    let (first, last) = editor.visible_frame_range();

    let rows: Vec<(String, View)> = (first..=last)
        .map(|frame| {
            (
                format!("frame-{frame}"),
                frame_row(editor, context, frame, &cells),
            )
        })
        .collect();

    let list = ListView::new()
        .selection_mode(ListViewSelectionMode::Single)
        .selected_index(editor.selected_frame)
        .on_selection_changed(context.callback(Msg::SelectFrame))
        .collection_slot(ListViewSlot::Items, rows);

    Grid::new()
        .rows([GridLength::Auto, GridLength::Auto, GridLength::Auto])
        .children((
            Border::new()
                .padding(6.0)
                .grid_row(0)
                .content(toolbar(editor, context, first, last, cells.len())),
            Border::new().grid_row(1).content(header_row(&cells)),
            Border::new().grid_row(2).content(list),
        ))
}

fn toolbar(
    editor: &Editor,
    context: &mut ViewContext<Editor>,
    first: usize,
    last: usize,
    columns: usize,
) -> View {
    let total = editor.matrix.len();
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
            TextBlock::new().text(format!(
                "кадров {total} · {frame_label} · колонок {columns}"
            )),
            Button::new()
                .is_enabled(first > 0)
                .on_click(context.message(Msg::ShiftFrames(-1)))
                .content("▲"),
            TextBlock::new().text(format!("кадры {first}–{last}")),
            Button::new()
                .is_enabled(last + 1 < total)
                .on_click(context.message(Msg::ShiftFrames(1)))
                .content("▼"),
            Button::new()
                .on_click(context.message(Msg::AddFrame))
                .content("+ кадр"),
            Button::new()
                .on_click(context.message(Msg::RemoveFrame))
                .content("− кадр"),
        ))
}

fn header_row(cells: &[Cell]) -> View {
    let items: Vec<(String, View)> = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            (
                format!("header-{index}"),
                TextBlock::new()
                    .text(cell.label())
                    .width(cell.width())
                    .into(),
            )
        })
        .collect();

    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(2.0)
        .keyed_children(items)
}

fn frame_row(
    editor: &Editor,
    context: &mut ViewContext<Editor>,
    frame: usize,
    cells: &[Cell],
) -> View {
    let items: Vec<(String, View)> = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let view: View = match *cell {
                Cell::Frame => TextBlock::new()
                    .text(format!("{frame}"))
                    .width(cell.width())
                    .into(),
                Cell::Bit(action) => CheckBox::new()
                    .is_checked(editor.matrix.bit(frame, action))
                    .on_is_checked_changed(context.callback(move |value: bool| Msg::ToggleBit {
                        frame,
                        action,
                        on: value,
                    }))
                    .width(cell.width())
                    // Сокращённая подпись колонки расшифровывается ключом JSON.
                    .tooltip(action.key()),
                Cell::Number(number) => {
                    let text = editor
                        .matrix
                        .number(frame, number)
                        .map_or_else(|| "·".to_string(), |value| value.to_string());
                    TextBlock::new().text(text).width(cell.width()).into()
                }
            };
            (format!("cell-{index}"), view)
        })
        .collect();

    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(2.0)
        .keyed_children(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_cells_cover_frame_bits_and_numbers() {
        let cells = all_cells();
        assert_eq!(
            cells.len(),
            1 + BitAction::ALL.len() + NumberColumn::ALL.len()
        );
        assert_eq!(cells.first().copied(), Some(Cell::Frame));

        let bits = cells
            .iter()
            .filter(|cell| matches!(cell, Cell::Bit(_)))
            .count();
        assert_eq!(bits, BitAction::ALL.len());

        let numbers = cells
            .iter()
            .filter(|cell| matches!(cell, Cell::Number(_)))
            .count();
        assert_eq!(numbers, NumberColumn::ALL.len());
    }

    #[test]
    fn widths_are_positive() {
        for cell in all_cells() {
            assert!(cell.width() > 0.0, "ширина колонки должна быть положительной");
        }
    }
}
