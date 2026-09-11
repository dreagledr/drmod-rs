//! HTTP API автоматизации: скрипты ввода, состояние игры, кольцевой буфер логов.
//!
//! Сервер слушает `127.0.0.1:5223` (собственный минимальный HTTP-сервер на
//! `TcpListener`) в отдельном потоке. Render-цикл продвигает активный скрипт
//! (подача ввода через `replay::set_input_override` и keybind-эмуляцию
//! ripper/blade), пишет кадры в кольцевой буфер и обновляет снимок `/state`.
//! HTTP-поток только читает `SharedState` (Arc<Mutex>).
//!
//! Собственный сервер вместо tiny_http: tiny_http не выставляет таймауты на
//! сокетах (клиент может заблокировать поток навсегда в `respond`/чтении тела)
//! и порождает неуправляемые внутренние потоки (accept + TaskPool + на каждое
//! соединение), которые остаются живыми на выгруженном коде DLL при eject.
//! Здесь один поток, неблокирующий accept с опросом stop-флага и таймауты
//! read/write на каждом соединении — `shutdown()` гарантированно завершает
//! поток за ограниченное время.
//!
//! Дизайн — `docs/API.md`.

use crate::logger;
use crate::segment;
use crate::tas::addresses;
use crate::tas::hooks;
use crate::tas::replay;
use crate::tas::types::{InputOverride, InputUnit, PlayerState};
use crate::ui::UiState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Адрес и порт HTTP API.
const BIND_ADDR: &str = "127.0.0.1:5223";
/// Максимальная длительность скрипта в кадрах (60 сек при 60 FPS).
const MAX_SCRIPT_FRAMES: u32 = 3600;
/// Ёмкость кольцевого буфера логов (60 FPS × 60 сек).
const RING_CAPACITY: usize = 3600;
/// Лимит тела запроса (защита от гигантских скриптов).
const MAX_BODY_BYTES: u64 = 64 * 1024;
/// Максимум кадров в ответе /logs.
const MAX_LOG_LIMIT: usize = 5000;
/// Таймаут чтения/записи на клиентском сокете: клиент, который не читает
/// ответ или не досылает тело, не может заблокировать HTTP-поток навсегда
/// (иначе join в shutdown() зависнет на выгрузке DLL).
const IO_TIMEOUT: Duration = Duration::from_secs(1);
/// Пауза accept-цикла при отсутствии соединений (неблокирующий listener) —
/// shutdown() будит поток максимум за это время.
const ACCEPT_POLL_MS: u64 = 10;
/// Лимит заголовков запроса (защита от гигантских заголовков).
const MAX_HEADER_BYTES: usize = 16 * 1024;

/// Встроенный скрипт NumPad4: бег ~1 сек → прыжок на бегу → лёгкий удар →
/// поворот камеры. Ровно текущее поведение `update_input_injection`.
/// В release используется только через HTTP (POST /script/run) — отсюда allow.
#[allow(dead_code)]
const BUILTIN_SCRIPT: &str = r#"{
  "name": "run-jump-attack",
  "commands": [
    { "t": 0,  "duration": 60, "input": { "forward": true } },
    { "t": 45, "duration": 2,  "input": { "jump": true } },
    { "t": 85, "duration": 2,  "input": { "light_attack": true } },
    { "t": 95, "duration": 35, "input": { "camera": [300, 0] } }
  ]
}"#;

/// Статус скрипта.
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum ScriptStatus {
    /// Идёт фаза рестарта миссии (поле `restart`) — меню паузы, стрелки, confirm.
    Restarting,
    /// Взведён с триггером старта по позиции — ждёт попадания игрока в зону.
    Armed,
    Running,
    Done,
    Stopped,
}

impl ScriptStatus {
    /// Активен ли скрипт: держит слот, второй `POST /script/run` получает 409.
    fn is_active(self) -> bool {
        matches!(self, Self::Restarting | Self::Armed | Self::Running)
    }
}

/// Имя фазы скрипта для `/logs` (совпадает с JSON-статусом в `/script/{id}`).
fn script_phase_name(status: ScriptStatus) -> &'static str {
    match status {
        ScriptStatus::Restarting => "restarting",
        ScriptStatus::Armed => "armed",
        ScriptStatus::Running => "running",
        ScriptStatus::Done => "done",
        ScriptStatus::Stopped => "stopped",
    }
}

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
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptInput {
    #[serde(default)]
    forward: bool,
    #[serde(default)]
    backward: bool,
    #[serde(default)]
    left: bool,
    #[serde(default)]
    right: bool,
    #[serde(default)]
    jump: bool,
    #[serde(default)]
    light_attack: bool,
    #[serde(default)]
    heavy_attack: bool,
    #[serde(default)]
    camera: Option<[f32; 2]>,
    #[serde(default)]
    ripper: bool,
    #[serde(default)]
    blade: bool,
    #[serde(default)]
    ninja_run: bool,
    #[serde(default)]
    walk: bool,
    #[serde(default)]
    dodge: bool,
    #[serde(default)]
    lock_on: bool,
    #[serde(default)]
    subweapon: bool,
    #[serde(default)]
    item: bool,
    #[serde(default)]
    ar_mode: bool,
    #[serde(default)]
    weapon_select: bool,
    #[serde(default)]
    codec: bool,
    #[serde(default)]
    zandatsu: bool,
    #[serde(default)]
    camera_reset: bool,
    #[serde(default)]
    pause: bool,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    menu_up: bool,
    #[serde(default)]
    menu_down: bool,
    #[serde(default)]
    menu_left: bool,
    #[serde(default)]
    menu_right: bool,
    /// Сырой игровой код клавиши (docs/REPLAY.md §2.1) — эмуляция через
    /// детуры `isKeyDown`/`isKeyPressed`. Нужен для меню: в паузе игра не
    /// гоняет тик ввода, поэтому InputUnit-override до меню не доходит, а
    /// клавиши меню читаются как раз через `isKeyDown`. Пример: `139` (0x8B).
    #[serde(default)]
    raw_key: Option<u32>,
    /// Сырой DIK-код клавиши DirectInput (`ms_InputKeys[dik] = 0x80`) — второй
    /// канал для меню: игра маппит DIK в игровой код сама. Пример: 0xD0 —
    /// стрелка вниз (DIK_DOWN), 0xC8 — вверх.
    #[serde(default)]
    dik_key: Option<u32>,
    #[serde(default)]
    left_stick: Option<[f32; 2]>,
}

impl ScriptInput {
    /// Пустой ли вход (ни один ключ не задан).
    fn is_empty(&self) -> bool {
        !self.forward
            && !self.backward
            && !self.left
            && !self.right
            && !self.jump
            && !self.light_attack
            && !self.heavy_attack
            && !self.ripper
            && !self.blade
            && !self.ninja_run
            && !self.walk
            && !self.dodge
            && !self.lock_on
            && !self.subweapon
            && !self.item
            && !self.ar_mode
            && !self.weapon_select
            && !self.codec
            && !self.zandatsu
            && !self.camera_reset
            && !self.pause
            && !self.confirm
            && !self.menu_up
            && !self.menu_down
            && !self.menu_left
            && !self.menu_right
            && self.camera.is_none()
            && self.left_stick.is_none()
            && self.raw_key.is_none()
            && self.dik_key.is_none()
    }
}

/// Одна команда скрипта: входы активны с кадра `t` на `duration` кадров.
#[derive(Clone, Deserialize)]
struct ScriptCommand {
    t: u32,
    duration: u32,
    input: ScriptInput,
    /// Условие по состоянию врага: команда ждёт его выполнения, затем «стреляет»
    /// (её `t` заменяется кадром срабатывания). Обычные команды — без условия.
    #[serde(default)]
    when_enemy: Option<EnemyCondition>,
}

/// Условие команды по состоянию врага (адаптивный ввод).
///
/// Фиксированные кадры дают подброс лишь в 5–20% прогонов (бимодально: 23–35 м
/// или 2–3 м) — решает состояние врага: подброс даёт парирование его «прыжка на
/// игрока». Такая команда «спит», пока условие не выполнено, затем срабатывает
/// на `duration` кадров от кадра срабатывания (фронт — на первом).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnemyCondition {
    /// Анимации врага (+0x618), в одной из которых команда разрешена.
    /// Пусто — любая анимация: триггер может быть чисто по высоте игрока.
    /// Известные: 19 «выпад», 65545 «прыжок», 24 — попадание по врагу.
    #[serde(default)]
    anim: Vec<i32>,
    /// Минимальный кадр анимации врага (+0x8B4).
    #[serde(default)]
    frame_min: i32,
    #[serde(default = "i32_max")]
    frame_max: i32,
    /// Максимальная дистанция от игрока до врага (м) — удар должен доставать.
    #[serde(default = "f32_max")]
    dist_max: f32,
    /// Минимальное превышение КЛИНКА врага над игроком (м). Тело врага всегда
    /// на земле (`pos.y`), в атаке поднимается клинок (`blade_y`: пик «прыжка»
    /// ~2.97 м) — подброс направляет вверх, только когда клинок сверху, а игрок
    /// ниже; по телу врага условие не сработало бы никогда.
    #[serde(default = "f32_min")]
    blade_dy_min: f32,
    /// Высота игрока (м) для срабатывания: удар обязан быть **в прыжке** — только
    /// он даёт подброс, — но невысоко: при высоком ударе лаунч уходит
    /// горизонтально, при низком (y≈0.5–0.7, как в записи 142) — вверх.
    #[serde(default = "f32_min")]
    player_y_min: f32,
    #[serde(default = "f32_max")]
    player_y_max: f32,
    /// Вертикальная скорость игрока (м/с): 0 — **только на падении** (удар на
    /// подъёме не годится — подброс нужен на спуске, когда враг уже наверху).
    #[serde(default = "f32_max")]
    player_vy_max: f32,
    /// Повторять команду, пока условие держится (атака «спамом»: окно
    /// парирования узкое, одна попытка попадает в него лишь в ~1/3 случаев).
    #[serde(default)]
    repeat: bool,
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

