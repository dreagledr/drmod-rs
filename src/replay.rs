//! Record/Replay — типы и адреса системы ввода (этапы 0–1: чтение и подача).
//!
//! Подача ввода работает через override глобального `InputUnit[0]`
//! (`base + 0x177B850`) в хуке `cInput::updateInputUnit` (см. `docs/REPLAY.md`).
//! Прямая запись в сырые кэши и поля `Pl0000` не работает — игрок читает
//! ввод из `g_InputUnit0`, а не из этих мест.

/// cInput::ms_KeyInput — сырой ввод клавиатуры (вспомогательный кэш,
/// игроком для движения не читается).
pub const KEY_INPUT: usize = 0x177B7C0;
/// cInput::ms_MouseInput — сырой ввод мыши (вспомогательный кэш).
pub const MOUSE_INPUT: usize = 0x177B798;
/// cInput::updateInputUnit(InputUnit*, int userIndex) — функция, которую игра
/// вызывает каждый тик для заполнения глобального InputUnit из DirectInput.
/// Хук перехватывает её и перезаписывает unit[0] после вызова оригинала.
pub const UPDATE_INPUT_UNIT: usize = 0x9DAFE0;
/// Pl0000::m_CurrentInput — копия `g_InputUnit0` (смещение от объекта Pl0000).
pub const CURRENT_INPUT_OFFSET: usize = 0xCF8;
/// Глобальный InputUnit[0] (cInput) — реальный источник входа игрока.
/// Pl0000::updateInput копирует его в m_CurrentInput (гипотеза №1 FINDINGS).
pub const GLOBAL_INPUT_UNIT0: usize = 0x177B850;

/// Биты действий в `InputUnit.buttons_down`/`buttons_pressed` (эмпирически,
/// подтверждено сопоставлением с сырыми клавишами/мышью в debug-логе).
pub mod input_bits {
    /// Прыжок (Space)
    pub const JUMP: u32 = 0x0000_0001;
    /// Лёгкая атака (ЛКМ)
    pub const LIGHT_ATTACK: u32 = 0x0000_0040;
    /// Тяжёлая атака (ПКМ)
    pub const HEAVY_ATTACK: u32 = 0x0000_0080;
    /// Движение вперёд (W) — сопутствует left_stick=(0,-1000)
    pub const FORWARD: u32 = 0x0040_0000;
}

/// Pl0000::m_fInputMagnitudeSquared — квадрат магнитуды ввода.
pub const PL_INPUT_MAG_SQ: usize = 0xD28;
/// Pl0000::m_fInputDirection — направление ввода (спроецировано на камеру).
pub const PL_INPUT_DIR: usize = 0xD2C;
/// Pl0000::m_nButtonJump — прыжок.
pub const PL_BUTTON_JUMP: usize = 0xE18;
/// Pl0000::m_nButtonLightAttack — лёгкая атака.
pub const PL_BUTTON_LIGHT_ATTACK: usize = 0xE20;
/// Pl0000::m_nButtonHeavyAttack — тяжёлая атака.
pub const PL_BUTTON_HEAVY_ATTACK: usize = 0xE24;
/// Pl0000::m_nButtonAction — действие.
pub const PL_BUTTON_ACTION: usize = 0xE38;
/// Pl0000::m_nButtonNinjarun — ниндзя-бег.
pub const PL_BUTTON_NINJARUN: usize = 0xE48;
/// Pl0000::m_nButtonBlademode — блейд-мод.
pub const PL_BUTTON_BLADEMODE: usize = 0xE50;
/// Pl0000::m_nButtonUseItem — предмет.
pub const PL_BUTTON_USEITEM: usize = 0xE58;

