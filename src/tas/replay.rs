//! Record/Replay — типы и адреса системы ввода (этапы 0–1: чтение и подача).
//!
//! Подача ввода работает через override глобального `InputUnit[0]`
//! (`base + 0x177B850`) в хуке `cInput::updateInputUnit` (см. `docs/REPLAY.md`).
//! Хук и детуры ввода живут в `hooks.rs`; здесь — состояние override,
//! состояние записи/воспроизведения и ручной инжекции (debug).
//! Логирование — в `crate::logger`. Прямая запись в сырые кэши и поля
//! `Pl0000` не работает — игрок читает ввод из `g_InputUnit0`, а не из этих мест.

#[cfg(debug_assertions)]
use super::addresses;
#[cfg(debug_assertions)]
use super::types::{CameraState, PlayerState, ReplayFrame, ReplayRunMeta};
use super::types::{InputOverride, InputUnit};
use crate::logger;
#[cfg(debug_assertions)]
use crate::segment::{self, in_any_start_zone};
#[cfg(debug_assertions)]
use chrono::Local;
#[cfg(debug_assertions)]
use rusqlite::Connection;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
#[cfg(debug_assertions)]
use std::time::Instant;

const INPUT_OVERRIDE_INIT: InputOverride = InputOverride {
    active: false,
    input: InputUnit {
        buttons_down: 0,
        buttons_pressed: 0,
        buttons_released: 0,
        buttons_alternated: 0,
        left_stick: [0.0; 2],
        right_stick: [0.0; 2],
        left_trigger: 0.0,
        right_trigger: 0.0,
        valid_input: 0,
        repeat_count: 0,
    },
};

static INPUT_OVERRIDE: Mutex<InputOverride> = Mutex::new(INPUT_OVERRIDE_INIT);

/// Буфер кадров воспроизведения для подачи прямо из детура `updateInputUnit`
/// (в тике симуляции, вариант B) — в отличие от `INPUT_OVERRIDE`, который
/// ставится из render и применяется игрой на СЛЕДУЮЩЕМ тике.
/// Заполняется при старте воспроизведения (`start_playback_feed`), очищается
/// при остановке. Кадры предобработаны (`prepare_frame`): weapon_select gate
/// и конвертация raw-клавиш меню выполнены заранее, детур только копирует.
#[cfg(debug_assertions)]
pub(super) static PLAYBACK_FEED: Mutex<PlaybackFeed> = Mutex::new(PlaybackFeed {
    frames: Vec::new(),
    next_idx: 0,
    done: false,
    rsx_correction: 0.0,
});

#[cfg(debug_assertions)]
pub(super) struct PlaybackFeed {
    /// Кадры для подачи (1 кадр на тик симуляции).
    frames: Vec<ReplayFrame>,
    /// Индекс следующего кадра.
    next_idx: usize,
    /// Все кадры поданы — render должен остановить воспроизведение.
    done: bool,
    /// Поправка к right_stick_x для следующего поданного кадра (вариант E:
    /// компенсация курса). Применяется один раз и обнуляется.
    rsx_correction: f32,
}

/// Готовит один кадр к подаче детуром: weapon_select hold-gate и конвертация
/// raw-клавиш меню в D-Pad биты. Раньше выполнялось каждый кадр в render
/// (`playback_tick`); теперь — один раз при старте.
#[cfg(debug_assertions)]
fn prepare_frame(mut f: ReplayFrame) -> ReplayFrame {
    // weapon_select (бит 0x01) в записи: 1-й кадр (фронт pressed) — toggle
    // меню. Удержание (down без pressed) никогда не нужно — зануляем всегда.
    let mut input = f.input;
    if input.buttons_down & addresses::input_bits::WEAPON_SELECT != 0
        && input.buttons_pressed & addresses::input_bits::WEAPON_SELECT == 0
    {
        input.buttons_down &= !addresses::input_bits::WEAPON_SELECT;
    }
    // Навигация в меню: запись хранит её в сырых клавишах (стрелки
    // 0x90/0x93/0x92/0x91, Enter 0x15) — меню читает их через
    // isKeyDown/isKeyPressed из ms_KeyInput, а не из InputUnit.
    // Конвертируем в D-Pad биты InputUnit + фронт pressed. Клавиша 2 (0x2D) —
    // дубликат weapon_select (бит 0x01 уже в InputUnit) — зануляем.
    let mut raw_down = f.raw_down;
    let mut raw_pressed = f.raw_pressed;
    // KEY_ENTER=0x15 (запись), KEY_ESC=0x8E, KEY_UP=0x90,
    // KEY_RIGHT=0x91, KEY_LEFT=0x92, KEY_DOWN=0x93
    let menu_map = [
        (addresses::KEY_UP, addresses::input_bits::MENU_UP),
        (addresses::KEY_DOWN, addresses::input_bits::MENU_DOWN),
        (addresses::KEY_LEFT, addresses::input_bits::MENU_LEFT),
        (addresses::KEY_RIGHT, addresses::input_bits::MENU_RIGHT),
        (0x15, addresses::input_bits::CONFIRM), // Enter = KEY_ENTER в записи
        (addresses::KEY_ESC, addresses::input_bits::CANCEL), // Esc = BUTTON_B (отмена)
    ];
    for (code, bit) in menu_map {
        let idx = (code >> 5) as usize;
        let b = 1u32 << (code & 31);
        if idx < 6 && raw_down[idx] & b != 0 {
            input.buttons_down |= bit;
            if raw_pressed[idx] & b != 0 {
                input.buttons_pressed |= bit;
            }
            raw_down[idx] &= !b;
            raw_pressed[idx] &= !b;
        }
    }
    let key2_idx = (addresses::KEY_DIGIT2 >> 5) as usize;
    let key2_bit = 1u32 << (addresses::KEY_DIGIT2 & 31);
    if key2_idx < 6 && raw_down[key2_idx] & key2_bit != 0 {
        raw_down[key2_idx] &= !key2_bit;
        raw_pressed[key2_idx] &= !key2_bit;
    }
    f.input = input;
    f.raw_down = raw_down;
    f.raw_pressed = raw_pressed;
    f
}