/// Выполнено ли условие по врагу.
fn enemy_condition_ok(
    cond: &EnemyCondition,
    enemy: &crate::tas::types::EnemyState,
    player_pos: [f32; 3],
    player_vel_y: f32,
) -> bool {
    if enemy.found == 0 || (!cond.anim.is_empty() && !cond.anim.contains(&enemy.r_anim)) {
        return false;
    }
    if enemy.frame < cond.frame_min || enemy.frame > cond.frame_max {
        return false;
    }
    let dy = enemy.pos[1] - player_pos[1];
    if enemy.blade_y - player_pos[1] < cond.blade_dy_min {
        return false;
    }
    if player_pos[1] < cond.player_y_min || player_pos[1] > cond.player_y_max {
        return false;
    }
    if player_vel_y > cond.player_vy_max {
        return false;
    }
    if cond.dist_max < f32::MAX {
        let dx = enemy.pos[0] - player_pos[0];
        let dz = enemy.pos[2] - player_pos[2];
        if (dx * dx + dy * dy + dz * dz).sqrt() > cond.dist_max {
            return false;
        }
    }
    true
}

/// Триггер старта скрипта: скрипт взводится (`Armed`) и стартует, когда
/// позиция игрока попадает в зону вокруг `pos` (допуск как у отложенного
/// старта record/playback: ±0.1 м по X/Z, ±1.0 м по Y).
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptTrigger {
    pos: [f32; 3],
}

/// Тело `POST /script/run` (JSON).
#[derive(Deserialize)]
struct ScriptRequest {
    #[serde(default = "default_script_name")]
    name: String,
    commands: Vec<ScriptCommand>,
    /// Если задан — скрипт взводится и стартует по попаданию в зону;
    /// иначе запускается сразу (текущее поведение).
    #[serde(default)]
    trigger: Option<ScriptTrigger>,
    /// Если задан — перед взводом скрипт сам рестартует миссию через меню
    /// паузы (см. `RestartSpec`). Один активный скрипт на мод, поэтому рестарт
    /// и полёт делаются одной фазой одного скрипта.
    #[serde(default)]
    restart: Option<RestartSpec>,
}

/// Параметры рестарта миссии из меню паузы (поле `restart` запроса).
///
/// Схема (проверено live 2026-09-10 на P118_BEACH): `pause` (START) открывает
/// меню; курсор ходит клавишами DirectInput (`dik`, мод подмешивает их после
/// опроса устройства — работает только при окне игры в фокусе); пункт Restart
/// открывает диалог «Restart from last checkpoint?» с уже выбранным YES,
/// поэтому нужно два `confirm`.
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartSpec {
    /// Нажатий «вверх» (в меню паузы Restart — нижний пункт).
    #[serde(default = "one")]
    ups: u32,
    #[serde(default)]
    downs: u32,
    /// Кадров удержания стрелки.
    #[serde(default = "six")]
    hold: u32,
    /// Пауза после `pause`: меню должно успеть открыться (короткая пауза —
    /// стрелка приходит в анимацию и теряется).
    #[serde(default = "twenty")]
    open_gap: u32,
    /// Пауза между стрелками и подтверждением.
    #[serde(default = "ten")]
    gap: u32,
    /// Подтверждений подряд (2: пункт Restart + диалог YES).
    #[serde(default = "two")]
    confirms: u32,
    /// Пауза между подтверждениями (диалог должен появиться).
    #[serde(default = "twenty_five")]
    confirm_gap: u32,
    /// Хвост после последнего подтверждения (кадров).
    #[serde(default = "fifteen")]
    tail: u32,
}

fn one() -> u32 { 1 }
fn two() -> u32 { 2 }
fn six() -> u32 { 6 }
fn ten() -> u32 { 10 }
fn fifteen() -> u32 { 15 }
fn twenty() -> u32 { 20 }
fn twenty_five() -> u32 { 25 }

impl RestartSpec {
    /// Команды фазы рестарта: pause → стрелки → confirm ×N.
    fn commands(self) -> Vec<ScriptCommand> {
        let menu = |dik: u32, t: u32| ScriptCommand {
            t,
            duration: self.hold,
            input: ScriptInput { dik_key: Some(dik), ..Default::default() },
            when_enemy: None,
        };
        let confirm = |t: u32| ScriptCommand {
            t,
            duration: 3,
            input: ScriptInput { confirm: true, ..Default::default() },
            when_enemy: None,
        };
        let pause = ScriptCommand {
            t: 0,
            duration: 3,
            input: ScriptInput { pause: true, ..Default::default() },
            when_enemy: None,
        };
        let mut cmds = vec![pause];
        let mut t = 3 + self.open_gap;
        for _ in 0..self.ups {
            cmds.push(menu(addresses::DIK_UP, t));
            t += self.hold + self.gap;
        }
        for _ in 0..self.downs {
            cmds.push(menu(addresses::DIK_DOWN, t));
            t += self.hold + self.gap;
        }
        for i in 0..self.confirms {
            cmds.push(confirm(t));
            let last = i + 1 == self.confirms;
            t += 3 + if last { self.tail } else { self.confirm_gap };
        }
        cmds
    }
}

fn default_script_name() -> String {
    "script".to_string()
}

/// Сколько кадров после конца рестарт-последовательности ждать loading, прежде
/// чем признать рестарт неудавшимся (~4 с: загрузка миссии начинается в пределах
/// секунды после `confirm`).
const RESTART_LOADING_WAIT: u32 = 240;

/// Фаза, в которую переходит скрипт после рестарта: команды пользователя и
/// статус (Armed — ждать триггер, либо Running — если триггера нет).
struct PendingScript {
    commands: Vec<ScriptCommand>,
    total_frames: u32,
    status: ScriptStatus,
}

/// Активный (или последний) скрипт.
struct ScriptState {
    id: u32,
    name: String,
    commands: Vec<ScriptCommand>,
    frame: u32,
    status: ScriptStatus,
    total_frames: u32,
    /// Точка триггера старта (если скрипт взведён через `trigger`).
    trigger: Option<segment::Vec3>,
    /// Следующая фаза (заполняется, когда скрипт стартует с рестарта).
    pending: Option<PendingScript>,
    /// Игрок был найден за время скрипта: отличает реальный loading (надо
    /// остановить) от меню, в котором скрипт и стартовал (меню-скрипты).
    player_was_found: bool,
    /// Кадры срабатывания условных команд (`when_enemy`) — по индексу команды:
    /// условная команда срабатывает один раз.
    fired_at: Vec<Option<u32>>,
}

impl ScriptState {
    /// Список «условные команды ещё не сработали».
    fn new_fired(commands: &[ScriptCommand]) -> Vec<Option<u32>> {
        vec![None; commands.len()]
    }
}

/// Один кадр кольцевого буфера (сырые данные, JSON-форма — `LogFrameJson`).
#[derive(Clone)]
struct LogFrame {
    t_ms: u64,
    frame: u32,
    script_id: Option<u32>,
    /// Статус меню на кадре — без него нельзя проверить сценарии меню
    /// (пауза/рестарт): переходы InGame→PauseMenu→NONE видны только здесь.
    menu_status: &'static str,
    /// Фаза скрипта на кадре (`restarting`/`armed`/`running`/`done`): кадры
    /// до загрузки принадлежат фазе рестарта, и метрики полёта надо считать
    /// только по `running`.
    script_phase: Option<&'static str>,
    /// Ближайший враг: подброс даёт его атака (анимация 19 «прыжок на игрока»),
    /// поэтому sweep'у нужно видеть окно анимации врага в кадре.
    enemy: crate::tas::types::EnemyState,
    /// Что мы подали через override на этом кадре. В паузе `input` (cur_in)
    /// залипает на бите паузы и поданных битов не показывает — навигацию
    /// в меню видно только здесь.
    fed_down_bits: u32,
    fed_pressed_bits: u32,
    fed_left_stick: [f32; 2],
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
    camera_pos: [f32; 3],
    camera_look_at: [f32; 3],
    camera_roll: f32,
    input: InputUnit,
}

/// JSON-форма кадра лога: кнопки декодированы в имена (LLM-дружелюбно).
#[derive(Serialize)]
struct LogFrameJson {
    t_ms: u64,
    frame: u32,
    script_id: Option<u32>,
    menu_status: &'static str,
    script_phase: Option<&'static str>,
    enemy: crate::tas::types::EnemyState,
    fed_down_bits: u32,
    fed_pressed_bits: u32,
    fed_left_stick: [f32; 2],
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
    camera_pos: [f32; 3],
    camera_look_at: [f32; 3],
    camera_rot: [f32; 3],
    input: InputJson,
}

/// Декодированный ввод кадра лога.
#[derive(Serialize)]
struct InputJson {
    buttons: Vec<&'static str>,
    /// Сырые битмаски InputUnit: нужны для меню-битов, которые пересекаются
    /// с игровыми (`confirm`==`jump` 0x10, `menu_up`==`ar_mode` 0x8,
    /// `menu_left`==`weapon_select` 0x1) — по именам их не различить.
    down_bits: u32,
    pressed_bits: u32,
    left_stick: [f32; 2],
    right_stick: [f32; 2],
}

/// Последний снимок состояния (обновляется в render, читается /state).
#[derive(Clone, Default)]
struct StateSnapshot {
    t_ms: u64,
    mission_id: i32,
    mission_name: String,
    menu_status: String,
    player_found: bool,
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
    /// Позиция камеры (cCameraGame + 0x1B0) — позволяет проверять эффект
    /// camera/right_stick по логам (орбита при yaw, y при pitch), без
    /// визуального контроля.
    camera_pos: [f32; 3],
    /// Точка, куда камера смотрит (+0x1C0) — вместе с позицией даёт yaw/pitch.
    camera_look_at: [f32; 3],
    /// Крен камеры (+0x1F0).
    camera_roll: f32,
    /// Живая длительность кадра из `cSlowRateManager::m_fTickDifference` (мс,
    /// номинал 16.667) — видно, переживает ли нашу запись движок.
    dt_frame_ms: f32,
    /// Коэффициент кадра (`m_fTickRate`, номинал 1.0).
    dt_rate: f32,
}

