//! Корневой компонент редактора в мок-режиме: скрипты живут в памяти (`mock`),
//! панели показывают состояние, правки идут по цепочке
//! «матрица ↔ JSON-текст» без обращения к диску.

use crate::matrix::{FrameMatrix, NumberColumn};
use crate::mock;
use crate::model::{self, BitAction, Issue, Script, Trigger, WhenEnemy};
use crate::panels;
use windows_reactor::*;

/// Полоса свойств, таймлайн и JSON получают явные высоты: контейнеры в этом рендере
/// не клипуют, поэтому контент выше отведённой строки рисуется поверх нижних панелей.
const PANEL_CHROME: f64 = 96.0;
const PROPS_OPEN_HEIGHT: f64 = 320.0;
const PROPS_CLOSED_HEIGHT: f64 = 70.0;
const FALLBACK_WINDOW_HEIGHT: f64 = 900.0;
/// Доля свободной высоты под JSON-редактор по умолчанию; дальше её двигает сплиттер.
const SPLITTER_HEIGHT: f64 = 12.0;
const SPLITTER_COLOR: Color = Color::rgb(0x8a, 0x8f, 0x94);
const SPLITTER_ACTIVE_COLOR: Color = Color::rgb(0x4c, 0x7c, 0xd0);
const DEFAULT_SPLIT_RATIO: f64 = 0.81;

/// Высота строки кадра в таймлайне (в DIPs, с учётом отступов `ListView`).
pub(crate) const ROW_HEIGHT: f64 = 66.0;
/// Тулбар и заголовок таймлайна.
pub(crate) const TIMELINE_CHROME: f64 = 60.0;

/// Числовые поля условия `when_enemy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhenField {
    FrameMin,
    FrameMax,
    DistMax,
    BladeDyMin,
    PlayerYMin,
    PlayerYMax,
    PlayerVyMax,
}

/// Мок-скрипт в списке слева.
pub(crate) struct Entry {
    pub(crate) name: &'static str,
    pub(crate) script: Script,
}

pub(crate) struct Editor {
    pub(crate) entries: Vec<Entry>,
    pub(crate) selected_file: Option<usize>,
    /// Каноничное состояние: то, что уехало бы в файл.
    pub(crate) script: Script,
    /// Сериализация `script` для текстового редактора.
    pub(crate) text: String,
    /// Производная матрица кадров.
    pub(crate) matrix: FrameMatrix,
    pub(crate) issues: Vec<Issue>,
    pub(crate) parse_error: Option<String>,
    pub(crate) selected_frame: Option<usize>,
    pub(crate) conditional_index: Option<usize>,
    pub(crate) status: String,
    /// Текст в той форме, в какой его ждёт `RichEditBox` (CR-разделители).
    /// Подаём контролу ровно её: как только отдадим более короткую форму, контрол
    /// переустановит текст и на каждую установку ответит событием — бесконечный цикл,
    /// в котором затирается ввод пользователя.
    pub(crate) text_for_control: String,
    pub(crate) pane_open: bool,
    pub(crate) properties_open: bool,
    /// Доля высоты под таймлайн (остальное — JSON). Меняется сплиттером.
    pub(crate) split_ratio: f64,
    /// Первый видимый кадр: список показывает ровно те строки, что влезают в высоту
    /// таймлайна (контейнеры здесь не клипуют, и «лишние» строки выдавливали бы редактор).
    pub(crate) frame_offset: usize,
    /// Начало перетаскивания сплиттера: `(window_y, ratio)`.
    pub(crate) drag_start: Option<(f64, f64)>,
    /// Клиентская высота окна в DIPs: от неё считаются высоты панелей.
    pub(crate) window_height: f64,
}