/// Заполняет буфер подачи кадрами воспроизведения (вызывается при старте).
/// `next_idx = 1`: frames[0] — нулевой ввод спавна, не подаётся. Компенсация
/// лага подачи та же, что в варианте A: кадр, заданный в детуре тика N,
/// применяется игрой в тике N+1, поэтому детур тика N+1 подаёт frames[N+1]
/// (в записи тик N+1 имел ввод frames[N+1]).
#[cfg(debug_assertions)]
pub(super) fn start_playback_feed(frames: Vec<ReplayFrame>) {
    if let Ok(mut g) = PLAYBACK_FEED.lock() {
        g.frames = frames.into_iter().map(prepare_frame).collect();
        g.next_idx = 1;
        g.done = false;
    }
    DUP_REQUESTED.store(false, Ordering::Relaxed);
    DUP_LAST.store(0, Ordering::Relaxed);
    DUP_COUNT.store(0, Ordering::Relaxed);
}

/// Очищает буфер подачи (при остановке воспроизведения).
#[cfg(debug_assertions)]
pub(super) fn clear_playback_feed() {
    if let Ok(mut g) = PLAYBACK_FEED.lock() {
        g.frames.clear();
        g.next_idx = 0;
        g.done = false;
    }
}

/// Все ли кадры поданы (детур подал последний) — render останавливает
/// воспроизведение и флашит лог.
#[cfg(debug_assertions)]
pub(super) fn playback_done() -> bool {
    PLAYBACK_FEED.lock().map(|g| g.done).unwrap_or(false)
}

/// Сколько кадров подано / всего (для debug-панели).
#[cfg(debug_assertions)]
pub(super) fn playback_progress() -> (usize, usize) {
    PLAYBACK_FEED
        .lock()
        .map(|g| (g.next_idx.min(g.frames.len()), g.frames.len()))
        .unwrap_or((0, 0))
}

/// Подача следующего кадра из детура `updateInputUnit` (в тике симуляции).
/// Перезаписывает unit + ставит raw-клавиши меню и blade/ripper в атомики —
/// всё применяется игрой в ЭТОМ же тике. Возвращает true, если буфер активен
/// (обычный `INPUT_OVERRIDE` из render применять не нужно).
/// При исчерпании кадров подаёт нулевой ввод и помечает `done`.
/// NOTE: детур вызывается игрой в тике; считается, что для `user_index == 0`
/// это ровно один вызов на тик (стабильность лага 1 в старых данных
/// подтверждает соотношение render:тик = 1:1).
#[cfg(debug_assertions)]
pub(super) fn feed_playback(unit: *mut InputUnit) -> bool {
    let mut g = match PLAYBACK_FEED.lock() {
        Ok(x) => x,
        Err(e) => e.into_inner(),
    };
    if g.frames.is_empty() {
        return false;
    }
    if g.next_idx < g.frames.len() {
        let mut frame = g.frames[g.next_idx];
        // Вариант D: дубль кадра — render запросил повторную подачу текущего
        // кадра (компенсация отставания); next_idx не растёт.
        let dup = DUP_REQUESTED.swap(false, Ordering::Relaxed);
        if !dup {
            g.next_idx += 1;
        }
        // Вариант E: поправка курса — добавляем к right_stick_x один раз.
        if g.rsx_correction != 0.0 {
            frame.input.right_stick[0] += g.rsx_correction;
            g.rsx_correction = 0.0;
        }
        unsafe { *unit = frame.input };
        // Raw-клавиши меню и blade/ripper — в атомики; детур применит raw
        // ниже (apply_raw_keys), blade/ripper игра прочитает в handleActions
        // того же тика.
        super::hooks::set_raw_keys(frame.raw_down, frame.raw_pressed);
        if frame.ripper_pressed != 0 {
            super::hooks::set_ripper_frames(1);
        }
        let blade_on = frame.blade_down != 0;
        if blade_on != super::hooks::blade_hold() {
            super::hooks::set_blade_hold(blade_on);
        }
    } else {
        // Конец: отпустить кнопки, render вскоре остановит воспроизведение.
        unsafe { *unit = InputUnit::default() };
        g.done = true;
    }
    true
}