/// `GET /health` — живость, версия, base_addr, uptime.
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    base_addr: String,
    uptime_ms: u64,
}

/// `GET /state` — снимок игрока.
#[derive(Serialize)]
struct PlayerSnapshot {
    found: bool,
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
}

/// Статус скрипта (в `/state` и `/script/{id}`).
#[derive(Serialize)]
struct ScriptStatusJson {
    id: u32,
    name: String,
    status: ScriptStatus,
    frame: u32,
    total_frames: u32,
}

/// `GET /state` — снимок камеры: позиция, look-at точка, углы
/// (yaw/pitch/roll, выводятся из pos→lookAt и m_fRoll).
#[derive(Serialize)]
struct CameraSnapshot {
    pos: [f32; 3],
    look_at: [f32; 3],
    rot: [f32; 3],
}

/// `GET /state` — текущий снимок игры + статус скрипта + fps.
#[derive(Serialize)]
struct StateResponse {
    t_ms: u64,
    mission_id: i32,
    mission_name: String,
    menu_status: String,
    player: PlayerSnapshot,
    camera: CameraSnapshot,
    script: Option<ScriptStatusJson>,
    fps: f32,
    /// Счётчик тиков симуляции (детур `updateInputUnit`): по нему видно, сколько
    /// шагов игра делает за секунду при разной дельте кадра — подброс от
    /// риппера считается по числу тиков за «медленную секунду».
    sim_ticks: u64,
    /// Откуда вызывают геттер времени (только debug): по этим адресам ищется
    /// планировщик шагов симуляции — тот, кто читает время каждый кадр.
    time_callers: Vec<TimeCaller>,
    /// Шаг времени движка (см. `POST /dt`).
    dt: DtSnapshot,
}

/// Один вызывающий геттера времени: адрес возврата и число вызовов.
#[derive(Serialize)]
struct TimeCaller {
    /// Абсолютный адрес в памяти процесса (с релокацией).
    addr: String,
    /// Тот же адрес минус база модуля — по нему искать в exe на диске.
    rva: String,
    /// Сколько раз с этого адреса позвали геттер (монотонно).
    calls: u64,
}

/// Шаг времени движка: живая дельта кадра и признак фиксированного тика.
#[derive(Serialize)]
struct DtSnapshot {
    /// Включён ли фиксированный тик (`POST /dt`).
    fixed: bool,
    /// Значение, которым подменяем дельту, мс (по умолчанию номинал 16.667).
    fixed_ms: f32,
    /// Идут ли синтетические часы (шаг = ровно 16.667 мс символьного времени).
    steps: bool,
    /// Сколько раз синтетическая ветка сработала (монотонно) и последнее
    /// отданное значение — видно, используется ли она вообще.
    steps_returns: u64,
    steps_last_ms: f32,
    /// Измеренная движком длительность кадра, мс (номинал 16.667).
    frame_ms: f32,
    /// Коэффициент кадра = `frame_ms / 16.667` (номинал 1.0).
    rate: f32,
}

/// `POST /dt` — ответ.
#[derive(Serialize)]
struct DtResponse {
    fixed: bool,
    /// Чем подменяем дельту, мс.
    ms: f32,
    /// Синтетические часы (шаг = 16.667 мс).
    steps: bool,
    /// Абсолютный адрес `cSlowRateManager` (для сверки с зондом).
    addr: String,
}

/// `POST /script/run` — ответ.
#[derive(Serialize)]
struct ScriptRunResponse {
    script_id: u32,
    name: String,
    total_frames: u32,
    /// `armed` — скрипт взведён и ждёт триггера, `running` — выполняется.
    status: ScriptStatus,
}

/// `POST /script/stop` — ответ.
#[derive(Serialize)]
struct ScriptStopResponse {
    stopped: bool,
    script_id: u32,
}

/// `POST /eject` — ответ. Сам eject выполняет render-цикл (см. `handle_eject`).
#[derive(Serialize)]
struct EjectResponse {
    ejecting: bool,
}

/// `GET /logs` — ответ.
#[derive(Serialize)]
struct LogsResponse {
    from_ms: u64,
    to_ms: u64,
    count: usize,
    frames: Vec<LogFrameJson>,
}

/// Ошибка: `{ "error": "..." }`.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// Тело ответа — один из типизированных JSON-ответов (untagged: сериализуется
/// как содержимое варианта).
#[derive(Serialize)]
#[serde(untagged)]
enum Response {
    Health(HealthResponse),
    State(StateResponse),
    ScriptRun(ScriptRunResponse),
    ScriptStop(ScriptStopResponse),
    ScriptStatus(ScriptStatusJson),
    Logs(LogsResponse),
    Eject(EjectResponse),
    Dt(DtResponse),
    Error(ErrorResponse),
}

