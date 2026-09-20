//! Таймлайн-матрица: производная от `Script.commands` модель «кадр → набор входов».
//!
//! Каноничным остаётся `Script`; матрица пересобирается из него и умеет
//! сериализоваться обратно: соседние одинаковые кадры сворачиваются в одну
//! команду (RLE), пустые кадры команд не дают. Команды с `when_enemy` матрица
//! не трогает — они живут отдельным списком и сохраняются как есть.

use crate::model::{BitAction, Command, Input, Script, WhenEnemy};

/// Числовые колонки таймлайна: камера и левый стик по осям.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberColumn {
    CameraX,
    CameraY,
    StickX,
    StickY,
}

impl NumberColumn {
    pub const ALL: &'static [NumberColumn] = &[
        NumberColumn::CameraX,
        NumberColumn::CameraY,
        NumberColumn::StickX,
        NumberColumn::StickY,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NumberColumn::CameraX => "cam_dx",
            NumberColumn::CameraY => "cam_dy",
            NumberColumn::StickX => "stick_x",
            NumberColumn::StickY => "stick_y",
        }
    }

    pub fn get(self, input: &Input) -> Option<i32> {
        match self {
            NumberColumn::CameraX => input.camera.map(|camera| camera[0]),
            NumberColumn::CameraY => input.camera.map(|camera| camera[1]),
            NumberColumn::StickX => input.left_stick.map(|stick| stick[0]),
            NumberColumn::StickY => input.left_stick.map(|stick| stick[1]),
        }
    }

    /// Записывает ось; пара осей, ставшая `[0, 0]`, убирается, чтобы не мусорить в JSON.
    pub fn set(self, input: &mut Input, value: Option<i32>) {
        match self {
            NumberColumn::CameraX | NumberColumn::CameraY => {
                let axis = usize::from(self == NumberColumn::CameraY);
                let mut pair = input.camera.unwrap_or([0, 0]);
                pair[axis] = value.unwrap_or(0);
                input.camera = (pair != [0, 0]).then_some(pair);
            }
            NumberColumn::StickX | NumberColumn::StickY => {
                let axis = usize::from(self == NumberColumn::StickY);
                let mut pair = input.left_stick.unwrap_or([0, 0]);
                pair[axis] = value.unwrap_or(0);
                input.left_stick = (pair != [0, 0]).then_some(pair);
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameMatrix {
    /// Кадры 0..len; пустой `Input` = «в этом кадре ввод не подаётся».
    frames: Vec<Input>,
    /// Команды с `when_enemy` — вне RLE, показываются в свойствах.
    conditional: Vec<Command>,
}

impl FrameMatrix {
    pub fn from_script(script: &Script) -> Self {
        let length = script
            .commands
            .iter()
            .filter(|command| command.when_enemy.is_none())
            .map(|command| command.t + command.duration)
            .max()
            .unwrap_or(0);

        let mut frames = vec![Input::default(); length as usize];
        for command in script
            .commands
            .iter()
            .filter(|command| command.when_enemy.is_none())
        {
            for frame in command.t..command.t.saturating_add(command.duration) {
                if let Some(slot) = frames.get_mut(frame as usize) {
                    slot.merge(&command.input);
                }
            }
        }

        Self {
            frames,
            conditional: script
                .commands
                .iter()
                .filter(|command| command.when_enemy.is_some())
                .cloned()
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn frame(&self, index: usize) -> Option<&Input> {
        self.frames.get(index)
    }

    /// Кадр, который показывает матрица: за пределами длины — пустой набор.
    fn frame_mut(&mut self, index: usize) -> &mut Input {
        if index >= self.frames.len() {
            self.frames.resize(index + 1, Input::default());
        }
        &mut self.frames[index]
    }

    pub fn bit(&self, index: usize, action: BitAction) -> bool {
        self.frame(index)
            .is_some_and(|frame| action.get(frame))
    }

    pub fn set_bit(&mut self, index: usize, action: BitAction, value: bool) {
        action.set(self.frame_mut(index), value);
    }

    pub fn number(&self, index: usize, column: NumberColumn) -> Option<i32> {
        self.frame(index).and_then(|frame| column.get(frame))
    }

    pub fn set_number(&mut self, index: usize, column: NumberColumn, value: Option<i32>) {
        column.set(self.frame_mut(index), value);
    }

    pub fn push_frame(&mut self) {
        self.frames.push(Input::default());
    }

    /// Убирает последний кадр; если последний кадр непустой — это потеря ввода,
    /// поэтому вызывающий должен подтверждать такое действие.
    pub fn pop_frame(&mut self) -> bool {
        match self.frames.pop() {
            Some(frame) => !frame.is_empty(),
            None => false,
        }
    }

    pub fn conditional(&self) -> &[Command] {
        &self.conditional
    }

    pub fn set_conditional(&mut self, index: usize, when_enemy: WhenEnemy) {
        if let Some(command) = self.conditional.get_mut(index) {
            command.when_enemy = Some(when_enemy);
        }
    }

    pub fn remove_conditional(&mut self, index: usize) {
        if index < self.conditional.len() {
            self.conditional.remove(index);
        }
    }

    /// Свернуть кадры в команды: соседние одинаковые — один интервал, пустые пропускаются.
    pub fn to_commands(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        let mut start = 0usize;

        while start < self.frames.len() {
            let mut end = start + 1;
            while end < self.frames.len() && self.frames[end] == self.frames[start] {
                end += 1;
            }
            if !self.frames[start].is_empty() {
                commands.push(Command {
                    t: start as u32,
                    duration: (end - start) as u32,
                    input: self.frames[start].clone(),
                    when_enemy: None,
                });
            }
            start = end;
        }

        commands
    }

    /// Переносит матрицу в скрипт: безусловные команды пересобираются, условные остаются.
    pub fn apply_to(&self, script: &mut Script) {
        script.commands = self.to_commands();
        script.commands.extend(self.conditional.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Input, from_text};

    fn script_of(json: &str) -> Script {
        from_text(json).expect("тестовый JSON разбирается")
    }

    #[test]
    fn intervals_become_frames() {
        let script = script_of(
            r#"{"name":"a","commands":[
                {"t":0,"duration":60,"input":{"forward":true,"blade":true}},
                {"t":45,"duration":2,"input":{"jump":true}}
            ]}"#,
        );
        let matrix = FrameMatrix::from_script(&script);

        // Длину задаёт самая дальняя граница: 0..60 и прыжок внутри него.
        assert_eq!(matrix.len(), 60);
        assert!(matrix.bit(0, BitAction::Forward));
        assert!(matrix.bit(59, BitAction::Blade));
        assert!(!matrix.bit(0, BitAction::Jump));
        assert!(matrix.bit(45, BitAction::Jump));
        assert!(matrix.bit(46, BitAction::Jump));
        assert!(!matrix.bit(47, BitAction::Jump));
    }

    #[test]
    fn round_trip_is_idempotent() {
        let script = script_of(
            r#"{"name":"a","commands":[
                {"t":0,"duration":60,"input":{"forward":true,"left_stick":[0,-1000]}},
                {"t":70,"duration":5,"input":{"jump":true}}
            ],"trigger":{"pos":[-24.7,12.1,120.7]}}"#,
        );
        let mut matrix = FrameMatrix::from_script(&script);
        let mut rebuilt = script.clone();
        matrix.apply_to(&mut rebuilt);
        matrix = FrameMatrix::from_script(&rebuilt);

        let mut second = rebuilt.clone();
        matrix.apply_to(&mut second);
        assert_eq!(rebuilt, second, "повторный проход не должен менять скрипт");
        assert_eq!(rebuilt.trigger, script.trigger);
        assert_eq!(rebuilt.commands.len(), 2);
    }

    #[test]
    fn toggle_inside_interval_splits_command() {
        let script = script_of(
            r#"{"name":"a","commands":[{"t":0,"duration":10,"input":{"forward":true}}]}"#,
        );
        let mut matrix = FrameMatrix::from_script(&script);
        matrix.set_bit(4, BitAction::Forward, false);

        let mut rebuilt = script.clone();
        matrix.apply_to(&mut rebuilt);
        assert_eq!(rebuilt.commands.len(), 2, "интервал должен разорваться");
        assert_eq!((rebuilt.commands[0].t, rebuilt.commands[0].duration), (0, 4));
        assert_eq!((rebuilt.commands[1].t, rebuilt.commands[1].duration), (5, 5));
    }

    #[test]
    fn toggle_on_empty_frame_creates_single_command() {
        let script = script_of(r#"{"name":"a","commands":[]}"#);
        let mut matrix = FrameMatrix::from_script(&script);
        assert_eq!(matrix.len(), 0, "пустой скрипт — пустая матрица");

        matrix.set_bit(0, BitAction::Jump, true);
        let mut rebuilt = script.clone();
        matrix.apply_to(&mut rebuilt);

        assert_eq!(rebuilt.commands.len(), 1);
        assert_eq!(rebuilt.commands[0].t, 0);
        assert_eq!(rebuilt.commands[0].duration, 1);
        assert!(rebuilt.commands[0].input.jump);
    }

    #[test]
    fn empty_frames_do_not_become_commands() {
        let script = script_of(
            r#"{"name":"a","commands":[
                {"t":0,"duration":5,"input":{"forward":true}},
                {"t":20,"duration":5,"input":{"forward":true}}
            ]}"#,
        );
        let matrix = FrameMatrix::from_script(&script);
        let commands = matrix.to_commands();
        assert_eq!(commands.len(), 2, "провал между командами не заполняется");
        assert_eq!(commands[1].t, 20);
    }

    #[test]
    fn conditional_commands_survive_rle() {
        let script = script_of(
            r#"{"name":"a","commands":[
                {"t":0,"duration":10,"input":{"forward":true}},
                {"t":74,"duration":24,"input":{"forward":true,"heavy_attack":true},
                 "when_enemy":{"anim":[65545],"player_y_min":0.3,"player_y_max":0.8,"player_vy_max":0.0}}
            ]}"#,
        );
        let matrix = FrameMatrix::from_script(&script);
        assert_eq!(matrix.len(), 10, "условная команда длину матрицы не задаёт");
        assert_eq!(matrix.conditional().len(), 1);

        let mut rebuilt = script_of(r#"{"name":"a","commands":[]}"#);
        matrix.apply_to(&mut rebuilt);
        assert_eq!(rebuilt.commands.len(), 2);
        let conditional = rebuilt
            .commands
            .iter()
            .find(|command| command.when_enemy.is_some())
            .expect("условная команда сохранена");
        assert_eq!(conditional.t, 74);
        assert_eq!(conditional.when_enemy.as_ref().unwrap().anim, Some(vec![65545]));
    }

    #[test]
    fn zero_camera_pair_is_dropped() {
        let mut input = Input::default();
        NumberColumn::CameraX.set(&mut input, Some(300));
        assert_eq!(input.camera, Some([300, 0]));
        NumberColumn::CameraX.set(&mut input, Some(0));
        assert_eq!(input.camera, None, "пара нулей не должна оставаться в JSON");
    }

    #[test]
    fn push_and_pop_frames() {
        let mut matrix = FrameMatrix::from_script(&script_of(
            r#"{"name":"a","commands":[{"t":0,"duration":3,"input":{"jump":true}}]}"#,
        ));
        assert_eq!(matrix.len(), 3);
        matrix.push_frame();
        assert_eq!(matrix.len(), 4);
        assert!(!matrix.pop_frame(), "пустой кадр пропадает молча");
        assert!(matrix.pop_frame(), "непустой кадр требует подтверждения");
        assert_eq!(matrix.len(), 2);
        assert!(matrix.pop_frame());
    }
}