/// Базовый адрес модуля игры (для чтения GameMenuStatus при гейте
/// weapon_select в playback). Устанавливается один раз при init.
static BASE_ADDR: AtomicUsize = AtomicUsize::new(0);

// --- Вариант D: дубль кадра ввода (компенсация отставания вдоль) ---
// Render оценивает отставание вдоль вектора движения; при превышении порога
// и безопасном hold-кадре ставит DUP_REQUESTED; детур feed_playback подаёт
// текущий кадр повторно (не инкрементит next_idx) — персонаж проходит ещё
// одно смещение кадра (~0.11-0.16 м) и догоняет запись.

/// Запрошен ли дубль следующего кадра (render ставит, детур снимает).
#[cfg(debug_assertions)]
static DUP_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Индекс подачи, на котором сделан последний дубль (частота: не чаще 1 на 3).
#[cfg(debug_assertions)]
static DUP_LAST: AtomicUsize = AtomicUsize::new(0);
/// Число дублей за текущий прогон (лимит).
#[cfg(debug_assertions)]
static DUP_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Порог отставания вдоль (м), при котором запрашиваем дубль.
const DUP_LAG_THRESHOLD: f32 = 0.15;
/// Минимальный интервал между дублями (кадров подачи).
const DUP_MIN_INTERVAL: usize = 3;
/// Вариант D ВЫКЛЮЧЕН (2026-08-23): дубль в hold-окне перед прыжком продлевал
/// движение и давал ПЕРЕЛЁТ (визуально: персонаж прыгнул слишком далеко,
/// фейлы 98/99 в прогоне 96). Для включения — `true` и подобрать пороги/
/// безопасное окно.
const DUP_ENABLED: bool = false;
/// Максимум дублей за прогон (если вариант D включён).
const DUP_MAX: usize = 30;
/// Окно безопасности перед фронтом/сменой анимации (кадров).
const DUP_SAFE_AHEAD: usize = 5;

/// Анимации с поступательным движением — в них дубль кадра безопасен
/// (бег, ninja run, ходьба; переходы 13/14 не включаем).
const DUP_MOVING_ANIMS: &[i32] = &[5, 71, 4, 11, 3];

/// Запрашивает дубль следующего кадра (вызывается из render при отставании).
#[cfg(debug_assertions)]
pub(super) fn request_frame_dup() {
    DUP_REQUESTED.store(true, Ordering::Relaxed);
}

/// Оценивает, нужно ли продублировать следующий кадр: отставание вдоль
/// превысило порог и текущий кадр — безопасный hold (удержание движения,
/// без фронтов в ближайшие `DUP_SAFE_AHEAD` кадров). Вызывается из render
/// (`playback_tick`) с текущей позицией игрока.
#[cfg(debug_assertions)]
pub(super) fn should_dup_frame(play_pos: [f32; 3]) -> bool {
    if !DUP_ENABLED {
        return false; // вариант D отключён (перелёт при прыжках)
    }
    let g = match PLAYBACK_FEED.lock() {
        Ok(x) => x,
        Err(e) => e.into_inner(),
    };
    let n = g.frames.len();
    let idx = g.next_idx;
    if g.frames.is_empty() || idx == 0 || idx >= n {
        return false;
    }
    // Направление движения: разность записанных позиций поданного и следующего кадра.
    let prev = g.frames[idx - 1].state.pos;
    let next = g.frames[idx].state.pos;
    let dir = [next[0] - prev[0], next[1] - prev[1], next[2] - prev[2]];
    let dlen = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if dlen < 0.01 {
        return false; // не двигаемся
    }
    // Отставание вдоль: проекция (play_pos - prev) на направление движения.
    let along = ((play_pos[0] - prev[0]) * dir[0]
        + (play_pos[1] - prev[1]) * dir[1]
        + (play_pos[2] - prev[2]) * dir[2])
        / dlen;
    if along > -DUP_LAG_THRESHOLD {
        return false; // не отстали
    }
    // Безопасность: удержание движения, без фронтов впереди.
    let cur = g.frames[idx].input;
    if cur.buttons_pressed != 0 || cur.buttons_released != 0 {
        return false;
    }
    if cur.left_stick[0] == 0.0 && cur.left_stick[1] == 0.0 {
        return false; // нет движения (стик в нуле)
    }
    if !DUP_MOVING_ANIMS.contains(&g.frames[idx].state.r_anim) {
        return false;
    }
    let end = (idx + DUP_SAFE_AHEAD).min(n);
    for i in idx..end {
        let f = g.frames[i];
        if i > idx
            && (f.input.buttons_down != cur.buttons_down
                || f.state.r_anim != g.frames[idx].state.r_anim)
        {
            return false; // впереди смена ввода/анимации
        }
        if i > idx && (f.input.buttons_pressed != 0 || f.input.buttons_released != 0) {
            return false; // впереди фронт
        }
    }
    // Частота и лимит.
    if idx.saturating_sub(DUP_LAST.load(Ordering::Relaxed)) < DUP_MIN_INTERVAL {
        return false;
    }
    if DUP_COUNT.load(Ordering::Relaxed) >= DUP_MAX {
        return false;
    }
    DUP_LAST.store(idx, Ordering::Relaxed);
    DUP_COUNT.fetch_add(1, Ordering::Relaxed);
    logger::log_line(&format!(
        "playback: frame dup at idx {} (along {:.2} m)",
        idx, along
    ));
    true
}