/// Кольцевой буфер кадров: при переполнении старые кадры вытесняются.
struct RingBuffer {
    frames: VecDeque<LogFrame>,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, frame: LogFrame) {
        if self.frames.len() == self.capacity {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    fn query(
        &self,
        from_ms: u64,
        to_ms: u64,
        script_id: Option<u32>,
        limit: usize,
    ) -> Vec<LogFrame> {
        self.frames
            .iter()
            .filter(|f| f.t_ms >= from_ms && f.t_ms <= to_ms)
            .filter(|f| script_id.is_none_or(|id| f.script_id == Some(id)))
            .take(limit)
            .cloned()
            .collect()
    }
}

/// `cSlowRateManager` (движковый менеджер шага времени) — singleton по
/// `base + 0x17E93B0` (ref/mgr-plugin-sdk, смещения сверены с дизассемблером
/// `startup`/`setTickDelay`). Здесь игра держит измеренную длительность кадра:
/// `m_fTickDifference` (+0x8C) в миллисекундах — живой разброс 16.25–19.25 при
/// номинале 16.667 (то есть 52–61 FPS) — и коэффициент `m_fTickRate` (+0x7C),
/// равный `diff / 16.667`, который читают анимация и кинематика. Разброс этой
/// дельты и есть дрейф, из-за которого одинаковые прогоны расходились.
const SLOW_RATE_MANAGER: usize = 0x17E93B0;
const SRM_TICK_RATE: usize = 0x7C;
const SRM_TICK_DIFF: usize = 0x8C;
const SRM_UNIT0_DELTA: usize = 0x48;
/// Номинал движка: 1/60 с в миллисекундах и коэффициент 1.0.
const NOMINAL_FRAME_MS: f32 = 16.666_668;
const NOMINAL_RATE: f32 = 1.0;

/// «Кормить движку фиксированный тик» (`POST /dt`, по умолчанию выключено):
/// в детуре `updateInputUnit` перезаписываем измеренную дельту кадра номиналом.
static FIXED_DT: AtomicBool = AtomicBool::new(false);
/// Значение фиксированной дельты, мс (f32 в битах — `AtomicF32` в std нет).
/// Номинал движка 16.667, но при 57 FPS реальная средняя ~17.5: физика шла по
/// ней, и фиксация другого значения возвращает прежние дуги уже без разброса.
static FIXED_DT_MS_BITS: AtomicU32 = AtomicU32::new(f32::to_bits(NOMINAL_FRAME_MS));
/// Абсолютный адрес `cSlowRateManager` (base + 0x17E93B0); 0 — не включено.
static SRM_ADDR: AtomicUsize = AtomicUsize::new(0);

/// Пишет выбранную дельту кадра (см. `FIXED_DT`). Только записи по готовому
/// адресу: ни аллокаций, ни логов — это путь детура.
fn apply_fixed_dt() {
    let srm = SRM_ADDR.load(Ordering::Relaxed);
    if srm == 0 {
        return;
    }
    let ms = f32::from_bits(FIXED_DT_MS_BITS.load(Ordering::Relaxed));
    // Коэффициент держим согласованным с дельтой: движок считает его как
    // diff / номинал, и читать его может как анимация, так и кинематика.
    let rate = if NOMINAL_FRAME_MS > 0.0 {
        ms / NOMINAL_FRAME_MS
    } else {
        NOMINAL_RATE
    };
    let p = srm as *mut u8;
    unsafe {
        *(p.add(SRM_TICK_DIFF) as *mut f32) = ms;
        *(p.add(SRM_TICK_RATE) as *mut f32) = rate;
        *(p.add(SRM_UNIT0_DELTA) as *mut f32) = rate;
    }
}

/// Очередь ввода активного скрипта для подачи из детура `updateInputUnit`, то
/// есть **по тикам симуляции** (как `PLAYBACK_FEED` у replay). Раньше кадр
/// скрипта считался в render-цикле: при 43–57 FPS кадров и 60 тиках физики один
/// и тот же кадр скрипта (прыжок 45, удар 76) попадал в разные моменты физики —
/// отсюда «то в воздухе, то на земле» и невоспроизводимость серий.
static SCRIPT_QUEUE: Mutex<VecDeque<InputOverride>> = Mutex::new(VecDeque::new());
/// Счётчик тиков симуляции: инкрементирует детур `updateInputUnit`, render по
/// нему досчитывает, сколько кадров скрипта подать.
static SIM_TICKS: AtomicU64 = AtomicU64::new(0);
/// Предел очереди кадров: страховка от накопления, если тики обгонят render.
const SCRIPT_QUEUE_MAX: usize = 8;
/// Предел кадров скрипта за один render-кадр: при 60 тиках и 40-60 FPS это
/// 1-2, больше бывает только при сбое (тогда лучше отстать, чем «промотать»).
const MAX_STEPS_PER_FRAME: usize = 4;

/// Подача ввода скрипта на текущий тик симуляции — вызывается из детура
/// `updateInputUnit`. `false` — очередь пуста (подавать нечего, вызывающий
/// применит обычный override и тем самым удержит последний ввод). Ни
/// логирования, ни работы с файлами здесь нет: в детуре это даёт рекурсию
/// access violation (см. `docs/REPLAY_FINDINGS.md`).
pub(crate) fn feed_tick(unit: *mut InputUnit) -> bool {
    SIM_TICKS.fetch_add(1, Ordering::Relaxed);
    // Шаг для синтетических часов: один вызов ввода = один шаг символьного
    // времени (16.667 мс), если синтетика включена.
    hooks::note_step();
    if FIXED_DT.load(Ordering::Relaxed) {
        apply_fixed_dt();
    }
    let ov = match SCRIPT_QUEUE.lock() {
        Ok(mut q) => q.pop_front(),
        Err(_) => None,
    };
    match ov {
        Some(ov) => {
            if ov.active {
                unsafe { *unit = ov.input };
            }
            true
        }
        None => false,
    }
}

/// Сколько тиков симуляции прошло (детур `updateInputUnit`).
pub(crate) fn sim_ticks() -> u64 {
    SIM_TICKS.load(Ordering::Relaxed)
}

/// Кладёт ввод очередного кадра скрипта в очередь тиков. Тот же ввод остаётся
/// в активном override: его видит debug-панель, лог кадров, и он же служит
/// «удержанием» последнего кадра, если тиков оказалось больше, чем кадров.
fn push_script_frame(ov: InputOverride) {
    replay::set_input_override(ov);
    if let Ok(mut q) = SCRIPT_QUEUE.lock() {
        while q.len() >= SCRIPT_QUEUE_MAX {
            q.pop_front();
        }
        q.push_back(ov);
    }
}

/// Сбрасывает очередь кадров (старт нового скрипта, остановка, eject).
pub(crate) fn clear_script_queue() {
    if let Ok(mut q) = SCRIPT_QUEUE.lock() {
        q.clear();
    }
}

/// Общее состояние, разделяемое между render-потоком (пишет) и HTTP (читает).
struct SharedState {
    ring: RingBuffer,
    snapshot: StateSnapshot,
    script: Option<ScriptState>,
    start: Instant,
    frame_count: u32,
    fps: f32,
    base_addr: usize,
    /// Последний виденный счётчик тиков симуляции: по нему render считает,
    /// сколько кадров скрипта подать за прошедший кадр отрисовки.
    last_sim_tick: u64,
    next_script_id: u32,
    /// Запрос eject через `POST /eject`: HTTP-поток ставит флаг и успевает
    /// ответить, render-цикл проверяет его каждый кадр и выполняет
    /// `shutdown()` + `hudhook::eject()` (из HTTP-потока это невозможно —
    /// `shutdown()` джойнит сам себя).
    eject_requested: bool,
}

/// HTTP API-сервер. Владеет потоком; `Drop`/`shutdown` останавливает поток
/// (флаг + join) и снимает override ввода — обязателен перед eject DLL.
pub struct ApiServer {
    handle: Option<std::thread::JoinHandle<()>>,
    state: Arc<Mutex<SharedState>>,
    stop: Arc<AtomicBool>,
}

impl ApiServer {
    /// Запускает HTTP-сервер на `127.0.0.1:5223`. При неудачном bind сервер
    /// деградирует в no-op (логируется) — мод продолжает работать без API.
    pub fn new(base_addr: usize) -> Self {
        let state = Arc::new(Mutex::new(SharedState {
            ring: RingBuffer::new(RING_CAPACITY),
            snapshot: StateSnapshot::default(),
            script: None,
            start: Instant::now(),
            frame_count: 0,
            fps: 0.0,
            base_addr,
            last_sim_tick: 0,
            next_script_id: 1,
            eject_requested: false,
        }));

        let stop = Arc::new(AtomicBool::new(false));
        let handle = match TcpListener::bind(BIND_ADDR) {
            Ok(listener) => {
                logger::log_line(&format!("api: http server on {}", BIND_ADDR));
                let state = Arc::clone(&state);
                let stop = Arc::clone(&stop);
                Some(
                    std::thread::Builder::new()
                        .name("drmod-api".into())
                        .spawn(move || api_thread(listener, state, stop))
                        .expect("api thread spawn"),
                )
            }
            Err(e) => {
                logger::log_line(&format!("api: http bind {} FAIL: {}", BIND_ADDR, e));
                None
            }
        };

        Self {
            handle,
            state,
            stop,
        }
    }

    /// Покадровый апдейт из render: продвижение скрипта, запись кадра в буфер,
    /// обновление снимка. Вызывается каждый кадр, независимо от UI.
    pub fn frame_update(
        &self,
        ui_state: &UiState,
        input: InputUnit,
        state: PlayerState,
        camera: crate::tas::types::CameraState,
        enemy: crate::tas::types::EnemyState,
    ) {
        // Общий сброс фронта ripper каждый render-кадр: флаг isKeybindPressed(11)
        // живёт ровно один игровой тик (ставится script_tick/playback/NumPad7,
        // виден игре на тике K+1, сбрасывается здесь на кадре K+1). Детур не
        // декрементит — игра вызывает isKeybindPressed(11) спорадически.
        hooks::set_ripper_frames(0);
        let mut guard = self.state.lock().unwrap();
        guard.frame_count += 1;
        let elapsed = guard.start.elapsed();
        let elapsed_ms = elapsed.as_millis() as u64;
        if elapsed.as_secs_f32() > 0.0 {
            guard.fps = guard.frame_count as f32 / elapsed.as_secs_f32();
        }

        // Авто-стоп при loading: игрок пропал ПОСЛЕ того, как был найден, —
        // скрипт останавливается, иначе залипший override сломает
        // пересоздающегося игрока. Если скрипт стартовал в меню (игрока и не
        // было) — не останавливаем: так работают меню-скрипты (загрузка
        // сохранения, навигация в титуле) через `dik_key`.
        if !ui_state.player_found
            && let Some(s) = guard.script.as_mut()
            && s.status == ScriptStatus::Running
            && s.player_was_found
        {
            logger::log_line(&format!(
                "api: script {} '{}' stopped (player not readable)",
                s.id, s.name
            ));
            stop_script(s);
        }
        if ui_state.player_found
            && let Some(s) = guard.script.as_mut()
        {
            s.player_was_found = true;
        }

        // Продвижение активного скрипта: вычисляем InputUnit кадра и подаём
        // через override; по завершении снимаем override и keybind-эмуляцию.
        // Взведённый скрипт (Armed) стартует, когда игрок попадает в зону
        // триггера — первый тик выполняется со следующего кадра (как при
        // запуске через HTTP).
        let base_addr = guard.base_addr;
        // Сколько кадров скрипта подать за этот render-кадр. В геймплее — по
        // числу прошедших тиков симуляции (детур `updateInputUnit`): только так
        // кадр скрипта совпадает с тиком физики и тайминги не зависят от FPS.
        // В меню/загрузке тиков нет (детур там не вызывается) — по одному кадру
        // за кадр отрисовки, как было, иначе скрипт меню/рестарта встал бы.
        let steps = if ui_state.menu_status.is_in_game() {
            let now = sim_ticks();
            let elapsed = now.saturating_sub(guard.last_sim_tick);
            guard.last_sim_tick = now;
            (elapsed as usize).min(MAX_STEPS_PER_FRAME)
        } else {
            1
        };
        let script_phase = guard
            .script
            .as_ref()
            .map(|s| script_phase_name(s.status));
        let script_id = if let Some(s) = guard.script.as_mut() {
            if s.status == ScriptStatus::Restarting {
                // Фаза рестарта: подаём меню-ввод, пока не начался loading
                // (миссия реально перезагрузилась) — только тогда переходим к
                // командам пользователя. Иначе триггер сработал бы по старой
                // позиции игрока (он мог стоять в зоне спавна) и прогон был бы
                // невалидным, а скрипт снялся бы авто-стопом на loading.
                let ov = script_tick(s, base_addr, &enemy, state.pos, state.velocity[1]);
                replay::set_input_override(ov);
                let loading = !ui_state.player_found;
                let timeout = s.frame >= s.total_frames + RESTART_LOADING_WAIT;
                if loading || timeout {
                    replay::set_input_override(InputOverride::default());
                    hooks::clear_keybind_emulation();
                    if let Some(next) = s.pending.take() {
                        s.fired_at = ScriptState::new_fired(&next.commands);
                        s.commands = next.commands;
                        s.total_frames = next.total_frames;
                        s.frame = 0;
                        s.status = if loading {
                            next.status
                        } else {
                            // loading не наступил — рестарт не сработал
                            // (курсор не на пункте Restart, нет фокуса окна)
                            logger::log_line(&format!(
                                "api: script {} '{}' рестарт не сработал: loading не наступил",
                                s.id, s.name
                            ));
                            ScriptStatus::Stopped
                        };
                        logger::log_line(&format!(
                            "api: script {} '{}' restart done -> {:?} (loading={})",
                            s.id, s.name, next.status, loading
                        ));
                    } else {
                        s.status = ScriptStatus::Done;
                    }
                }
                Some(s.id)
            } else if s.status == ScriptStatus::Running {
                // Кадры скрипта кладём в очередь тиков: детур `updateInputUnit`
                // заберёт по одному за тик симуляции (`api::feed_tick`), поэтому
                // кадр скрипта = тик физики.
                for _ in 0..steps {
                    let ov = script_tick(s, base_addr, &enemy, state.pos, state.velocity[1]);
                    push_script_frame(ov);
                    if s.status != ScriptStatus::Running {
                        break;
                    }
                }
                if s.status == ScriptStatus::Done {
                    // Последние кадры уже не нужны: снимаем override, чистим
                    // очередь и keybind-эмуляцию.
                    replay::set_input_override(InputOverride::default());
                    clear_script_queue();
                    hooks::clear_keybind_emulation();
                    logger::log_line(&format!("api: script {} '{}' done", s.id, s.name));
                }
                Some(s.id)
            } else if s.status == ScriptStatus::Armed
                && let Some(target) = s.trigger
                && segment::in_zone(ui_state.position, target)
            {
                s.status = ScriptStatus::Running;
                s.frame = 0;
                hooks::clear_keybind_emulation();
                logger::log_line(&format!(
                    "api: script {} '{}' trigger -> started",
                    s.id, s.name
                ));
                None
            } else {
                None
            }
        } else {
            None
        };

        // Дельта кадра из cSlowRateManager: если включён фиксированный тик, тут
        // видно, осталась ли наша запись (номинал 16.667 / 1.0) или движок
        // перезаписал её своей измеренной после нас.
        let srm = base_addr + SLOW_RATE_MANAGER;
        let (dt_frame_ms, dt_rate) = unsafe {
            (
                *((srm + SRM_TICK_DIFF) as *const f32),
                *((srm + SRM_TICK_RATE) as *const f32),
            )
        };

        guard.snapshot = StateSnapshot {
            t_ms: elapsed_ms,
            mission_id: ui_state.mission_id,
            mission_name: ui_state.mission_name.clone(),
            menu_status: ui_state.menu_status.name().to_string(),
            player_found: ui_state.player_found,
            pos: state.pos,
            rot: state.rotation,
            vel: state.velocity,
            hp: state.hp,
            r_anim: state.r_anim,
            ripper: state.ripper_enabled,
            blade: state.blade_mode_type,
            camera_pos: camera.pos,
            camera_look_at: camera.look_at,
            camera_roll: camera.roll,
            dt_frame_ms,
            dt_rate,
        };

        if ui_state.player_found {
            let frame_count = guard.frame_count;
            let fed = replay::input_override();
            let (fed_down, fed_pressed, fed_left) = (
                fed.input.buttons_down,
                fed.input.buttons_pressed,
                fed.input.left_stick,
            );
            drop(fed);
            guard.ring.push(LogFrame {
                t_ms: elapsed_ms,
                frame: frame_count,
                script_id,
                script_phase,
                enemy,
                menu_status: ui_state.menu_status.name(),
                fed_down_bits: fed_down,
                fed_pressed_bits: fed_pressed,
                fed_left_stick: fed_left,
                pos: state.pos,
                rot: state.rotation,
                vel: state.velocity,
                hp: state.hp,
                r_anim: state.r_anim,
                ripper: state.ripper_enabled,
                blade: state.blade_mode_type,
                camera_pos: camera.pos,
                camera_look_at: camera.look_at,
                camera_roll: camera.roll,
                input,
            });
        }
    }

    /// Активен ли скрипт (выполняется или взведён с триггером). Используется,
    /// чтобы debug-инжекция/record/playback не вмешивались в ввод, которым
    /// управляет API-скрипт: взведённый скрипт вот-вот стартует, и параллельный
    /// playback конфликтовал бы за override. В release вызывается только извне
    /// (HTTP) — отсюда allow.
    #[allow(dead_code)]
    pub fn is_script_active(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .script
            .as_ref()
            .is_some_and(|s| s.status.is_active())
    }

    /// Выполняется ли скрипт прямо сейчас (фаза `running`, а не `restarting`/
    /// `armed`): по этому гейту читаются тяжёлые/опасные данные (враг).
    pub fn is_script_running(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .script
            .as_ref()
            .is_some_and(|s| s.status == ScriptStatus::Running)
    }

    /// Запрошен ли eject через `POST /eject`. Проверяется в render-цикле
    /// каждый кадр; при `true` render выполняет `shutdown()` + `eject()`.
    pub fn eject_requested(&self) -> bool {
        self.state.lock().unwrap().eject_requested
    }

    /// Запрос eject из UI (кнопка «Выход / Выгрузить DLL»): ставит тот же
    /// флаг, что и `POST /eject` — render-цикл выполняет выгрузку в конце
    /// кадра. Единый путь для кнопки и HTTP.
    pub fn request_eject(&self) {
        self.state.lock().unwrap().eject_requested = true;
    }

    /// Запускает встроенный скрипт NumPad4 (run-jump-attack) — тот же механизм,
    /// что и `POST /script/run`. Игнорируется, если другой скрипт уже активен.
    /// В release вызывается только извне (HTTP) — отсюда allow.
    #[allow(dead_code)]
    pub fn start_builtin_script(&self) {
        let req = match parse_script(BUILTIN_SCRIPT) {
            Ok(v) => v,
            Err(e) => {
                logger::log_line(&format!("api: builtin script parse FAIL: {}", e));
                return;
            }
        };
        let mut guard = self.state.lock().unwrap();
        if guard
            .script
            .as_ref()
            .is_some_and(|s| s.status.is_active())
        {
            logger::log_line("api: builtin script ignored (another script active)");
            return;
        }
        let id = guard.next_script_id;
        guard.next_script_id += 1;
        let total_frames = req
            .commands
            .iter()
            .map(|c| c.t + c.duration)
            .max()
            .unwrap_or(0);
        hooks::clear_keybind_emulation();
        clear_script_queue();
        guard.script = Some(ScriptState {
            id,
            name: req.name.clone(),
            fired_at: ScriptState::new_fired(&req.commands),
            commands: req.commands,
            frame: 0,
            status: ScriptStatus::Running,
            total_frames,
            trigger: None,
            pending: None,
            player_was_found: false,
        });
        logger::log_line(&format!(
            "api: builtin script {} '{}' started ({} frames)",
            id, req.name, total_frames
        ));
    }

    /// Останавливает HTTP-поток и снимает override ввода. Вызывается перед
    /// `hudhook::eject()` и в `Drop` — без этого поток останется висеть
    /// на выгруженном коде DLL, а игра — с залипшим вводом.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        replay::set_input_override(InputOverride::default());
        hooks::clear_keybind_emulation();
        if let Ok(mut guard) = self.state.lock()
            && let Some(s) = guard.script.as_mut()
            && s.status.is_active()
        {
            s.status = ScriptStatus::Stopped;
        }
        logger::log_line("api: http server stopped");
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Цикл HTTP-потока: неблокирующий accept + опрос stop-флага. Поток выходит
/// максимум через ACCEPT_POLL_MS после shutdown (или после текущего
/// соединения, ограниченного IO_TIMEOUT) — join в shutdown() не виснет.
fn api_thread(listener: TcpListener, state: Arc<Mutex<SharedState>>, stop: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                // Таймауты на сокете: клиент, который не читает ответ или не
                // досылает тело, не может заблокировать поток навсегда.
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                handle_connection(stream, &state);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(ACCEPT_POLL_MS));
            }
            Err(_) => break,
        }
    }
}