/// Сырой ввод клавиатуры (cInput::KeyInput).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyInput {
    /// +0x00 m_aKeysDown — зажатые клавиши (битовая маска)
    pub keys_down: [u32; 6],
    /// +0x18 m_aKeysPressed — однократное нажатие в этом кадре
    pub keys_pressed: [u32; 6],
    /// +0x30 m_aKeysReleased — отпущенные в этом кадре
    pub keys_released: [u32; 6],
    /// +0x48 m_aKeysAlternated — «перещёлкнутые»
    pub keys_alternated: [u32; 6],
    /// +0x60 m_aKeyHistory — история нажатий
    pub key_history: [u32; 6],
    /// +0x78 m_nPressDelay — задержка повтора
    pub press_delay: i32,
}

/// Снимок сырого состояния мыши (cInput::MouseInput, читается по полям —
/// раскладка между +0x18 и +0x1C в SDK не уточнена).
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseState {
    /// +0x00 m_nMouseButtons — зажатые кнопки (битовая маска)
    pub buttons: i32,
    /// +0x04 m_nButtonsPressed — нажатые в этом кадре
    pub buttons_pressed: i32,
    /// +0x10 m_MousePosition — текущая позиция курсора
    pub position: [f32; 2],
    /// +0x20 m_LastMousePosition — позиция на прошлом кадре
    pub last_position: [f32; 2],
}

/// Нормализованный ввод игрока (cInput::InputUnit).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InputUnit {
    /// +0x00 m_nButtonsDown
    pub buttons_down: u32,
    /// +0x04 m_nButtonsPressed
    pub buttons_pressed: u32,
    /// +0x08 m_nButtonsReleased
    pub buttons_released: u32,
    /// +0x0C m_nButtonsAlternated
    pub buttons_alternated: u32,
    /// +0x10 m_fLeftStick
    pub left_stick: [f32; 2],
    /// +0x18 m_fRightStick
    pub right_stick: [f32; 2],
    /// +0x20 m_fLeftTrigger
    pub left_trigger: f32,
    /// +0x24 m_fRightTrigger
    pub right_trigger: f32,
    /// +0x28 m_bValidInput
    pub valid_input: i32,
    /// +0x2C m_nRepeatCount
    pub repeat_count: i32,
}

/// Снимок нормализованного ввода игрока (Pl0000) — поля, которые реально
/// двигают персонажа. Смещения из SDK `Pl0000.h` (якоря 0xB74/0x13FC).
#[derive(Clone, Copy, Debug, Default)]
pub struct PlInputSnapshot {
    /// m_CurrentInput (0xCF8) — кнопки + стики + триггеры
    pub input: InputUnit,
    /// m_fInputMagnitudeSquared (0xD28)
    pub input_mag_sq: f32,
    /// m_fInputDirection (0xD2C)
    pub input_direction: f32,
    /// m_nButtonJump (0xE18)
    pub button_jump: i32,
    /// m_nButtonLightAttack (0xE20)
    pub button_light_attack: i32,
    /// m_nButtonHeavyAttack (0xE24)
    pub button_heavy_attack: i32,
    /// m_nButtonAction (0xE38)
    pub button_action: i32,
    /// m_nButtonNinjarun (0xE48)
    pub button_ninjarun: i32,
    /// m_nButtonBlademode (0xE50)
    pub button_blademode: i32,
    /// m_nButtonUseItem (0xE58)
    pub button_use_item: i32,
}

/// Значения, подменяющие ввод игрока в хуке `updateInputUnit`.
/// `active = false` — реальный ввод проходит без изменений.
/// `input` — полный InputUnit, который записывается в `g_InputUnit0`.
#[derive(Clone, Copy, Debug, Default)]
pub struct InputOverride {
    pub active: bool,
    pub input: InputUnit,
}

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

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
static ORIG_UPDATE_INPUT_UNIT: OnceLock<unsafe extern "C" fn(*mut InputUnit, i32)> =
    OnceLock::new();
