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
            #[cfg(debug_assertions)]
            record_bare_frame(unsafe { &*unit });
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

/// Один кадр записи: полный InputUnit + таймстамп от старта записи.
#[cfg(debug_assertions)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReplayFrame {
    pub duration_ms: i64,
    pub input: InputUnit,
}

/// Короткая запись по нумпаду (smoke-тест InputUnit, Этап 1.5) — без сегментов
/// и без SQLite. Кадры пишутся в память, воспроизводятся через `set_input_override`.
#[cfg(debug_assertions)]
struct BareRecording {
    active: bool,
    start: Option<Instant>,
    frames: Vec<ReplayFrame>,
}

#[cfg(debug_assertions)]
static BARE_RECORDING: Mutex<BareRecording> = Mutex::new(BareRecording {
    active: false,
    start: None,
    frames: Vec::new(),
});

/// Начинает короткую запись (NumPad5). Очищает буфер и фиксирует момент старта.
#[cfg(debug_assertions)]
pub fn start_bare_recording() {
    let Ok(mut st) = BARE_RECORDING.lock() else {
        return;
    };
    st.active = true;
    st.start = Some(Instant::now());
    st.frames.clear();
}

/// Останавливает короткую запись и возвращает накопленные кадры.
/// Возвращает `None`, если запись не была активна.
#[cfg(debug_assertions)]
pub fn stop_bare_recording() -> Option<Vec<ReplayFrame>> {
    let Ok(mut st) = BARE_RECORDING.lock() else {
        return None;
    };
    if !st.active {
        return None;
    }
    st.active = false;
    Some(std::mem::take(&mut st.frames))
}

/// Активна ли короткая запись.
#[cfg(debug_assertions)]
pub fn is_bare_recording() -> bool {
    BARE_RECORDING.lock().map(|g| g.active).unwrap_or(false)
}

/// Число накопленных кадров в буфере короткой записи.
#[cfg(debug_assertions)]
pub fn bare_recording_frame_count() -> usize {
    BARE_RECORDING.lock().map(|g| g.frames.len()).unwrap_or(0)
}

/// Пушит один кадр реального ввода в буфер короткой записи.
/// Вызывается из детура ДО вызова оригинала (unit ещё содержит реальный ввод).
#[cfg(debug_assertions)]
fn record_bare_frame(unit: &InputUnit) {
    let Ok(mut st) = BARE_RECORDING.lock() else {
        return;
    };
    if !st.active {
        return;
    }
    let duration_ms = st
        .start
        .map(|t| t.elapsed().as_millis() as i64)
        .unwrap_or(0);
    st.frames.push(ReplayFrame {
        duration_ms,
        input: *unit,
    });
}