/// Обслуживает одно соединение: читает заголовки и тело, маршрутизирует,
/// пишет ответ и закрывает соединение (Connection: close — без keep-alive,
/// висящих соединений не остаётся).
fn handle_connection(mut stream: TcpStream, state: &Arc<Mutex<SharedState>>) {
    // Читаем заголовки до \r\n\r\n (лимит MAX_HEADER_BYTES).
    let mut buf = [0u8; 2048];
    let mut head = Vec::new();
    let header_end = loop {
        if let Some(p) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if head.len() > MAX_HEADER_BYTES {
            respond(
                &mut stream,
                400,
                &Response::Error(ErrorResponse {
                    error: "headers too large".into(),
                }),
            );
            return;
        }
        match stream.read(&mut buf) {
            Ok(0) => return, // клиент закрыл соединение
            Ok(n) => head.extend_from_slice(&buf[..n]),
            Err(_) => return, // таймаут/ошибка — бросаем соединение
        }
    };

    // Первая строка: METHOD SP TARGET SP HTTP/x.y.
    let head_str = String::from_utf8_lossy(&head[..header_end]);
    let mut lines = head_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();

    // Content-Length и Transfer-Encoding из заголовков.
    let mut content_length = 0usize;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                chunked = !value.eq_ignore_ascii_case("identity");
            }
        }
    }
    if chunked {
        respond(
            &mut stream,
            400,
            &Response::Error(ErrorResponse {
                error: "chunked transfer not supported".into(),
            }),
        );
        return;
    }
    if content_length > MAX_BODY_BYTES as usize {
        respond(
            &mut stream,
            413,
            &Response::Error(ErrorResponse {
                error: "body too large".into(),
            }),
        );
        return;
    }

    // Тело: остаток после заголовков (мог прийти в том же пакете) +
    // дозапись до Content-Length.
    let mut body_bytes = head[header_end..].to_vec();
    let mut remaining = content_length.saturating_sub(body_bytes.len());
    while remaining > 0 {
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                let take = n.min(remaining);
                body_bytes.extend_from_slice(&buf[..take]);
                remaining -= take;
            }
            Err(_) => return,
        }
    }
    let body = String::from_utf8_lossy(&body_bytes).into_owned();

    let (code, value) = route(&method, &target, &body, state);
    respond(&mut stream, code, &value);
}

/// Маршрутизация по (метод, путь). Возвращает (код, JSON-тело ответа).
fn route(
    method: &str,
    target: &str,
    body: &str,
    state: &Arc<Mutex<SharedState>>,
) -> (u16, Response) {
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };
    match (method, path) {
        ("GET", "/health") => (200, Response::Health(health_json(state))),
        ("GET", "/state") => (200, Response::State(state_json(state))),
        ("POST", "/script/run") => handle_script_run(body, state),
        ("POST", "/script/stop") => handle_script_stop(state),
        ("POST", "/eject") => handle_eject(state),
        ("POST", "/dt") => handle_dt(body, state),
        ("GET", path) if path.starts_with("/script/") => handle_script_get(path, state),
        ("GET", "/logs") => handle_logs(query, state),
        _ => (
            404,
            Response::Error(ErrorResponse {
                error: "not found".into(),
            }),
        ),
    }
}

