//! DTO скрипта ввода — тело `POST /script/run` (формат: docs/API.md §4).
//!
//! Разделяются между модом (`src/api.rs` — десериализация запроса) и
//! инструментами (`tools/script_gen` — генерация фикстур и приёмка JSON,
//! который пишет редактор): определение в одном месте гарантирует, что
//! сгенерированный JSON примет мод.
//!
//! `Serialize` реализован ради инструментов: поля с дефолтами/`None`
//! пропускаются, чтобы фикстуры читались как примеры из §4.

use serde::{Deserialize, Serialize};

/// Лимит длительности скрипта в кадрах (60 с при 60 FPS) — `t + duration`
/// команды не может его превысить.
pub const MAX_SCRIPT_FRAMES: u32 = 3600;

/// Вход одной команды скрипта (JSON-объект `input`). Все поля опциональны;
/// неизвестные ключи — ошибка (защита от опечаток LLM).
///
/// Семантика (см. docs/API.md §4.2):
/// - движение (`forward`/`backward`/`left`/`right`) — биты InputUnit + left_stick;
/// - hold-действия (`ninja_run`/`walk`/`dodge`) — удержание keybind'а
///   на все кадры команды (isKeybindDown);
/// - `blade` — бит 0x800 в InputUnit (как ninja 0x4000): игра кодирует блейд
///   этим битом, keybind-эмуляция isKeybindDown(8) для скриптов не работает
///   (игра читает её только в key-event обработке);
/// - битовые (`jump`/`light_attack`/`heavy_attack`/`ar_mode`/`weapon_select`) — бит
///   в InputUnit + фронт pressed на первом кадре команды;
/// - pressed-действия (`ripper`/`lock_on`/`subweapon`/`item`/
///   `codec`/`camera_reset`/`zandatsu`) — фронт keybind'а
///   на первом кадре команды (isKeybindPressed), `duration` игнорируется;
/// - меню-клавиши (`pause`/`confirm`/`menu_up`/`menu_down`/`menu_left`/
///   `menu_right`) — биты геймпада в InputUnit + фронт pressed: `pause` —
///   START-bit 0x100 (реальный Esc), `confirm` — BUTTON_A 0x10, `menu_*` —
///   D-Pad 0x8/0x4/0x1/0x2. Подача через кэш `ms_KeyInput` не работает
///   (docs/API.md §10.3).
#[derive(Clone, Copy, Default, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptInput {
    #[serde(default, skip_serializing_if = "is_false")]
    pub forward: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub backward: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub left: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub right: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub jump: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub light_attack: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub heavy_attack: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ripper: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub blade: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ninja_run: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub walk: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dodge: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub lock_on: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub subweapon: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub item: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ar_mode: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub weapon_select: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub codec: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub zandatsu: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub camera_reset: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pause: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub confirm: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_up: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_down: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_left: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub menu_right: bool,
    /// Сырой игровой код клавиши (docs/REPLAY.md §2.1) — эмуляция через
    /// детуры `isKeyDown`/`isKeyPressed`. Нужен для меню: в паузе игра не
    /// гоняет тик ввода, поэтому InputUnit-override до меню не доходит, а
    /// клавиши меню читаются как раз через `isKeyDown`. Пример: `139` (0x8B).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_key: Option<u32>,
    /// Сырой DIK-код клавиши DirectInput (`ms_InputKeys[dik] = 0x80`) — второй
    /// канал для меню: игра маппит DIK в игровой код сама. Пример: 0xD0 —
    /// стрелка вниз (DIK_DOWN), 0xC8 — вверх.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dik_key: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_stick: Option<[f32; 2]>,
}

impl ScriptInput {
    /// Пустой ли вход (ни один ключ не задан).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Одна команда скрипта: входы активны с кадра `t` на `duration` кадров.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ScriptCommand {
    pub t: u32,
    pub duration: u32,
    pub input: ScriptInput,
    /// Условие по состоянию врага: команда ждёт его выполнения, затем «стреляет»
    /// (её `t` заменяется кадром срабатывания). Обычные команды — без условия.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_enemy: Option<EnemyCondition>,
}

/// Условие команды по состоянию врага (адаптивный ввод).
///
/// Фиксированные кадры дают подброс лишь в 5–20% прогонов (бимодально: 23–35 м
/// или 2–3 м) — решает состояние врага: подброс даёт парирование его «прыжка на
/// игрока». Такая команда «спит», пока условие не выполнено, затем срабатывает
/// на `duration` кадров от кадра срабатывания (фронт — на первом).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyCondition {
    /// Анимации врага (+0x618), в одной из которых команда разрешена.
    /// Пусто — любая анимация: триггер может быть чисто по высоте игрока.
    /// Известные: 19 «выпад», 65545 «прыжок», 24 — попадание по врагу.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anim: Vec<i32>,
    /// Минимальный кадр анимации врага (+0x8B4).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frame_min: i32,
    #[serde(default = "i32_max", skip_serializing_if = "is_i32_max")]
    pub frame_max: i32,
    /// Максимальная дистанция от игрока до врага (м) — удар должен доставать.
    #[serde(default = "f32_max", skip_serializing_if = "is_f32_max")]
    pub dist_max: f32,
    /// Минимальное превышение КЛИНКА врага над игроком (м). Тело врага всегда
    /// на земле (`pos.y`), в атаке поднимается клинок (`blade_y`: пик «прыжка»
    /// ~2.97 м) — подброс направляет вверх, только когда клинок сверху, а игрок
    /// ниже; по телу врага условие не сработало бы никогда.
    #[serde(default = "f32_min", skip_serializing_if = "is_f32_min")]
    pub blade_dy_min: f32,
    /// Высота игрока (м) для срабатывания: удар обязан быть **в прыжке** — только
    /// он даёт подброс, — но невысоко: при высоком ударе лаунч уходит
    /// горизонтально, при низком (y≈0.5–0.7, как в записи 142) — вверх.
    #[serde(default = "f32_min", skip_serializing_if = "is_f32_min")]
    pub player_y_min: f32,
    #[serde(default = "f32_max", skip_serializing_if = "is_f32_max")]
    pub player_y_max: f32,
    /// Вертикальная скорость игрока (м/с): 0 — **только на падении** (удар на
    /// подъёме не годится — подброс нужен на спуске, когда враг уже наверху).
    #[serde(default = "f32_max", skip_serializing_if = "is_f32_max")]
    pub player_vy_max: f32,
    /// Повторять команду, пока условие держится (атака «спамом»: окно
    /// парирования узкое, одна попытка попадает в него лишь в ~1/3 случаев).
    #[serde(default, skip_serializing_if = "is_false")]
    pub repeat: bool,
}