// --- Вариант E: компенсация курса — поправка к right_stick_x ---
// render считает отклонение cam_yaw от записи и ставит поправку для
// следующего поданного кадра; детур добавляет её к right_stick_x.
// Чувствительность ~0.00065 °/ед/кадр (analyze7_stick.py) = 1.13e-5 рад/ед.

/// Чувствительность стика: рад на единицу right_stick за кадр.
const CAM_SENS_RAD: f32 = 0.00065 * std::f32::consts::PI / 180.0;
/// Порог отклонения yaw (рад), ниже которого не корректируем (шум камеры).
const CAM_THRESHOLD_RAD: f32 = 0.5 * std::f32::consts::PI / 180.0;
/// Частичность коррекции (0..1) — поворот тела следует за камерой плавно,
/// полная компенсация за 1 кадр даст перекоррекцию.
const CAM_GAIN: f32 = 0.6;
/// Клэмп поправки (ед. стика) — как значения в записи (до ~2000).
const CAM_CLAMP: f32 = 2000.0;
/// Максимальный корректируемый угол: при |dYaw| >= 90° камера смотрит «не туда»
/// (старт/спавн/рестарт) — коррекция там раскручивала камеру (прогон 101:
/// развернуло на 180°). Дрейф курса — это единицы градусов.
const CAM_MAX_CORRECT_RAD: f32 = 90.0 * std::f32::consts::PI / 180.0;

/// cam_yaw из CameraState (как в dbdump dump.rs): atan2(dx, dz).
#[cfg(debug_assertions)]
fn cam_yaw(cam: &CameraState) -> f32 {
    let dx = cam.look_at[0] - cam.pos[0];
    let dz = cam.look_at[2] - cam.pos[2];
    dx.atan2(dz)
}

/// Считает поправку к right_stick_x по отклонению текущей камеры от записи.
/// Эталон — последний ПОДАННЫЙ кадр (`next_idx - 1`), а не будущий: сравнение
/// с будущим кадром при активной коррекции не сходилось (эталон «уезжал»
/// вместе с поворотом) и раскручивало камеру. Возвращает 0, если отклонение
/// вне (порог, 90°) или кадров нет.
#[cfg(debug_assertions)]
pub(super) fn camera_correction(cam: &CameraState) -> f32 {
    let g = match PLAYBACK_FEED.lock() {
        Ok(x) => x,
        Err(e) => e.into_inner(),
    };
    if g.frames.is_empty() || g.next_idx == 0 {
        return 0.0;
    }
    let idx = g.next_idx - 1; // последний поданный кадр — эталон текущего момента
    let rec_yaw = cam_yaw(&g.frames[idx].camera);
    let play_yaw = cam_yaw(cam);
    let mut dyaw = play_yaw - rec_yaw;
    // перенос в [-PI, PI)
    dyaw =
        (dyaw + std::f32::consts::PI).rem_euclid(2.0 * std::f32::consts::PI) - std::f32::consts::PI;
    if dyaw.abs() >= CAM_MAX_CORRECT_RAD || dyaw.abs() < CAM_THRESHOLD_RAD {
        return 0.0;
    }
    // Знак: положительный rsx поворачивает камеру ВЛЕВО (yaw уменьшается) —
    // подтверждено record 104 (rsx 1400..6600 -> dYaw -2..-4°/кадр) и
    // analyze7_stick.py. При dYaw > 0 (камера правее записи) нужен rsx > 0,
    // поэтому corr = +dyaw / SENS * GAIN (с минусом коррекция ПРОТИВОДЕЙСТВОВАЛА
    // записанному повороту и разворачивала камеру: прогоны 101/104).
    let corr = (dyaw / CAM_SENS_RAD * CAM_GAIN).clamp(-CAM_CLAMP, CAM_CLAMP);
    logger::log_line(&format!(
        "playback: cam correction rsx={:.0} (dYaw {:.2} deg)",
        corr,
        dyaw * 180.0 / std::f32::consts::PI
    ));
    corr
}