/// `GET /health` — живость, версия, base_addr, uptime.
fn health_json(state: &Arc<Mutex<SharedState>>) -> HealthResponse {
    let guard = state.lock().unwrap();
    HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        base_addr: format!("0x{:08X}", guard.base_addr),
        uptime_ms: guard.start.elapsed().as_millis() as u64,
    }
}

/// `GET /state` — текущий снимок игры + статус скрипта + fps.
fn state_json(state: &Arc<Mutex<SharedState>>) -> StateResponse {
    let guard = state.lock().unwrap();
    let s = &guard.snapshot;
    let script = guard.script.as_ref().map(|sc| ScriptStatusJson {
        id: sc.id,
        name: sc.name.clone(),
        status: sc.status,
        frame: sc.frame,
        total_frames: sc.total_frames,
    });
    StateResponse {
        t_ms: s.t_ms,
        mission_id: s.mission_id,
        mission_name: s.mission_name.clone(),
        menu_status: s.menu_status.clone(),
        player: PlayerSnapshot {
            found: s.player_found,
            pos: s.pos,
            rot: s.rot,
            vel: s.vel,
            hp: s.hp,
            r_anim: s.r_anim,
            ripper: s.ripper,
            blade: s.blade,
        },
        camera: CameraSnapshot {
            pos: s.camera_pos,
            look_at: s.camera_look_at,
            rot: camera_angles(s.camera_pos, s.camera_look_at, s.camera_roll),
        },
        script,
        fps: guard.fps,
        sim_ticks: sim_ticks(),
        time_callers: hooks::time_callers()
            .into_iter()
            .map(|(addr, calls)| TimeCaller {
                addr: format!("0x{addr:08X}"),
                rva: format!("0x{:08X}", addr.saturating_sub(guard.base_addr)),
                calls,
            })
            .collect(),
        dt: DtSnapshot {
            fixed: FIXED_DT.load(Ordering::Relaxed),
            fixed_ms: f32::from_bits(FIXED_DT_MS_BITS.load(Ordering::Relaxed)),
            steps: hooks::synthetic_clock_on(),
            steps_returns: hooks::synthetic_clock_stats().0,
            steps_last_ms: hooks::synthetic_clock_stats().1 as f32 / 1000.0,
            frame_ms: s.dt_frame_ms,
            rate: s.dt_rate,
        },
    }
}

/// `POST /dt` — включить/выключить фиксированный шаг времени движка.
/// Тело: `{"fixed": true}` или `{"fixed": true, "ms": 17.5}`. Адрес менеджера
/// считаем от `base_addr` состояния; при выключении движок сам вернётся к
/// измеренной дельте (мы её не трогаем).
fn handle_dt(body: &str, state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    #[derive(Deserialize)]
    struct Req {
        fixed: bool,
        /// Дельта кадра в мс: номинал движка 16.667, при 57 FPS реальная
        /// средняя ~17.5 (по ней физика и шла до фиксации).
        ms: Option<f32>,
        /// Синтетические часы: каждый шаг (вызов ввода) двигает символьное
        /// время ровно на 16.667 мс — абсолютно стабильный шаг, при нехватке
        /// FPS игра замедляется. Пацер кадров при этом видит реальное время.
        steps: Option<bool>,
    }
    let req: Req = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                400,
                Response::Error(ErrorResponse {
                    error: format!(
                        "bad body: {e} (ожидается {{\"fixed\": true, \"ms\": 17.5}})"
                    ),
                }),
            )
        }
    };
    if let Some(on) = req.steps {
        let was = hooks::set_synthetic_clock(on);
        logger::log_line(&format!(
            "api: синтетические часы {} (было {})",
            if on { "вкл" } else { "выкл" },
            if was { "вкл" } else { "выкл" }
        ));
    }
    if let Some(ms) = req.ms {
        if !(1.0..=1000.0).contains(&ms) {
            return (
                400,
                Response::Error(ErrorResponse {
                    error: format!("ms={ms} вне разумного диапазона 1..1000"),
                }),
            );
        }
        FIXED_DT_MS_BITS.store(ms.to_bits(), Ordering::Relaxed);
    }
    let addr = {
        let guard = state.lock().unwrap();
        guard.base_addr + SLOW_RATE_MANAGER
    };
    if req.fixed {
        SRM_ADDR.store(addr, Ordering::Relaxed);
        FIXED_DT.store(true, Ordering::Relaxed);
    } else {
        FIXED_DT.store(false, Ordering::Relaxed);
        SRM_ADDR.store(0, Ordering::Relaxed);
    }
    let ms = f32::from_bits(FIXED_DT_MS_BITS.load(Ordering::Relaxed));
    logger::log_line(&format!(
        "api: fixed dt {} ({ms} мс, cSlowRateManager=0x{addr:08X})",
        if req.fixed { "on" } else { "off" }
    ));
    (
        200,
        Response::Dt(DtResponse {
            fixed: req.fixed,
            ms,
            steps: hooks::synthetic_clock_on(),
            addr: format!("0x{addr:08X}"),
        }),
    )
}

/// `POST /script/run` — запуск скрипта из JSON-тела.
fn handle_script_run(body: &str, state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let req = match parse_script(body) {
        Ok(v) => v,
        Err(e) => return (400, Response::Error(ErrorResponse { error: e })),
    };
    let mut guard = state.lock().unwrap();
    if guard
        .script
        .as_ref()
        .is_some_and(|s| s.status.is_active())
    {
        let s = guard.script.as_ref().unwrap();
        return (
            409,
            Response::Error(ErrorResponse {
                error: format!("script already active: id={} name={}", s.id, s.name),
            }),
        );
    }
    let id = guard.next_script_id;
    guard.next_script_id += 1;
    // С рестартом: первая фаза — меню паузы (pause → стрелки → confirm), после
    // неё скрипт переходит к командам пользователя (Armed или сразу Running).
    let (phase_commands, phase_status, pending) = match req.restart {
        Some(spec) => {
            let total = req
                .commands
                .iter()
                .map(|c| c.t + c.duration)
                .max()
                .unwrap_or(0);
            let pending = PendingScript {
                commands: req.commands,
                total_frames: total,
                status: if req.trigger.is_some() {
                    ScriptStatus::Armed
                } else {
                    ScriptStatus::Running
                },
            };
            (spec.commands(), ScriptStatus::Restarting, Some(pending))
        }
        None => (
            req.commands,
            if req.trigger.is_some() {
                ScriptStatus::Armed
            } else {
                ScriptStatus::Running
            },
            None,
        ),
    };
    let total_frames = phase_commands
        .iter()
        .map(|c| c.t + c.duration)
        .max()
        .unwrap_or(0);
    // Сброс остатков keybind-эмуляции (ripper/blade) до старта.
    hooks::clear_keybind_emulation();
    let trigger = req.trigger.map(|t| segment::Vec3 {
        x: t.pos[0],
        y: t.pos[1],
        z: t.pos[2],
    });
    let status = phase_status;
    let next_status = pending.as_ref().map(|p| p.status);
    clear_script_queue();
    guard.script = Some(ScriptState {
        id,
        name: req.name.clone(),
        fired_at: ScriptState::new_fired(&phase_commands),
        commands: phase_commands,
        frame: 0,
        status,
        total_frames,
        trigger,
        pending,
        player_was_found: false,
    });
    if phase_status == ScriptStatus::Restarting {
        logger::log_line(&format!(
            "api: script {} '{}' restart phase ({} frames), затем {:?}",
            id, req.name, total_frames, next_status
        ));
    } else if let Some(t) = trigger {
        logger::log_line(&format!(
            "api: script {} '{}' armed, waiting for trigger ({:.1},{:.1},{:.1})",
            id, req.name, t.x, t.y, t.z
        ));
    } else {
        logger::log_line(&format!(
            "api: script {} '{}' started ({} frames)",
            id, req.name, total_frames
        ));
    }
    (
        200,
        Response::ScriptRun(ScriptRunResponse {
            script_id: id,
            name: req.name,
            total_frames,
            status,
        }),
    )
}

/// `POST /script/stop` — остановка активного скрипта.
fn handle_script_stop(state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let mut guard = state.lock().unwrap();
    match guard.script.as_mut() {
        Some(s) if s.status.is_active() => {
            let id = s.id;
            stop_script(s);
            logger::log_line(&format!("api: script {} stopped by request", id));
            (
                200,
                Response::ScriptStop(ScriptStopResponse {
                    stopped: true,
                    script_id: id,
                }),
            )
        }
        _ => (
            404,
            Response::Error(ErrorResponse {
                error: "no active script".into(),
            }),
        ),
    }
}

/// `POST /eject` — запрос выгрузки DLL. Только ставит флаг: сам eject
/// (`shutdown()` + `hudhook::eject()`) выполняет render-цикл, когда увидит
/// флаг в следующем кадре. Из HTTP-потока это невозможно — `shutdown()`
/// джойнит сам себя, а `hudhook::eject()` обрабатывается в render-цикле.
/// Идемпотентно: повторный запрос до обработки флага тоже возвращает 200.
fn handle_eject(state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let mut guard = state.lock().unwrap();
    guard.eject_requested = true;
    logger::log_line("api: eject requested via POST /eject");
    (200, Response::Eject(EjectResponse { ejecting: true }))
}