#[derive(Clone)]
pub(crate) enum Msg {
    Resized(WindowSize),
    ShiftFrames(i32),
    SplitDragStart(f64),
    SplitDragTo(f64),
    SplitDragEnd,
    SelectFile(Option<usize>),
    Reset,
    TextChanged(String),
    ToggleBit {
        frame: usize,
        action: BitAction,
        on: bool,
    },
    SetNumber {
        frame: usize,
        column: NumberColumn,
        value: Option<f64>,
    },
    AddFrame,
    RemoveFrame,
    SelectFrame(Option<usize>),
    TogglePane(bool),
    ToggleProperties(bool),
    NameChanged(String),
    TriggerPos(bool),
    TriggerTicks(bool),
    TriggerPosValue {
        axis: usize,
        value: Option<f64>,
    },
    TriggerTicksValue(Option<f64>),
    RestartEnabled(bool),
    ConditionalSelected(Option<usize>),
    ConditionalAnim(String),
    ConditionalNumber {
        field: WhenField,
        value: Option<f64>,
    },
    ConditionalRepeat(bool),
    ConditionalDelete,
}

impl Editor {
    fn load(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };

        self.script = entry.script.clone();
        self.text = model::to_text(&self.script);
        self.text_for_control = cr_form(&self.text);
        self.matrix = FrameMatrix::from_script(&self.script);
        self.issues = model::validate(&self.script);
        self.parse_error = None;
        self.selected_frame = None;
        self.conditional_index = None;
        self.selected_file = Some(index);
        self.status = format!(
            "мок «{}»: {}, {} кадров",
            entry.name,
            commands_label(self.script.commands.len()),
            self.matrix.len()
        );
    }

    /// Сериализация модели в текст редактора.
    fn sync_text(&mut self) {
        self.text = model::to_text(&self.script);
    }

    /// Правка матрицы: пересобрать команды, текст и замечания.
    fn commit_matrix(&mut self) {
        self.matrix.apply_to(&mut self.script);
        self.sync_text();
        self.text_for_control = cr_form(&self.text);
        self.issues = model::validate(&self.script);
        self.parse_error = None;
    }

    pub(crate) fn selected_when_enemy(&self) -> Option<&WhenEnemy> {
        let index = self.conditional_index?;
        self.matrix
            .conditional()
            .get(index)
            .and_then(|command| command.when_enemy.as_ref())
    }

    fn update_when_enemy(&mut self, update: impl FnOnce(&mut WhenEnemy)) {
        let Some(index) = self.conditional_index else {
            return;
        };
        let Some(current) = self
            .matrix
            .conditional()
            .get(index)
            .and_then(|command| command.when_enemy.clone())
        else {
            return;
        };

        let mut updated = current;
        update(&mut updated);
        self.matrix.set_conditional(index, updated);
        self.commit_matrix();
    }

    fn after_properties_change(&mut self) {
        self.sync_text();
        self.text_for_control = cr_form(&self.text);
        self.issues = model::validate(&self.script);
    }

    /// Свободная высота под таймлайн: окно минус свойства, шапки панелей и сплиттер.
    fn available_height(&self) -> f64 {
        let total = if self.window_height > 200.0 {
            self.window_height
        } else {
            FALLBACK_WINDOW_HEIGHT
        };
        let props = if self.properties_open {
            PROPS_OPEN_HEIGHT
        } else {
            PROPS_CLOSED_HEIGHT
        };
        (total - props - PANEL_CHROME - SPLITTER_HEIGHT).max(240.0)
    }

    /// Высота, отведённая таймлайну (её задаёт сплиттер).
    pub(crate) fn timeline_height(&self) -> f64 {
        (self.available_height() * self.split_ratio).max(120.0)
    }

    /// Сколько строк кадров помещается в таймлайн: список показывает ровно столько,
    /// иначе лишние строки рисуются поверх нижней панели (контейнеры здесь не клипуют).
    pub(crate) fn visible_frames(&self) -> usize {
        let rows = (self.timeline_height() - TIMELINE_CHROME) / ROW_HEIGHT;
        rows.floor().max(1.0) as usize
    }

    /// Диапазон видимых кадров: `(первый, последний включительно)`.
    pub(crate) fn visible_frame_range(&self) -> (usize, usize) {
        let total = self.matrix.len();
        if total == 0 {
            return (0, 0);
        }
        let count = self.visible_frames().min(total);
        let first = self.frame_offset.min(total.saturating_sub(1));
        (first, (first + count - 1).min(total - 1))
    }
}