/// Ставит поправку к right_stick_x для следующего поданного кадра.
#[cfg(debug_assertions)]
pub(super) fn set_rsx_correction(v: f32) {
    if let Ok(mut g) = PLAYBACK_FEED.lock() {
        g.rsx_correction = v;
    }
}

/// Устанавливает базовый адрес модуля (вызывается при init из `lib.rs`).
pub fn set_base_addr(base: usize) {
    BASE_ADDR.store(base, Ordering::Relaxed);
}

/// Возвращает базовый адрес модуля.
#[allow(dead_code)]
pub fn base_addr() -> usize {
    BASE_ADDR.load(Ordering::Relaxed)
}

/// Читает текущий GameMenuStatus (base + 0x17E9F9C) из памяти игры.
fn game_menu_status() -> i32 {
    let base = BASE_ADDR.load(Ordering::Relaxed);
    if base == 0 {
        return -1;
    }
    unsafe { (base as *const i32).add(0x17E9F9C / 4).read_unaligned() }
}

/// Последнее значение m_CurrentInput.buttons_down<<32 | buttons_pressed —
/// для ловли фронтов (pressed/down) при реальном вводе.
static LAST_CUR_IN: AtomicU64 = AtomicU64::new(0);

/// Возвращает true, если (down, pressed) изменились с прошлого вызова,
/// и запоминает новые значения. Используется в render для логирования
/// однократных нажатий (прыжок/атаки), которые иначе проскакивают
/// между периодическими frame-логами.
pub fn cur_in_changed(down: u32, pressed: u32) -> bool {
    let key = ((down as u64) << 32) | pressed as u64;
    LAST_CUR_IN.swap(key, Ordering::Relaxed) != key
}

/// Устанавливает override для хука ввода (вызывается из render).
/// Логирует изменения состояния в debug.log.
pub fn set_input_override(ov: InputOverride) {
    if let Ok(mut guard) = INPUT_OVERRIDE.lock() {
        let changed = guard.active != ov.active
            || guard.input.left_stick != ov.input.left_stick
            || guard.input.right_stick != ov.input.right_stick
            || guard.input.buttons_down != ov.input.buttons_down
            || guard.input.buttons_pressed != ov.input.buttons_pressed;
        if changed {
            logger::log_line(&format!(
                "set_override: active={} down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2})",
                ov.active,
                ov.input.buttons_down,
                ov.input.buttons_pressed,
                ov.input.left_stick[0],
                ov.input.left_stick[1],
                ov.input.right_stick[0],
                ov.input.right_stick[1]
            ));
        }
        *guard = ov;
    }
}