/// `GET /script/{id}` — статус скрипта.
fn handle_script_get(path: &str, state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let id_str = &path["/script/".len()..];
    let id: u32 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => {
            return (
                400,
                Response::Error(ErrorResponse {
                    error: "invalid script id".into(),
                }),
            );
        }
    };
    let guard = state.lock().unwrap();
    match guard.script.as_ref().filter(|s| s.id == id) {
        Some(s) => (
            200,
            Response::ScriptStatus(ScriptStatusJson {
                id: s.id,
                name: s.name.clone(),
                status: s.status,
                frame: s.frame,
                total_frames: s.total_frames,
            }),
        ),
        None => (
            404,
            Response::Error(ErrorResponse {
                error: format!("script {} not found", id),
            }),
        ),
    }
}

/// `GET /logs` — кадры из кольцевого буфера по интервалу/скрипту.
fn handle_logs(query: &str, state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let params = parse_query(query);
    let guard = state.lock().unwrap();
    let now = guard.start.elapsed().as_millis() as u64;
    let from_ms = params
        .get("from_ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let to_ms = params
        .get("to_ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(now);
    let script_id = params.get("script_id").and_then(|v| v.parse().ok());
    let limit = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
        .min(MAX_LOG_LIMIT);
    let frames: Vec<LogFrameJson> = guard
        .ring
        .query(from_ms, to_ms, script_id, limit)
        .iter()
        .map(|f| f.to_json())
        .collect();
    (
        200,
        Response::Logs(LogsResponse {
            from_ms,
            to_ms,
            count: frames.len(),
            frames,
        }),
    )
}

/// Пишет HTTP-ответ с JSON-телом и Connection: close (без keep-alive —
/// соединение живёт ровно один запрос, висящих соединений нет).
fn respond(stream: &mut TcpStream, code: u16, value: &Response) {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        code,
        reason,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

/// Разбор query-строки в map (без URL-декодирования — параметры числовые).
fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

/// Парсинг скрипта из JSON-тела + пост-валидация лимитов. Serde покрывает
/// типы и неизвестные ключи (с path к полю), здесь — кросс-полевые проверки.
fn parse_script(body: &str) -> Result<ScriptRequest, String> {
    let req: ScriptRequest =
        serde_json::from_str(body).map_err(|e| format!("invalid script: {}", e))?;
    if req.name.len() > 64 {
        return Err("name too long (max 64)".into());
    }
    if req.commands.is_empty() {
        return Err("commands is empty".into());
    }
    for (i, cmd) in req.commands.iter().enumerate() {
        if cmd.duration == 0 {
            return Err(format!("commands[{}]: duration must be >= 1", i));
        }
        if cmd.t > MAX_SCRIPT_FRAMES || cmd.duration > MAX_SCRIPT_FRAMES {
            return Err(format!(
                "commands[{}]: t/duration exceeds max {}",
                i, MAX_SCRIPT_FRAMES
            ));
        }
        if cmd.t + cmd.duration > MAX_SCRIPT_FRAMES {
            return Err(format!(
                "commands[{}]: t+duration exceeds max {}",
                i, MAX_SCRIPT_FRAMES
            ));
        }
        if cmd.input.is_empty() {
            return Err(format!("commands[{}]: input is empty", i));
        }
    }
    Ok(req)
}

/// Вычисляет InputUnit кадра `script.frame` из активных команд и продвигает
/// скрипт. Keybind-действия подаются через эмуляцию `isKeybindPressed`
/// (фронт, 1 кадр) / `isKeybindDown` (удержание на время команды); меню-клавиши
/// (стрелки/Enter) — записью в кэш `ms_KeyInput` (меню читает их через
/// `isKeyDown`/`isKeyPressed`, а не через keybind'ы).
fn script_tick(
    script: &mut ScriptState,
    base_addr: usize,
    enemy: &crate::tas::types::EnemyState,
    player_pos: [f32; 3],
    player_vel_y: f32,
) -> InputOverride {
    let k = script.frame;
    // Условные команды (`when_enemy`): спят, пока не выполнено условие по врагу.
    // При срабатывании подменяем `t` на текущий кадр — дальше работает обычная
    // логика (активное окно, фронт `pressed` на первом кадре).
    for idx in 0..script.commands.len() {
        if script.commands[idx].when_enemy.is_some() {
            let cond_ok = script.commands[idx]
                .when_enemy
                .as_ref()
                .is_some_and(|c| enemy_condition_ok(c, enemy, player_pos, player_vel_y));
            // Срабатывает один раз; с `repeat` — заново каждые `duration` кадров,
            // пока условие держится (несколько ударов в окне прыжка врага).
            let can_fire = match script.fired_at[idx] {
                None => true,
                Some(at) => {
                    script.commands[idx]
                        .when_enemy
                        .as_ref()
                        .is_some_and(|c| c.repeat)
                        && k >= at + script.commands[idx].duration
                }
            };
            if can_fire && cond_ok {
                script.commands[idx].t = k;
                script.fired_at[idx] = Some(k);
                logger::log_line(&format!(
                    "api: script '{}' команда {} сработала по врагу: anim={} frame={} \
                     дистанция={:.2} клинок_выше_игрока={:.2} игрок_y={:.2} (кадр {})",
                    script.name,
                    idx,
                    enemy.r_anim,
                    enemy.frame,
                    ((enemy.pos[0] - player_pos[0]).powi(2)
                        + (enemy.pos[1] - player_pos[1]).powi(2)
                        + (enemy.pos[2] - player_pos[2]).powi(2))
                    .sqrt(),
                    enemy.blade_y - player_pos[1],
                    player_pos[1],
                    k
                ));
            }
        }
    }
    let mut unit = InputUnit {
        valid_input: 1,
        ..Default::default()
    };
    let mut active = false;
    let mut ripper_edge = false;
    // Сырые клавиши меню: в паузе игра не гоняет тик ввода (InputUnit-override
    // до меню не доходит), клавиши меню читаются через isKeyDown/isKeyPressed
    // — эмулируем их коды (`raw_key`), детуры в hooks.rs вернут 1.
    let mut raw_down = [0u32; 6];
    let mut raw_pressed = [0u32; 6];
    // DIK-клавиша DirectInput подана на этом кадре (`dik_key`).
    let mut dik_active = false;
    let mut dik_mask = [0u32; 8];
    let mut blade_on = false;
    let mut ninja_on = false;
    let mut walk_on = false;
    let mut dodge_on = false;
    let mut lock_on = false;
    let mut subweapon = false;
    let mut item = false;
    let mut camera_reset = false;
    let mut zandatsu = false;
    for cmd in &script.commands {
        if k < cmd.t || k >= cmd.t + cmd.duration {
            continue;
        }
        let inp = &cmd.input;
        // Движение: биты направлений + left_stick (если не задан явно).
        let mut stick = [0.0f32, 0.0];
        if inp.forward {
            unit.buttons_down |= addresses::input_bits::FORWARD;
            stick[1] -= 1000.0;
            active = true;
        }
        if inp.backward {
            unit.buttons_down |= addresses::input_bits::BACK;
            stick[1] += 1000.0;
            active = true;
        }
        if inp.left {
            unit.buttons_down |= addresses::input_bits::LEFT;
            stick[0] -= 1000.0;
            active = true;
        }
        if inp.right {
            unit.buttons_down |= addresses::input_bits::RIGHT;
            stick[0] += 1000.0;
            active = true;
        }
        if stick != [0.0, 0.0] && inp.left_stick.is_none() {
            unit.left_stick = stick;
        }
        if inp.jump {
            unit.buttons_down |= addresses::input_bits::JUMP;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::JUMP;
            }
            active = true;
        }
        if inp.ar_mode {
            // AR-режим: бит 0x08 в InputUnit (как прыжок) — raw-подача через
            // кэш ms_KeyInput не работает (§10.3). Бит + фронт pressed.
            unit.buttons_down |= addresses::input_bits::AR_MODE;
            // if k == cmd.t {
            //     unit.buttons_pressed |= addresses::input_bits::AR_MODE;
            // }
            active = true;
        }
        if inp.weapon_select {
            // Меню выбора оружия: настоящий ввод — бит 0x01 (DPAD_LEFT).
            // Игра сама откроет меню, когда увидит бит. Зануляем бит, как
            // только меню открылось (GameMenuStatus == SelectWeaponMenu=9):
            // в меню бит 0x01 — навигация влево, листала бы слоты.
            // Так ввод воспроизводится как в записи (без прямой записи памяти).
            let in_weapon_menu = base_addr != 0
                && unsafe { (base_addr as *const i32).add(0x17E9F9C / 4).read_unaligned() == 9 };
            if !in_weapon_menu {
                unit.buttons_down |= addresses::input_bits::WEAPON_SELECT;
                unit.buttons_pressed |= addresses::input_bits::WEAPON_SELECT;
            }
            active = true;
        }
        if inp.light_attack {
            unit.buttons_down |= addresses::input_bits::LIGHT_ATTACK;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::LIGHT_ATTACK;
            }
            active = true;
        }
        if inp.heavy_attack {
            unit.buttons_down |= addresses::input_bits::HEAVY_ATTACK;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::HEAVY_ATTACK;
            }
            active = true;
        }
        if let Some([dx, dy]) = inp.camera {
            unit.right_stick = [dx, dy];
            active = true;
        }
        // Hold-действия (isKeybindDown): удержание на все кадры команды.
        if inp.ninja_run {
            // Ниндзя-бег: для unit 0 игра читает бит 0x4000 в InputUnit
            // (реальный ввод LCtrl+W: cur_in down=00404000), а не
            // isKeybindDown(9) — call site 0x61DBE0 ставит биты 0x1000/0x8000
            // только для unit 1..3. Вход в состояние — по ФРОНТУ pressed=0x4000
            // (ручной бег: pressed=00004000 на 1-м кадре, r_anim=79); без
            // фронта игра не активирует ниндзя-бег (r_anim=71).
            unit.buttons_down |= addresses::input_bits::NINJA_RUN;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::NINJA_RUN;
            }
            ninja_on = true;
            active = true;
        }
        if inp.walk {
            walk_on = true;
            active = true;
        }
        if inp.dodge {
            dodge_on = true;
            active = true;
        }
        if inp.blade {
            // Блейд-режим: игра читает бит 0x800 в InputUnit (как ninja 0x4000) —
            // реальный ввод при удержании клавиши блейда даёт down=0x800 + фронт
            // pressed на 1-м кадре (запись 33 в БД). Keybind-эмуляция
            // (set_blade_hold) НЕ работает: игра вызывает isKeybindDown(8) только
            // в key-event обработке (0x61DA85), скрипт key events не создаёт.
            // Механизм — как в playback (replay.rs): подача InputUnit с битом 0x800.
            unit.buttons_down |= addresses::input_bits::BLADE;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::BLADE;
            }
            blade_on = true;
            active = true;
        }
        // Toggle-действия: удержание keybind'а на все кадры команды
        // (isKeybindDown; игра сама детектирует фронт). Клавиши 1/2/3 и Esc
        // игра читает как сырые клавиши — они обрабатываются ниже.
        if inp.ripper && k == cmd.t {
            ripper_edge = true;
        }
        if inp.lock_on {
            lock_on = true;
            active = true;
        }
        if inp.subweapon {
            subweapon = true;
            active = true;
        }
        if inp.item {
            item = true;
            active = true;
        }
        if inp.camera_reset {
            camera_reset = true;
            active = true;
        }
        if inp.zandatsu {
            zandatsu = true;
            active = true;
        }
        // Пауза (Esc) — бит 0x100 в InputUnit + фронт pressed: реальный Esc
        // кодируется именно им (debug.log 2026-09-10, P118_BEACH: кадр смены
        // статуса 1→3 PauseMenu имеет `cur_in down=00000100 pressed=00000100`).
        // Esc — это START геймпада, а не BUTTON_B (0x20).
        if inp.pause {
            unit.buttons_down |= addresses::input_bits::PAUSE;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::PAUSE;
            }
            active = true;
        }
        // Навигация в меню — D-Pad биты геймпада (0x1/0x2/0x4/0x8) + подтверждение
        // BUTTON_A (0x10). Проверено live (2026-08-19): подача битов в открытом
        // меню двигает выбор. Меню открывается отдельно — weapon_select через
        // прямую запись GameMenuStatus (см. выше).
        let menu_bits = [
            (inp.menu_up, addresses::input_bits::MENU_UP),
            (inp.menu_down, addresses::input_bits::MENU_DOWN),
            (inp.menu_left, addresses::input_bits::MENU_LEFT),
            (inp.menu_right, addresses::input_bits::MENU_RIGHT),
            (inp.confirm, addresses::input_bits::CONFIRM),
        ];
        for (on, bit) in menu_bits {
            if on {
                unit.buttons_down |= bit;
                if k == cmd.t {
                    unit.buttons_pressed |= bit;
                }
                active = true;
            }
        }
        if let Some(code) = inp.raw_key {
            set_key_bit(&mut raw_down, code);
            if k == cmd.t {
                set_key_bit(&mut raw_pressed, code);
            }
            active = true;
        }
        if let Some(dik) = inp.dik_key {
            set_dik_bit(&mut dik_mask, dik);
            dik_active = true;
            active = true;
        }
        if let Some(ls) = inp.left_stick {
            unit.left_stick = ls;
            active = true;
        }
    }
    // Ходьба: игра кодирует её магнитудой стика — Tab масштабирует стик ×0.5
    // (ручная ходьба: L=(0,-500), r_anim=2). Keybind 4 (walk) игра НЕ читает
    // через isKeybindDown (дизассемблирование 2026-08-18: call sites только
    // 1/2/3/9/20 + цикл 5..22) — эмуляция keybind'а не работает, персонаж
    // бежал (r_anim=3). Масштабируем итоговый стик (направления или явный).
    if walk_on {
        unit.left_stick = [unit.left_stick[0] * 0.5, unit.left_stick[1] * 0.5];
    }
    if ripper_edge {
        hooks::set_ripper_frames(1);
    }
    hooks::set_blade_hold(blade_on);
    hooks::set_keybind_hold(addresses::KEYBIND_NINJARUN, ninja_on);
    hooks::set_keybind_hold(addresses::KEYBIND_DEFFENSIVE_OFFENSIVE, dodge_on);
    hooks::set_keybind_hold(addresses::KEYBIND_SWITCH_LOCK_ON, lock_on);
    hooks::set_keybind_hold(addresses::KEYBIND_USE_SUBWEAPON, subweapon);
    hooks::set_keybind_hold(addresses::KEYBIND_USE_ITEM, item);
    hooks::set_keybind_hold(addresses::KEYBIND_CAMERA_RESET, camera_reset);
    hooks::set_keybind_hold(addresses::KEYBIND_EXECUTION, zandatsu);
    // Сырые клавиши меню живут один кадр: ставим биты текущего кадра, иначе
    // снимаем (меню не должно видеть «залипшую» стрелку).
    if raw_down.iter().any(|b| *b != 0) || raw_pressed.iter().any(|b| *b != 0) {
        hooks::set_raw_keys(raw_down, raw_pressed);
        // В меню детур `updateInputUnit` не выполняется (игра не гоняет тик
        // ввода), поэтому кэш `ms_KeyInput` обновляем сами — меню читает
        // именно его (реальные нажатия видны как `menu: press isKeyDown(...)`).
        hooks::apply_raw_keys();
    } else {
        hooks::clear_raw_keys();
    }
    if dik_active {
        hooks::set_dik_mask(dik_mask);
    } else {
        hooks::clear_dik_mask();
    }
    script.frame += 1;
    if script.frame >= script.total_frames && script.pending.is_none() {
        script.status = ScriptStatus::Done;
    }
    InputOverride {
        active,
        input: unit,
    }
}

