//! Корневой компонент редактора в мок-режиме: скрипты живут в памяти (`mock`),
//! панели показывают состояние, правки идут по цепочке
//! «матрица ↔ JSON-текст» без обращения к диску.

use crate::matrix::{FrameMatrix, NumberColumn};
use crate::mock;
use crate::model::{self, BitAction, Issue, Script, Trigger, WhenEnemy};
use crate::panels;
use windows_reactor::*;

/// Высота полосы свойств: выше — она распирает окно и выдавливает таймлайн и JSON.
const PANEL_CHROME: f64 = 96.0;
const PROPS_OPEN_HEIGHT: f64 = 320.0;
const PROPS_CLOSED_HEIGHT: f64 = 70.0;
const FALLBACK_WINDOW_HEIGHT: f64 = 900.0;
/// Доля свободной высоты, которую забирает JSON-редактор; остальное — таймлайн.
const JSON_SHARE: f64 = 0.3;

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
    pub(crate) pane_open: bool,
    pub(crate) properties_open: bool,
    /// Клиентская высота окна в DIPs: от неё считаются высоты таймлайна и JSON.
    pub(crate) window_height: f64,
}

#[derive(Clone)]
pub(crate) enum Msg {
    Resized(WindowSize),
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
        self.issues = model::validate(&self.script);
    }
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
            pane_open: true,
            properties_open: false,
            window_height: 0.0,
        };
        editor.load(0);
        editor
    }

    fn update(&mut self, message: Msg, _context: &ComponentContext<Self>) {
        match message {
            Msg::Resized(size) => self.window_height = size.height,
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
                // `RichEditBox` отдаёт CRLF/CR и дописывает хвост из сотен переводов
                // строк — нормализуем и срезаем пустой хвост, оставляя один финальный `\n`.
                let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
                self.text = format!("{}\n", normalized.trim_end_matches('\n'));
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
        // Высоты считаем от клиентской высоты окна: `ListView` не ограничивает себя сам,
        // поэтому панели получают явные размеры, заполняющие окно целиком.
        context.on_window_size(context.callback(Msg::Resized));

        let height = if self.window_height > 200.0 {
            self.window_height
        } else {
            FALLBACK_WINDOW_HEIGHT
        };
        let props = if self.properties_open {
            PROPS_OPEN_HEIGHT
        } else {
            PROPS_CLOSED_HEIGHT
        };
        let available = (height - props - PANEL_CHROME).max(320.0);
        let json_height = (available * JSON_SHARE).clamp(140.0, 380.0);
        let timeline_height = (available - json_height).max(180.0);

        let content = Grid::new()
            .rows([GridLength::Auto, GridLength::Auto, GridLength::Auto])
            .children((
                Border::new()
                    .grid_row(0)
                    .content(panels::properties::render(self, context)),
                Border::new()
                    .grid_row(1)
                    .content(panels::timeline::render(self, context, timeline_height)),
                Border::new()
                    .grid_row(2)
                    .content(panels::text_editor::render(self, context, json_height)),
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