/// Текст в форме `RichEditBox`: LF → CR и завершающий CR.
pub(crate) fn cr_form(text: &str) -> String {
    let mut shown = text.replace('\n', "\r");
    shown.push('\r');
    shown
}

/// Тянущаяся полоса между таймлайном и JSON-редактором (`GridSplitter` в реакторе нет).
///
/// Реагирует только на нажатие левой кнопки и последующее перетаскивание (никакого
/// hover): фон обязателен, иначе `Border` прозрачен для hit-test и событий не получит.
fn splitter(editor: &Editor, context: &mut ViewContext<Editor>) -> Border {
    let color = if editor.drag_start.is_some() {
        SPLITTER_ACTIVE_COLOR
    } else {
        SPLITTER_COLOR
    };

    Border::new()
        .height(SPLITTER_HEIGHT)
        .background(Brush::Solid(color))
        // Без захвата указателя `PointerMoved` приходит только пока курсор над полосой —
        // перетаскивание «отваливается» на первом же смещении.
        .capture_pointer_on_press(true)
        .on_pointer_pressed(context.callback(|info: PointerEventInfo| {
            if info.is_left_button_pressed {
                Msg::SplitDragStart(info.window_y)
            } else {
                Msg::SplitDragEnd
            }
        }))
        .on_pointer_moved(context.callback(|info: PointerEventInfo| {
            if info.is_left_button_pressed {
                Msg::SplitDragTo(info.window_y)
            } else {
                // Кнопку отпустили вне полосы — перетаскивание закончено.
                Msg::SplitDragEnd
            }
        }))
        .on_pointer_released(context.callback(|_: PointerEventInfo| Msg::SplitDragEnd))
}

impl Component for Editor {
    type Message = Msg;
    type Input = ();

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        let entries: Vec<Entry> = mock::scripts()
            .into_iter()
            .map(|mock| Entry {
                name: mock.name,
                script: mock.script,
            })
            .collect();