/// Останавливает скрипт: снимает override, чистит очередь кадров и
/// keybind-эмуляцию.
fn stop_script(script: &mut ScriptState) {
    script.status = ScriptStatus::Stopped;
    replay::set_input_override(InputOverride::default());
    clear_script_queue();
    hooks::clear_keybind_emulation();
}

/// Углы камеры из pos→lookAt: yaw — поворот вокруг Y (atan2(dx, dz)),
/// pitch — наклон (atan2(dy, горизонталь)), roll — крен напрямую из m_fRoll.
/// Позволяет проверять camera/right_stick по логам: yaw меняется при повороте,
/// pitch — при наклоне (в отличие от позиции, где наклон даёт только малый y).
fn camera_angles(pos: [f32; 3], look_at: [f32; 3], roll: f32) -> [f32; 3] {
    let dx = look_at[0] - pos[0];
    let dy = look_at[1] - pos[1];
    let dz = look_at[2] - pos[2];
    let horiz = (dx * dx + dz * dz).sqrt();
    [dx.atan2(dz), dy.atan2(horiz), roll]
}

impl LogFrame {
    fn to_json(&self) -> LogFrameJson {
        LogFrameJson {
            t_ms: self.t_ms,
            frame: self.frame,
            script_id: self.script_id,
            menu_status: self.menu_status,
            script_phase: self.script_phase,
            enemy: self.enemy,
            fed_down_bits: self.fed_down_bits,
            fed_pressed_bits: self.fed_pressed_bits,
            fed_left_stick: self.fed_left_stick,
            pos: self.pos,
            rot: self.rot,
            vel: self.vel,
            hp: self.hp,
            r_anim: self.r_anim,
            ripper: self.ripper,
            blade: self.blade,
            camera_pos: self.camera_pos,
            camera_look_at: self.camera_look_at,
            camera_rot: camera_angles(self.camera_pos, self.camera_look_at, self.camera_roll),
            input: InputJson {
                buttons: decode_buttons(self.input.buttons_down),
                down_bits: self.input.buttons_down,
                pressed_bits: self.input.buttons_pressed,
                left_stick: self.input.left_stick,
                right_stick: self.input.right_stick,
            },
        }
    }
}

/// Ставит бит игрового кода клавиши в битмаску `m_aKeysDown`/`m_aKeysPressed`
/// (общая кодировка — `drmod_replay_types::key_codes`).
fn set_key_bit(bits: &mut [u32; 6], code: u32) {
    let index = drmod_replay_types::key_codes::index(code);
    if index < 6 {
        bits[index] |= drmod_replay_types::key_codes::bit(code);
    }
}

/// Ставит бит DIK-кода (0–255) в битмап из 8 слов: DIK — это линейный индекс
/// байта в `ms_InputKeys` (в отличие от игровых кодов клавиш — `key_codes`).
fn set_dik_bit(mask: &mut [u32; 8], dik: u32) {
    let index = (dik >> 5) as usize;
    if index < 8 {
        mask[index] |= 1 << (dik & 31);
    }
}

/// Декодирует битмаску `buttons_down` в имена действий (для логов).
fn decode_buttons(down: u32) -> Vec<&'static str> {
    let mut v = Vec::new();
    if down & addresses::input_bits::FORWARD != 0 {
        v.push("forward");
    }
    if down & addresses::input_bits::BACK != 0 {
        v.push("backward");
    }
    if down & addresses::input_bits::LEFT != 0 {
        v.push("left");
    }
    if down & addresses::input_bits::RIGHT != 0 {
        v.push("right");
    }
    if down & addresses::input_bits::JUMP != 0 {
        v.push("jump");
    }
    if down & addresses::input_bits::LIGHT_ATTACK != 0 {
        v.push("light_attack");
    }
    if down & addresses::input_bits::HEAVY_ATTACK != 0 {
        v.push("heavy_attack");
    }
    if down & addresses::input_bits::NINJA_RUN != 0 {
        v.push("ninja_run");
    }
    if down & addresses::input_bits::BLADE != 0 {
        v.push("blade");
    }
    if down & addresses::input_bits::PAUSE != 0 {
        v.push("pause");
    }
    v
}