/// Доступ к текущему override (используется в debug-панели).
pub fn input_override() -> MutexGuard<'static, InputOverride> {
    INPUT_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Применяет активный override к unit (вызывается из детура
/// `updateInputUnit` в `hooks.rs`).
pub(super) fn apply_override(unit: *mut InputUnit) {
    if let Ok(guard) = INPUT_OVERRIDE.lock()
        && guard.active
    {
        unsafe { *unit = guard.input };
    }
}

/// Состояние активной записи (NumPad5, отложенный старт по триггеру).
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct RecordState {
    pub(crate) armed: bool,
    pub(crate) active: bool,
    pub(crate) frames: Vec<ReplayFrame>,
    pub(crate) start: Option<Instant>,
    pub(crate) started_at: String,
    pub(crate) mission_id: i32,
    pub(crate) mission_name: String,
    pub(crate) last_id: Option<i64>,
}

/// Состояние воспроизведения (NumPad6, отложенный старт по триггеру).
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct PlaybackState {
    pub(crate) armed: bool,
    pub(crate) active: bool,
    pub(crate) frames: Vec<ReplayFrame>,
    pub(crate) frame_idx: usize,
    pub(crate) log: Vec<ReplayFrame>,
    pub(crate) start: Option<Instant>,
    pub(crate) started_at: String,
    pub(crate) last_id: Option<i64>,
}

/// Состояние ручной инжекции ввода (debug-панель + NumPad4-скрипт).
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct InjectionState {
    pub(crate) w: bool,
    pub(crate) camera: bool,
    pub(crate) jump_frames: u32,
    pub(crate) light_frames: u32,
    pub(crate) heavy_frames: u32,
    pub(crate) script_frames: u32,
}

/// Всё состояние Record/Replay (debug): запись + воспроизведение + инжекция.
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct ReplayState {
    pub(crate) record: RecordState,
    pub(crate) playback: PlaybackState,
    pub(crate) inject: InjectionState,
}

#[cfg(debug_assertions)]
impl ReplayState {
    /// Переключает запись по NumPad5 (отложенный старт).
    /// `active → стоп + flush`, `armed → отмена`, `idle → arm` (старт по триггеру).
    pub(crate) fn toggle_record(&mut self, conn: Option<&Connection>) {
        if self.record.active {
            self.stop_record(conn);
        } else if self.record.armed {
            self.record.armed = false;
        } else {
            self.stop_playback(conn);
            self.playback.armed = false;
            self.record.armed = true;
        }
    }

    /// Переключает воспроизведение по NumPad6 (отложенный старт).
    /// `active → стоп + flush`, `armed → отмена`, `idle → arm` (требует кадры).
    pub(crate) fn toggle_playback(&mut self, conn: Option<&Connection>) {
        if self.playback.active {
            self.stop_playback(conn);
        } else if self.playback.armed {
            self.playback.armed = false;
        } else if !self.playback.frames.is_empty() {
            self.stop_record(conn);
            self.record.armed = false;
            set_input_override(InputOverride::default());
            // Снять остатки ручной keybind-эмуляции (NumPad7/8) до старта.
            super::hooks::clear_keybind_emulation();
            self.playback.armed = true;
        }
    }

    /// Останавливает запись и флашит буфер в БД (kind=record). Кадры
    /// сохраняются в `playback.frames`, чтобы запись можно было воспроизвести.
    pub(crate) fn stop_record(&mut self, conn: Option<&Connection>) {
        if !self.record.active {
            return;
        }
        self.record.active = false;
        let frames = std::mem::take(&mut self.record.frames);
        let duration_ms = self
            .record
            .start
            .map(|i| i.elapsed().as_millis() as i64)
            .unwrap_or(0);
        self.record.start = None;
        let meta = ReplayRunMeta {
            kind: "record",
            mission_id: self.record.mission_id,
            mission_name: self.record.mission_name.clone(),
            started_at: self.record.started_at.clone(),
            duration_ms,
            source_replay_id: None,
        };
        self.record.last_id = conn.and_then(|conn| super::db::flush_replay(conn, &meta, &frames));
        self.playback.frames = frames;
        logger::log_line(&format!(
            "record: stop frames={} id={:?}",
            self.playback.frames.len(),
            self.record.last_id
        ));
    }

    /// Останавливает воспроизведение, снимает override и флашит лог результата
    /// в БД (kind=playback, source_replay_id=id исходной записи).
    pub(crate) fn stop_playback(&mut self, conn: Option<&Connection>) {
        set_input_override(InputOverride::default());
        // Снять keybind-эмуляцию (ripper/blade): иначе удержание blade
        // останется активным и будет подмешиваться в реальный ввод.
        super::hooks::clear_keybind_emulation();
        // Очистить буфер подачи детура (вариант B).
        clear_playback_feed();
        let was_active = self.playback.active;
        self.playback.active = false;
        self.playback.frame_idx = 0;
        if was_active && !self.playback.log.is_empty() {
            let log = std::mem::take(&mut self.playback.log);
            let duration_ms = self
                .playback
                .start
                .map(|i| i.elapsed().as_millis() as i64)
                .unwrap_or(0);
            let meta = ReplayRunMeta {
                kind: "playback",
                mission_id: self.record.mission_id,
                mission_name: self.record.mission_name.clone(),
                started_at: self.playback.started_at.clone(),
                duration_ms,
                source_replay_id: self.record.last_id,
            };
            self.playback.last_id =
                conn.and_then(|conn| super::db::flush_replay(conn, &meta, &log));
            logger::log_line(&format!(
                "playback: stop frames={} id={:?}",
                log.len(),
                self.playback.last_id
            ));
        }
        self.playback.start = None;
    }

    /// Отложенный старт: если arm и игрок в триггере — запускает запись или
    /// воспроизведение. Вызывается из `update` каждый кадр.
    fn update_deferred_start(
        &mut self,
        pos: Option<segment::Vec3>,
        mission_id: i32,
        mission_name: &str,
    ) {
        if !in_any_start_zone(pos) {
            return;
        }
        if self.record.armed {
            self.record.armed = false;
            logger::log_line("deferred: trigger -> start recording");
            // Запись ловит только реальный ввод — сбрасываем keybind-эмуляцию,
            // чтобы остатки ручного NumPad7/8 не подмешались в кадры.
            super::hooks::clear_keybind_emulation();
            self.record.active = true;
            self.record.frames.clear();
            self.record.start = Some(Instant::now());
            self.record.started_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            self.record.mission_id = mission_id;
            self.record.mission_name = mission_name.to_string();
        }
        if self.playback.armed {
            self.playback.armed = false;
            logger::log_line("deferred: trigger -> start playback");
            set_input_override(InputOverride::default());
            super::hooks::clear_keybind_emulation();
            self.playback.active = true;
            self.playback.frame_idx = 0;
            // Заполняем буфер подачи для детура (вариант B).
            start_playback_feed(self.playback.frames.clone());
            self.playback.log.clear();
            self.playback.start = Some(Instant::now());
            self.playback.started_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        }
    }

    /// Останавливает активную запись/воспроизведение при входе в loading.
    /// Arm НЕ снимается — он должен пережить loading и сработать на спавне.
    pub(crate) fn stop_on_loading(&mut self, conn: Option<&Connection>) {
        if self.record.active {
            logger::log_line("loading: stop active recording");
            self.stop_record(conn);
        }
        if self.playback.active {
            logger::log_line("loading: stop active playback");
            self.stop_playback(conn);
        }
        // Сбрасываем keybind-эмуляцию (ripper/blade): иначе при рестарте
        // детуры isKeybindPressed/isKeybindDown возвращают 1 на пересоздающемся
        // игроке → handleActions падает (access violation).
        super::hooks::clear_keybind_emulation();
    }

    /// Этап 1 (debug): инжекция ввода через override хука updateInputUnit.
    /// Все действия пишутся в глобальный InputUnit[0] — реальный источник
    /// входа игрока (прямая запись в поля Pl0000 в Present не работает:
    /// поздно — после handleActions). Биты — см. `addresses::input_bits`.
    /// Вызывается из `update` каждый кадр.
    fn update_input_injection(&mut self) {
        // Во время воспроизведения override управляется исключительно playback —
        // debug-инъекция не должна затирать применяемый кадр.
        if self.playback.active {
            return;
        }

        // Скрипт-последовательность (NumPad4): бег ~1 сек → прыжок на бегу →
        // лёгкий удар → поворот камеры. Тайминги в кадрах (60 FPS).
        const SCRIPT_RUN_END: u32 = 60; // бег первые 60 кадров (~1 сек)
        const SCRIPT_JUMP_AT: u32 = 45; // прыжок на 45-м кадре (на бегу)
        const SCRIPT_ATTACK_AT: u32 = 85; // лёгкий удар на 85-м кадре (после)
        const SCRIPT_CAMERA_AT: u32 = 95; // поворот камеры с 95-го кадра
        const SCRIPT_TOTAL: u32 = 130; // конец скрипта

        let script_active = self.inject.script_frames > 0;
        let jump_active = self.inject.jump_frames > 0;
        let light_active = self.inject.light_frames > 0;
        let heavy_active = self.inject.heavy_frames > 0;
        let active = script_active
            || self.inject.w
            || self.inject.camera
            || jump_active
            || light_active
            || heavy_active;

        let mut unit = InputUnit {
            valid_input: 1,
            ..Default::default()
        };

        if script_active {
            let t = self.inject.script_frames;
            if t <= SCRIPT_RUN_END {
                unit.buttons_down |= addresses::input_bits::FORWARD;
                unit.left_stick = [0.0, -1000.0];
            }
            if (SCRIPT_JUMP_AT..SCRIPT_JUMP_AT + 2).contains(&t) {
                unit.buttons_down |= addresses::input_bits::JUMP;
                unit.buttons_pressed |= addresses::input_bits::JUMP;
            }
            if (SCRIPT_ATTACK_AT..SCRIPT_ATTACK_AT + 2).contains(&t) {
                unit.buttons_down |= addresses::input_bits::LIGHT_ATTACK;
                unit.buttons_pressed |= addresses::input_bits::LIGHT_ATTACK;
            }
            if (SCRIPT_CAMERA_AT..SCRIPT_TOTAL).contains(&t) {
                // Поворот камеры вправо (мышь = right_stick, дельта в пикселях)
                unit.right_stick = [300.0, 0.0];
            }
            self.inject.script_frames += 1;
            if self.inject.script_frames > SCRIPT_TOTAL {
                self.inject.script_frames = 0;
            }
        }

        if self.inject.w {
            unit.buttons_down |= addresses::input_bits::FORWARD;
            unit.left_stick = [0.0, -1000.0];
        }
        if jump_active {
            unit.buttons_down |= addresses::input_bits::JUMP;
            unit.buttons_pressed |= addresses::input_bits::JUMP;
            self.inject.jump_frames -= 1;
        }
        if light_active {
            unit.buttons_down |= addresses::input_bits::LIGHT_ATTACK;
            unit.buttons_pressed |= addresses::input_bits::LIGHT_ATTACK;
            self.inject.light_frames -= 1;
        }
        if heavy_active {
            unit.buttons_down |= addresses::input_bits::HEAVY_ATTACK;
            unit.buttons_pressed |= addresses::input_bits::HEAVY_ATTACK;
            self.inject.heavy_frames -= 1;
        }
        if self.inject.camera {
            unit.right_stick = [500.0, 0.0];
        }
        set_input_override(InputOverride {
            active,
            input: unit,
        });
    }

    /// Единый покадровый апдейт Record/Replay (вызывается из render).
    /// Порядок важен: инжекция → отложенный старт → захват кадра записи →
    /// подача кадра воспроизведения. Кадр (input/state/camera) читается
    /// вызывающей стороной один раз и используется и для записи, и для
    /// лога воспроизведения.
    pub(crate) fn update(
        &mut self,
        conn: Option<&Connection>,
        pos: Option<segment::Vec3>,
        mission_id: i32,
        mission_name: &str,
        input: InputUnit,
        state: PlayerState,
        camera: CameraState,
    ) {
        self.update_input_injection();
        self.update_deferred_start(pos, mission_id, mission_name);
        self.capture_frame(input, state, camera);
        self.playback_tick(conn, input, state, camera);
        // Сырые клавиши НЕ сбрасываем здесь: override/raw-биты, выставленные
        // в render(K), применяются игрой на тике K+1 — сброс в конце кадра
        // убил бы их до применения. Каждый кадр playback перезаписывает биты
        // целиком (set_raw_keys), финальный сброс — в stop_playback.
        // Сброс сэмплов blade/ripper в конце кадра: следующий тик накапливает
        // сэмплы с нуля, а кадр 0 записи (старт в этом же render) уже прочитал
        // сэмплы прошедшего тика.
        super::hooks::reset_keybind_samples();
    }

    /// Захват кадра записи (вызывается из `update` каждый кадр).
    /// Флаги blade/ripper берутся из сэмплов детуров `isKeybindDown`/
    /// `isKeybindPressed` — реальный ввод, который игра видела в прошедшем
    /// тике, а не реконструкция из состояния. Сырые клавиши (m_aKeysDown/
    /// m_aKeysPressed из ms_KeyInput) — сэмпл детура `updateInputUnit` ПОСЛЕ
    /// оригинала: меню читает стрелки/Enter/Esc через `isKeyDown`/
    /// `isKeyPressed` из этого кэша, а не из InputUnit.
    fn capture_frame(&mut self, input: InputUnit, state: PlayerState, camera: CameraState) {
        if self.record.active {
            let (raw_down, raw_pressed) = super::hooks::read_raw_keys_sampled();
            self.record.frames.push(ReplayFrame {
                frame_index: self.record.frames.len() as u32,
                input,
                state,
                camera,
                blade_down: super::hooks::read_blade_down_sampled() as u8,
                ripper_pressed: super::hooks::read_ripper_pressed_sampled() as u8,
                raw_down,
                raw_pressed,
            });
        }
    }

    /// Захват результата кадра воспроизведения (вызывается из `update` каждый
    /// render-кадр). Подача кадров — в детуре `updateInputUnit` (вариант B):
    /// детур берёт следующий кадр из `PLAYBACK_FEED` в тике симуляции, так что
    /// фаза Present↔тик не влияет на момент применения ввода. Здесь — только
    /// лог результата (состояние после тика, прочитанное в render) и остановка,
    /// когда детур подал все кадры.
    /// playback.log[N] = результат кадра frame[N] — синхронно с record[N].
    fn playback_tick(
        &mut self,
        conn: Option<&Connection>,
        cur_input: InputUnit,
        state: PlayerState,
        camera: CameraState,
    ) {
        if !self.playback.active {
            return;
        }
        if playback_done() {
            // Все кадры поданы детуром — снять override и флашнуть лог.
            self.stop_playback(conn);
            return;
        }
        // Счётчик подачи для debug-панели.
        self.playback.frame_idx = playback_progress().0;
        self.playback.log.push(ReplayFrame {
            frame_index: self.playback.log.len() as u32,
            input: cur_input,
            state,
            camera,
            blade_down: super::hooks::read_blade_down_sampled() as u8,
            ripper_pressed: super::hooks::read_ripper_pressed_sampled() as u8,
            raw_down: super::hooks::read_raw_keys_sampled().0,
            raw_pressed: super::hooks::read_raw_keys_sampled().1,
        });
        // Вариант D: компенсация отставания вдоль — если текущий кадр —
        // безопасный hold и мы отстали от записи, продублировать следующий
        // кадр (детур подаст его повторно, next_idx не сдвинется).
        if should_dup_frame(state.pos) {
            request_frame_dup();
        }
        // Вариант E: компенсация курса — поправка к right_stick_x по
        // отклонению cam_yaw от записи (применяется следующим поданным кадром).
        set_rsx_correction(camera_correction(&camera));
    }
}