static DETOUR_COUNT: AtomicU32 = AtomicU32::new(0);
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
            log_line(&format!(
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

/// Сохраняет trampoline (адрес оригинальной функции) после создания хука.
pub fn set_original_update_input_unit(
    orig: unsafe extern "C" fn(*mut InputUnit, i32),
) -> Result<(), ()> {
    ORIG_UPDATE_INPUT_UNIT.set(orig).map_err(|_| ())
}

static LOG_MUTEX: Mutex<()> = Mutex::new(());

/// Дописывает строку в `%LOCALAPPDATA%\drmod\debug.log` с таймстампом.
/// Используется для отладки хука ввода (детур/override).
pub fn log_line(line: &str) {
    let Ok(_guard) = LOG_MUTEX.lock() else {
        return;
    };
    let Ok(localappdata) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let path = format!("{}\\drmod\\debug.log", localappdata);
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let _ = std::io::Write::write_fmt(
        &mut f,
        format_args!(
            "[{}] {}\r\n",
            chrono::Local::now().format("%H:%M:%S%.3f"),
            line
        ),
    );
}

/// Буфер отдельного лога состояния (velocity/rotation/heading/ripper/blade/...).
/// Пишется пачками, чтобы не открывать файл 60 раз в секунду.
static STATE_LOG_BUF: Mutex<Vec<String>> = Mutex::new(Vec::new());
static STATE_LOG_COUNT: AtomicU32 = AtomicU32::new(0);
const STATE_LOG_FLUSH_EVERY: u32 = 60;

/// Перезатирает `%LOCALAPPDATA%\drmod\state.log` при старте мода и очищает
/// буфер — чтобы лог не рос между сессиями.
pub fn init_state_log() {
    let Ok(localappdata) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let dir = format!("{}\\drmod", localappdata);
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{}\\state.log", dir);
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = std::io::Write::write_fmt(
            &mut f,
            format_args!(
                "[{}] state log started\r\nf=frame pos=(x,y,z) vel=(x,y,z, vertical-only) prev=(0x900, last pos) rot=(x,y,z) heading dir ripper blade ninja jump cam=(x,y,z)\r\n",
                chrono::Local::now().format("%H:%M:%S%.3f")
            ),
        );
    }
    if let Ok(mut buf) = STATE_LOG_BUF.lock() {
        buf.clear();
    }
    STATE_LOG_COUNT.store(0, Ordering::Relaxed);
}

/// Дописывает строку состояния в буфер; флашится в `state.log` каждые
/// `STATE_LOG_FLUSH_EVERY` строк.
pub fn log_state_line(line: &str) {
    if let Ok(mut buf) = STATE_LOG_BUF.lock() {
        buf.push(line.to_string());
    }
    if STATE_LOG_COUNT.fetch_add(1, Ordering::Relaxed) % STATE_LOG_FLUSH_EVERY
        == STATE_LOG_FLUSH_EVERY - 1
    {
        flush_state_log();
    }
}

/// Сбрасывает буфер состояния в `state.log` (append).
fn flush_state_log() {
    let Ok(localappdata) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let path = format!("{}\\drmod\\state.log", localappdata);
    let Ok(mut buf) = STATE_LOG_BUF.lock() else {
        return;
    };
    if buf.is_empty() {
        return;
    }
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    for line in buf.drain(..) {
        let _ = std::io::Write::write_fmt(&mut f, format_args!("{}\r\n", line));
    }
}

/// Детур `cInput::updateInputUnit` (__cdecl). Читает реальный ввод ДО вызова
/// оригинала (unit уже заполнен реальным вводом, а оригинал сбрасывает его),
/// затем вызывает оригинал и перезаписывает unit нашими значениями, если
/// override активен. Запись/перезапись только для `user_index == 0`.
pub unsafe extern "C" fn update_input_unit_detour(unit: *mut InputUnit, user_index: i32) {
    let n = DETOUR_COUNT.fetch_add(1, Ordering::Relaxed);
    let sample = n.is_multiple_of(60);

    if user_index == 0 {
        let override_active = INPUT_OVERRIDE.lock().map(|g| g.active).unwrap_or(false);
        if !override_active {
            // Запись реального ввода ДО оригинала: здесь unit ещё содержит
            // реальный ввод (заполняется до updateInputUnit), а оригинал
            // сбрасывает его (после оригинала unit уже valid=0).
            if sample {
                let u = unsafe { &*unit };
                log_line(&format!(
                    "detour: ui={} unit=0x{:08X} before_orig down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2}) valid={}",
                    user_index,
                    unit as usize,
                    u.buttons_down,
                    u.buttons_pressed,
                    u.left_stick[0],
                    u.left_stick[1],
                    u.right_stick[0],
                    u.right_stick[1],
                    u.valid_input
                ));
            }
        }
    }

    if let Some(&orig) = ORIG_UPDATE_INPUT_UNIT.get() {
        unsafe { orig(unit, user_index) };
    }

    if sample {
        let u = unsafe { &*unit };
        log_line(&format!(
            "detour: ui={} unit=0x{:08X} after_orig down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2}) valid={}",
            user_index,
            unit as usize,
            u.buttons_down,
            u.buttons_pressed,
            u.left_stick[0],
            u.left_stick[1],
            u.right_stick[0],
            u.right_stick[1],
            u.valid_input
        ));
    }

    if user_index != 0 {
        return;
    }
    if let Ok(guard) = INPUT_OVERRIDE.lock()
        && guard.active
    {
        let u = unsafe { &mut *unit };
        *u = guard.input;
        if sample {
            log_line(&format!(
                "detour: unit=0x{:08X} AFTER  down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2}) valid={}",
                unit as usize,
                u.buttons_down,
                u.buttons_pressed,
                u.left_stick[0],
                u.left_stick[1],
                u.right_stick[0],
                u.right_stick[1],
                u.valid_input
            ));
        }
    }
}

/// Полное состояние персонажа на кадр — позиция, поворот, скорость, HP,
/// анимация, оружие и активные состояния (прыжок/атаки/ниндзя/блейд/ripper).
/// Смещения из SDK (`ref/mgr-plugin-sdk`): `cParts.h` (0x50, 0x90),
/// `BehaviorAppBase.h` (0x890, якорь HP 0x870), `Pl0000.h` (0x3184, 0x40C8).
/// Проверено рантаймом: rotation.y=head (0x90), ripper (0x3184), blade (0x40C8).
/// velocity (0x890) — только вертикальная составляющая (прыжок/гравитация),
/// x/z всегда 0: горизонтального поля скорости нет (движение кинематическое).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerState {
    /// cParts::m_vecTransPos (+0x50)
    pub pos: [f32; 3],
    /// cParts::m_vecRotation (+0x90, Euler; меняется только Y = yaw = heading)
    pub rotation: [f32; 3],
    /// BehaviorAppBase::m_vecVelocity (+0x890) — вертикальная скорость (y);
    /// x/z всегда 0 (горизонтальной скорости в этом поле нет).
    pub velocity: [f32; 3],
    /// m_nHealth (+0x870)
    pub hp: i32,
    /// m_nCurrentAction (+0x618)
    pub r_anim: i32,
    /// m_SwordState (+0x13FC)
    pub sword_state: i32,
    /// m_bSwordHidden (+0xB74)
    pub sword_hidden: i32,
    /// m_fInputDirection (+0xD2C)
    pub input_direction: f32,
    /// m_fDesiredHeading (+0xD30)
    pub desired_heading: f32,
    /// m_nButtonJump (+0xE18)
    pub button_jump: i32,
    /// m_nButtonLightAttack (+0xE20)
    pub button_light_attack: i32,
    /// m_nButtonHeavyAttack (+0xE24)
    pub button_heavy_attack: i32,
    /// m_nButtonNinjarun (+0xE48)
    pub button_ninjarun: i32,
    /// m_nButtonBlademode (+0xE50)
    pub button_blademode: i32,
    /// m_bRipperModeEnabled (+0x3184)
    pub ripper_enabled: i32,
    /// m_nBladeModeType (+0x40C8)
    pub blade_mode_type: i32,
}

