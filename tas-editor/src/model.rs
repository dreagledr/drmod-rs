//! Модель скрипта TAS-редактора: DTO формата `POST /script/run` (docs/API.md §4)
//! и валидация по правилам §4.4.

use serde::{Deserialize, Serialize};

/// Максимум кадров скрипта: 3600 (60 FPS × 60 с) — ограничение мода.
pub const MAX_FRAMES: u32 = 3600;
/// `name` длиннее этого мод не примет.
pub const MAX_NAME_LEN: usize = 64;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Script {
    #[serde(default = "default_name")]
    pub name: String,
    pub commands: Vec<Command>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Trigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<Restart>,
}

fn default_name() -> String {
    "script".to_string()
}

impl Default for Script {
    fn default() -> Self {
        Self {
            name: default_name(),
            commands: Vec::new(),
            trigger: None,
            restart: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command {
    pub t: u32,
    pub duration: u32,
    pub input: Input,
    /// Условная команда (адаптивный ввод по врагу) — RLE её не трогает.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_enemy: Option<WhenEnemy>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(default, skip_serializing_if = "is_false")]
    pub forward: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub backward: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub left: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub right: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub walk: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub jump: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub light_attack: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub heavy_attack: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub blade: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ninja_run: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ar_mode: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ripper: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dodge: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub lock_on: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub subweapon: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub item: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub zandatsu: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub camera_reset: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pause: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub confirm: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub weapon_select: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub codec: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_up: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_down: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_left: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_right: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_stick: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dik_key: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_key: Option<u32>,
}

impl Input {
    /// Пустой набор — мод такой кадр считает «ввод не подаётся» (требуется хотя бы один ключ).
    pub fn is_empty(&self) -> bool {
        *self == Input::default()
    }

    /// Наложение ещё одной команды на этот кадр: флаги складываются по ИЛИ,
    /// числовые поля перезаписываются последним непустым значением.
    pub fn merge(&mut self, other: &Input) {
        for action in BitAction::ALL {
            if action.get(other) {
                action.set(self, true);
            }
        }
        if other.camera.is_some() {
            self.camera = other.camera;
        }
        if other.left_stick.is_some() {
            self.left_stick = other.left_stick;
        }
        if other.dik_key.is_some() {
            self.dik_key = other.dik_key;
        }
        if other.raw_key.is_some() {
            self.raw_key = other.raw_key;
        }
    }
}

macro_rules! bit_actions {
    ($($variant:ident => $key:literal, $label:literal, $field:ident;)*) => {
        /// Действия-флаги `input`, которые показывает матрица: одна колонка на действие.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum BitAction {
            $($variant),*
        }

        impl BitAction {
            pub const ALL: &'static [BitAction] = &[$(BitAction::$variant),*];

            /// Ключ в JSON (`input.{key}`).
            pub fn key(self) -> &'static str {
                match self {
                    $(BitAction::$variant => $key),*
                }
            }

            /// Короткая подпись колонки в таймлайне.
            pub fn label(self) -> &'static str {
                match self {
                    $(BitAction::$variant => $label),*
                }
            }

            pub fn get(self, input: &Input) -> bool {
                match self {
                    $(BitAction::$variant => input.$field),*
                }
            }

            pub fn set(self, input: &mut Input, value: bool) {
                match self {
                    $(BitAction::$variant => input.$field = value),*
                }
            }
        }
    };
}