impl Default for EnemyCondition {
    /// Все границы — в «без ограничения»: условие без полей разрешает любую
    /// анимацию при любом состоянии (docs/API.md §4.1).
    fn default() -> Self {
        Self {
            anim: Vec::new(),
            frame_min: 0,
            frame_max: i32_max(),
            dist_max: f32_max(),
            blade_dy_min: f32_min(),
            player_y_min: f32_min(),
            player_y_max: f32_max(),
            player_vy_max: f32_max(),
            repeat: false,
        }
    }
}

fn f32_min() -> f32 {
    f32::MIN
}

fn i32_max() -> i32 {
    i32::MAX
}

fn f32_max() -> f32 {
    f32::MAX
}

fn is_i32_max(v: &i32) -> bool {
    *v == i32::MAX
}

/// Кадры анимации начинаются с нуля, поэтому `frame_min = 0` — отсутствие
/// нижней границы: в JSON его не пишем.
fn is_zero(v: &i32) -> bool {
    *v == 0
}

fn is_f32_max(v: &f32) -> bool {
    *v == f32::MAX
}

fn is_f32_min(v: &f32) -> bool {
    *v == f32::MIN
}

/// Триггер старта скрипта: скрипт взводится (`Armed`) и стартует либо когда
/// позиция игрока попадает в зону вокруг `pos` (допуск как у отложенного
/// старта record/playback: ±0.1 м по X/Z, ±1.0 м по Y), либо через `ticks`
/// тиков симуляции после взвода.
///
/// Тиковый старт нужен для воспроизводимости: позиционный триггер срабатывает
/// на 0–1-м тике после загрузки (плюс-минус тик), а сдвиг старта на тик меняет
/// фазу врага и «дрожит» исход. Тиковый же привязан к тикам симуляции — фаза
/// одинаковая от прогона к прогону.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptTrigger {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks: Option<u64>,
}

/// Тело `POST /script/run` (JSON).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptRequest {
    #[serde(default = "default_script_name")]
    pub name: String,
    pub commands: Vec<ScriptCommand>,
    /// Если задан — скрипт взводится и стартует по попаданию в зону;
    /// иначе запускается сразу (текущее поведение).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<ScriptTrigger>,
    /// Если задан — перед взводом скрипт сам рестартует миссию через меню
    /// паузы (см. `RestartSpec`). Один активный скрипт на мод, поэтому рестарт
    /// и полёт делаются одной фазой одного скрипта.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<RestartSpec>,
}

fn default_script_name() -> String {
    "script".to_string()
}

/// Параметры рестарта миссии из меню паузы (поле `restart` запроса).
///
/// Схема (проверено live 2026-09-10 на P118_BEACH): `pause` (START) открывает
/// меню; курсор ходит клавишами DirectInput (`dik`, мод подмешивает их после
/// опроса устройства — работает только при окне игры в фокусе); пункт Restart
/// открывает диалог «Restart from last checkpoint?» с уже выбранным YES,
/// поэтому нужно два `confirm`.
///
/// Команды фазы рестарта строит мод (`src/api.rs` — `restart_commands`): тип
/// общий, а DIK-коды клавиш меню принадлежат моду.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartSpec {
    /// Нажатий «вверх» (в меню паузы Restart — нижний пункт).
    #[serde(default = "one")]
    pub ups: u32,
    #[serde(default)]
    pub downs: u32,
    /// Кадров удержания стрелки.
    #[serde(default = "six")]
    pub hold: u32,
    /// Пауза после `pause`: меню должно успеть открыться (короткая пауза —
    /// стрелка приходит в анимацию и теряется).
    #[serde(default = "twenty")]
    pub open_gap: u32,
    /// Пауза между стрелками и подтверждением.
    #[serde(default = "ten")]
    pub gap: u32,
    /// Подтверждений подряд (2: пункт Restart + диалог YES).
    #[serde(default = "two")]
    pub confirms: u32,
    /// Пауза между подтверждениями (диалог должен появиться).
    #[serde(default = "twenty_five")]
    pub confirm_gap: u32,
    /// Хвост после последнего подтверждения (кадров).
    #[serde(default = "fifteen")]
    pub tail: u32,
}

impl Default for RestartSpec {
    fn default() -> Self {
        Self {
            ups: one(),
            downs: 0,
            hold: six(),
            open_gap: twenty(),
            gap: ten(),
            confirms: two(),
            confirm_gap: twenty_five(),
            tail: fifteen(),
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