/// Состояние камеры на кадр: позиция + view-proj матрица (углы извлекаются
/// офлайн из матрицы).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraState {
    /// позиция камеры (camera + 0x1B0)
    pub pos: [f32; 3],
    /// view-proj матрица (camera + 0x200)
    pub view_proj: [f32; 16],
}

/// Один кадр записи: полный InputUnit (m_CurrentInput) + полное состояние
/// персонажа и камеры + порядковый номер кадра.
/// Номер монотонно растёт от старта записи — воспроизведение подаёт кадры
/// строго по индексу (1 кадр на тик), без dt-сопоставления.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReplayFrame {
    pub frame_index: u32,
    /// m_CurrentInput (0xCF8) — копия g_InputUnit0, стабильно читается в render.
    pub input: InputUnit,
    pub state: PlayerState,
    pub camera: CameraState,
}

/// Сериализация `#[repr(C)]`-структуры в байты (для BLOB в SQLite).
/// Структуры состоят из f32/i32/u32 — без padding, round-trip корректен.
fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

use rusqlite::Connection;

/// Метаданные одного прогона записи/воспроизведения (строка в `replay_runs`).
pub struct ReplayRunMeta {
    /// "record" | "playback"
    pub kind: &'static str,
    pub mission_id: i32,
    pub mission_name: String,
    pub started_at: String,
    /// реальный elapsed от старта до стопа (мс)
    pub duration_ms: i64,
    /// для playback — id исходной записи
    pub source_replay_id: Option<i64>,
}