bit_actions! {
    Forward => "forward", "fwd", forward;
    Backward => "backward", "back", backward;
    Left => "left", "left", left;
    Right => "right", "right", right;
    Walk => "walk", "walk", walk;
    Jump => "jump", "jump", jump;
    LightAttack => "light_attack", "L-atk", light_attack;
    HeavyAttack => "heavy_attack", "H-atk", heavy_attack;
    Blade => "blade", "blade", blade;
    NinjaRun => "ninja_run", "ninja", ninja_run;
    ArMode => "ar_mode", "AR", ar_mode;
    Ripper => "ripper", "rip", ripper;
    Dodge => "dodge", "dodge", dodge;
    LockOn => "lock_on", "lock", lock_on;
    Subweapon => "subweapon", "sub", subweapon;
    Item => "item", "item", item;
    Zandatsu => "zandatsu", "zdt", zandatsu;
    CameraReset => "camera_reset", "cam-0", camera_reset;
    Pause => "pause", "pause", pause;
    Confirm => "confirm", "ok", confirm;
    WeaponSelect => "weapon_select", "wpn", weapon_select;
    Codec => "codec", "codec", codec;
    MenuUp => "menu_up", "m-up", menu_up;
    MenuDown => "menu_down", "m-dn", menu_down;
    MenuLeft => "menu_left", "m-lt", menu_left;
    MenuRight => "menu_right", "m-rt", menu_right;
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhenEnemy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dist_max: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blade_dy_min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_y_min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_y_max: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_vy_max: Option<f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub repeat: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restart {
    #[serde(default = "one")]
    pub ups: u32,
    #[serde(default)]
    pub downs: u32,
    #[serde(default = "six")]
    pub hold: u32,
    #[serde(default = "twenty")]
    pub open_gap: u32,
    #[serde(default = "ten")]
    pub gap: u32,
    #[serde(default = "two")]
    pub confirms: u32,
    #[serde(default = "twenty_five")]
    pub confirm_gap: u32,
    #[serde(default = "fifteen")]
    pub tail: u32,
}

impl Default for Restart {
    fn default() -> Self {
        Self {
            ups: 1,
            downs: 0,
            hold: 6,
            open_gap: 20,
            gap: 10,
            confirms: 2,
            confirm_gap: 25,
            tail: 15,
        }
    }
}

fn one() -> u32 {
    1
}
fn two() -> u32 {
    2
}
fn six() -> u32 {
    6
}
fn ten() -> u32 {
    10
}
fn fifteen() -> u32 {
    15
}
fn twenty() -> u32 {
    20
}
fn twenty_five() -> u32 {
    25
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub severity: Severity,
    pub text: String,
}

impl Issue {
    fn error(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            text: text.into(),
        }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            text: text.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Проверка скрипта по правилам мода (docs/API.md §4.4): то, что он отклонил бы
/// с `400`, помечается как `Error`; сомнительное, но допустимое — `Warning`.
pub fn validate(script: &Script) -> Vec<Issue> {
    let mut issues = Vec::new();

    if script.name.chars().count() > MAX_NAME_LEN {
        issues.push(Issue::error(format!(
            "name длиннее {MAX_NAME_LEN} символов"
        )));
    }
    if script.commands.is_empty() {
        issues.push(Issue::error("commands пуст — мод такой скрипт отклонит"));
    }

    for (index, command) in script.commands.iter().enumerate() {
        let at = format!("команда #{} (t={})", index + 1, command.t);

        if command.duration < 1 {
            issues.push(Issue::error(format!("{at}: duration < 1")));
        }
        if command.t + command.duration > MAX_FRAMES {
            issues.push(Issue::error(format!(
                "{at}: t + duration = {} > {MAX_FRAMES} кадров",
                command.t + command.duration
            )));
        }
        if command.input.is_empty() && command.when_enemy.is_none() {
            issues.push(Issue::error(format!(
                "{at}: input пуст — нужен хотя бы один ключ"
            )));
        }
        if command.when_enemy.is_some() && command.input.is_empty() {
            issues.push(Issue::warning(format!(
                "{at}: условная команда без input — сработает вхолостую"
            )));
        }
    }

    match &script.trigger {
        Some(trigger) if trigger.pos.is_none() && trigger.ticks.is_none() => {
            issues.push(Issue::error(
                "trigger: нужен хотя бы один из ключей `pos`/`ticks`",
            ));
        }
        None => {}
        Some(_) => {}
    }

    dedup_advice(script, &mut issues);
    issues
}

/// Подсказка о соседних командах, которые мод выполнял бы одним интервалом.
fn dedup_advice(script: &Script, issues: &mut Vec<Issue>) {
    let mut unconditional: Vec<&Command> = script
        .commands
        .iter()
        .filter(|command| command.when_enemy.is_none())
        .collect();
    unconditional.sort_by_key(|command| command.t);

    for pair in unconditional.windows(2) {
        let [left, right] = pair else { continue };
        if left.input == right.input && left.t + left.duration == right.t {
            issues.push(Issue::warning(format!(
                "команды t={} и t={} одинаковы и стоят подряд — их можно слить",
                left.t, right.t
            )));
        }
    }
}

pub fn to_text(script: &Script) -> String {
    let mut text = serde_json::to_string_pretty(script).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

pub fn from_text(text: &str) -> Result<Script, String> {
    serde_json::from_str(text).map_err(|error| format!("JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Script {
        from_text(
            r#"{
              "name": "test-blade-run",
              "commands": [
                { "t": 0, "duration": 60, "input": { "forward": true, "blade": true } }
              ]
            }"#,
        )
        .expect("пример из test_inputs/blade_run.json разбирается")
    }

    #[test]
    fn parses_sample_and_round_trips() {
        let script = sample();
        assert_eq!(script.name, "test-blade-run");
        assert_eq!(script.commands.len(), 1);
        assert!(script.commands[0].input.forward);
        assert!(script.commands[0].input.blade);
        assert!(!script.commands[0].input.jump);

        let text = to_text(&script);
        assert_eq!(from_text(&text).expect("round trip"), script);
        assert!(validate(&script).iter().all(|issue| !issue.is_error()));
    }

    #[test]
    fn unknown_input_key_is_rejected() {
        let error = from_text(
            r#"{"name":"x","commands":[{"t":0,"duration":1,"input":{"jumpk":true}}]}"#,
        )
        .expect_err("опечатка в ключе должна падать");
        assert!(error.contains("unknown field"), "неожиданная ошибка: {error}");
    }

    #[test]
    fn validation_boundaries() {
        let script = Script {
            name: "n".repeat(MAX_NAME_LEN + 1),
            commands: vec![Command {
                t: MAX_FRAMES - 1,
                duration: 2,
                input: Input::default(),
                when_enemy: None,
            }],
            trigger: Some(Trigger::default()),
            restart: None,
        };
        let issues = validate(&script);
        let errors: Vec<&str> = issues
            .iter()
            .filter(|issue| issue.is_error())
            .map(|issue| issue.text.as_str())
            .collect();
        assert_eq!(errors.len(), 4, "ожидалось 4 ошибки, получено {errors:?}");
        assert!(errors.iter().any(|text| text.contains("name")));
        assert!(errors.iter().any(|text| text.contains("duration")));
        assert!(errors.iter().any(|text| text.contains("trigger")));
    }

    #[test]
    fn duration_zero_is_error() {
        let script = Script {
            commands: vec![Command {
                t: 0,
                duration: 0,
                input: Input {
                    jump: true,
                    ..Input::default()
                },
                when_enemy: None,
            }],
            ..Script::default()
        };
        assert!(validate(&script).iter().any(|issue| issue.text.contains("duration")));
    }

    #[test]
    fn adjacent_equal_commands_are_flagged() {
        let input = Input {
            forward: true,
            ..Input::default()
        };
        let script = Script {
            commands: vec![
                Command {
                    t: 0,
                    duration: 10,
                    input: input.clone(),
                    when_enemy: None,
                },
                Command {
                    t: 10,
                    duration: 5,
                    input,
                    when_enemy: None,
                },
            ],
            ..Script::default()
        };
        assert!(validate(&script)
            .iter()
            .any(|issue| issue.text.contains("слить")));
    }
}