        let mut editor = Self {
            entries,
            selected_file: None,
            script: Script::default(),
            text: String::new(),
            matrix: FrameMatrix::default(),
            issues: Vec::new(),
            parse_error: None,
            selected_frame: None,
            conditional_index: None,
            status: String::new(),
            text_for_control: String::new(),
            pane_open: true,
            properties_open: false,
            split_ratio: DEFAULT_SPLIT_RATIO,
            frame_offset: 0,
            drag_start: None,
            window_height: 0.0,
        };
        editor.load(0);
        editor
    }

    fn update(&mut self, message: Msg, _context: &ComponentContext<Self>) {
        match message {
            Msg::Resized(size) => self.window_height = size.height,
            Msg::ShiftFrames(delta) => {
                let last = self
                    .matrix
                    .len()
                    .saturating_sub(self.visible_frames());
                let next = self.frame_offset as i32 + delta;
                self.frame_offset = next.clamp(0, last as i32) as usize;
            }
            Msg::SplitDragStart(y) => self.drag_start = Some((y, self.split_ratio)),
            Msg::SplitDragTo(y) => {
                if let Some((start_y, start_ratio)) = self.drag_start {
                    let available = self.available_height();
                    self.split_ratio =
                        (start_ratio + (y - start_y) / available).clamp(0.2, 0.85);
                }
            }
            Msg::SplitDragEnd => self.drag_start = None,
            Msg::SelectFile(index) => {
                if let Some(index) = index
                    && Some(index) != self.selected_file
                {
                    self.load(index);
                }
            }
            Msg::Reset => {
                if let Some(index) = self.selected_file {
                    self.load(index);
                    self.status = format!("{} — мок сброшен к исходному", self.status);
                }
            }
            Msg::TextChanged(value) => {
                // Запоминаем ровно ту форму, которую вернул контрол: её же и подадим,
                // тогда `SetText` пойдёт с идентичным текстом и цикл событий разорвётся.
                self.text_for_control = value.clone();
                // `RichEditBox` отдаёт CR-разделители и хвост из пустых абзацев —
                // нормализуем в LF и срезаем пустой хвост, оставляя один финальный `\n`.
                let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
                let normalized = format!("{}\n", normalized.trim_end_matches('\n'));
                // Эхо программной установки (текст не изменился) — модель не трогаем.
                if normalized == self.text {
                    return;
                }
                self.text = normalized;
                match model::from_text(&self.text) {
                    Ok(script) => {
                        self.script = script;
                        self.matrix = FrameMatrix::from_script(&self.script);
                        self.issues = model::validate(&self.script);
                        self.parse_error = None;
                        if self
                            .selected_frame
                            .is_some_and(|frame| frame >= self.matrix.len())
                        {
                            self.selected_frame = None;
                        }
                        if self
                            .conditional_index
                            .is_some_and(|index| index >= self.matrix.conditional().len())
                        {
                            self.conditional_index = None;
                        }
                    }
                    Err(error) => self.parse_error = Some(error),
                }
            }
            Msg::ToggleBit { frame, action, on } => {
                self.matrix.set_bit(frame, action, on);
                self.commit_matrix();
            }
            Msg::SetNumber {
                frame,
                column,
                value,
            } => {
                self.matrix
                    .set_number(frame, column, value.map(|value| value as i32));
                self.commit_matrix();
            }
            Msg::AddFrame => {
                self.matrix.push_frame();
                self.commit_matrix();
            }
            Msg::RemoveFrame => {
                let last = self.matrix.len().saturating_sub(1);
                if self.matrix.pop_frame() {
                    self.status = format!("убран кадр {last} вместе с вводом");
                }
                if self.selected_frame.is_some_and(|frame| frame >= self.matrix.len()) {
                    self.selected_frame = None;
                }
                self.commit_matrix();
            }
            Msg::SelectFrame(index) => self.selected_frame = index,
            Msg::TogglePane(open) => self.pane_open = open,
            Msg::ToggleProperties(open) => self.properties_open = open,
            Msg::NameChanged(value) => {
                self.script.name = value;
                self.after_properties_change();
            }
            Msg::TriggerPos(enabled) => {
                let mut trigger = self.script.trigger.clone().unwrap_or_default();
                trigger.pos = enabled.then_some([0.0, 0.0, 0.0]);
                self.script.trigger =
                    (trigger.pos.is_some() || trigger.ticks.is_some()).then_some(trigger);
                self.after_properties_change();
            }
            Msg::TriggerTicks(enabled) => {
                let mut trigger = self.script.trigger.clone().unwrap_or_default();
                trigger.ticks = enabled.then_some(0);
                self.script.trigger =
                    (trigger.pos.is_some() || trigger.ticks.is_some()).then_some(trigger);
                self.after_properties_change();
            }
            Msg::TriggerPosValue { axis, value } => {
                let mut trigger: Trigger = self.script.trigger.clone().unwrap_or_default();
                let mut pos = trigger.pos.unwrap_or([0.0, 0.0, 0.0]);
                pos[axis] = value.unwrap_or(0.0) as f32;
                trigger.pos = Some(pos);
                self.script.trigger = Some(trigger);
                self.after_properties_change();
            }
            Msg::TriggerTicksValue(value) => {
                let mut trigger = self.script.trigger.clone().unwrap_or_default();
                trigger.ticks = Some(value.unwrap_or(0.0).max(0.0) as u32);
                self.script.trigger = Some(trigger);
                self.after_properties_change();
            }
            Msg::RestartEnabled(enabled) => {
                self.script.restart = enabled.then(model::Restart::default);
                self.after_properties_change();
            }
            Msg::ConditionalSelected(index) => self.conditional_index = index,
            Msg::ConditionalAnim(value) => {
                let anim = parse_numbers(&value);
                self.update_when_enemy(|when| when.anim = anim);
            }
            Msg::ConditionalNumber { field, value } => {
                self.update_when_enemy(|when| {
                    let float = value.map(|value| value as f32);
                    match field {
                        WhenField::FrameMin => when.frame_min = value.map(|v| v.max(0.0) as u32),
                        WhenField::FrameMax => when.frame_max = value.map(|v| v.max(0.0) as u32),
                        WhenField::DistMax => when.dist_max = float,
                        WhenField::BladeDyMin => when.blade_dy_min = float,
                        WhenField::PlayerYMin => when.player_y_min = float,
                        WhenField::PlayerYMax => when.player_y_max = float,
                        WhenField::PlayerVyMax => when.player_vy_max = float,
                    }
                });
            }
            Msg::ConditionalRepeat(value) => {
                self.update_when_enemy(|when| when.repeat = value);
            }
            Msg::ConditionalDelete => {
                if let Some(index) = self.conditional_index {
                    self.matrix.remove_conditional(index);
                    self.conditional_index = None;
                    self.commit_matrix();
                }
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        let title = match self.selected_file.and_then(|index| self.entries.get(index)) {
            Some(entry) => format!("TAS Editor — {} (мок)", entry.name),
            None => "TAS Editor".to_string(),
        };
        context.window_title(title);
        context.on_window_size(context.callback(Msg::Resized));

        // Таймлайн сам показывает ровно столько строк, сколько влезает в его высоту,
        // JSON-редактор — `STAR`-строка: так он всегда добирает свободное место,
        // и внизу окна не остаётся зазора.
        let content = Grid::new()
            .rows([
                GridLength::Auto,
                GridLength::Auto,
                GridLength::Auto,
                GridLength::Star(1.0),
            ])
            .children((
                Border::new()
                    .grid_row(0)
                    .content(panels::properties::render(self, context)),
                Border::new()
                    .grid_row(1)
                    .content(panels::timeline::render(self, context)),
                // Своя тянущаяся полоса: `GridSplitter` в реакторе нет.
                splitter(self, context).grid_row(2),
                Border::new()
                    .grid_row(3)
                    .content(panels::text_editor::render(self, context)),
            ));

        SplitView::new()
            .display_mode(SplitViewDisplayMode::Inline)
            .is_pane_open(self.pane_open)
            .open_pane_length(320.0)
            .slots([
                SlotView::new(
                    SplitViewSlot::Pane,
                    panels::script_list::render(self, context),
                ),
                SlotView::new(SplitViewSlot::Content, content),
            ])
    }
}

/// Склонение по русским правилам: 1 команда, 2 команды, 5 команд.
pub(crate) fn plural(count: usize, forms: [&str; 3]) -> String {
    let tail = count % 100;
    let [one, few, many] = forms;
    let word = match count % 10 {
        1 if tail != 11 => one,
        2..=4 if !(12..=14).contains(&tail) => few,
        _ => many,
    };
    format!("{count} {word}")
}

/// Склонение слова «команда» для подписей вида «1 команда», «2 команды», «5 команд».
pub(crate) fn commands_label(count: usize) -> String {
    plural(count, ["команда", "команды", "команд"])
}

/// Склонение слова «строка» для подписей вида «1 строка», «13 строк».
pub(crate) fn lines_label(count: usize) -> String {
    plural(count, ["строка", "строки", "строк"])
}

/// Разбор списка чисел из строки вида `65545, 19`.
fn parse_numbers(value: &str) -> Option<Vec<i32>> {
    let numbers: Vec<i32> = value
        .split([',', ';', ' '])
        .filter_map(|part| part.trim().parse::<i32>().ok())
        .collect();
    (!numbers.is_empty()).then_some(numbers)
}

#[cfg(test)]
mod tests {
    use super::parse_numbers;

    #[test]
    fn parses_number_lists() {
        assert_eq!(parse_numbers("65545, 19"), Some(vec![65545, 19]));
        assert_eq!(parse_numbers(" 14 ; 23 "), Some(vec![14, 23]));
        assert_eq!(parse_numbers(""), None);
        assert_eq!(parse_numbers("abc"), None);
    }
}