/// Создаёт таблицы Record/Replay (если их нет).
pub fn create_replay_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS replay_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            mission_id INTEGER NOT NULL,
            mission_name TEXT NOT NULL,
            started_at TEXT NOT NULL,
            frame_count INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            source_replay_id INTEGER
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS replay_record_frames (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            replay_id INTEGER NOT NULL,
            frame_index INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            input_unit BLOB NOT NULL,
            state BLOB NOT NULL,
            camera BLOB NOT NULL,
            FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS replay_playback_frames (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            replay_id INTEGER NOT NULL,
            frame_index INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            input_unit BLOB NOT NULL,
            state BLOB NOT NULL,
            camera BLOB NOT NULL,
            FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
        )",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_replay_record_frames ON replay_record_frames(replay_id, frame_index)",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_replay_playback_frames ON replay_playback_frames(replay_id, frame_index)",
        (),
    )?;
    Ok(())
}

/// Флашит накопленные кадры в БД одним bulk-insert: строка в `replay_runs`
/// и кадры в `replay_record_frames`/`replay_playback_frames` (по `kind`).
/// Паттерн — как `segment::finish_segment`. Возвращает id прогона.
pub fn flush_replay(conn: &Connection, meta: &ReplayRunMeta, frames: &[ReplayFrame]) -> Option<i64> {
    let frame_count = frames.len() as i64;
    let _ = conn.execute(
        "INSERT INTO replay_runs (kind, mission_id, mission_name, started_at, frame_count, duration_ms, source_replay_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            meta.kind,
            meta.mission_id,
            meta.mission_name,
            meta.started_at,
            frame_count,
            meta.duration_ms,
            meta.source_replay_id
        ],
    );
    let replay_id = conn.last_insert_rowid();
    if frames.is_empty() {
        return Some(replay_id);
    }

    let sql = match meta.kind {
        "record" => "INSERT INTO replay_record_frames (replay_id, frame_index, duration_ms, input_unit, state, camera) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        _ => "INSERT INTO replay_playback_frames (replay_id, frame_index, duration_ms, input_unit, state, camera) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    };

    let _ = conn.execute("BEGIN", []);
    let Ok(mut stmt) = conn.prepare(sql) else {
        let _ = conn.execute("ROLLBACK", []);
        return Some(replay_id);
    };
    for f in frames {
        let dur = (f.frame_index as i64) * 1000 / 60;
        let _ = stmt.execute(rusqlite::params![
            replay_id,
            f.frame_index as i64,
            dur,
            as_bytes(&f.input),
            as_bytes(&f.state),
            as_bytes(&f.camera)
        ]);
    }
    let _ = conn.execute("COMMIT", []);
    Some(replay_id)
}
