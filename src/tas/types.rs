//! Типы данных (DTO) системы ввода и записи/воспроизведения —
//! снимки памяти игры и кадры записи.

pub use drmod_replay_types::{CameraState, InputUnit, PlayerState};

/// Сырой ввод клавиатуры (cInput::KeyInput).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct KeyInput {
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

/// Снимок сырого состояния мыши (cInput::MouseInput) — зажатые кнопки.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MouseState {
    /// +0x00 m_nMouseButtons — зажатые кнопки (битовая маска)
    pub buttons: i32,
}

/// Снимок ввода игрока (Pl0000) — направление и кнопка прыжка по
/// подтверждённым SDK-смещениям.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlInputSnapshot {
    /// m_fInputDirection (0xD2C)
    pub input_direction: f32,
    /// m_nButtonJump (0xE18)
    pub button_jump: i32,
}

/// Значения, подменяющие ввод игрока в хуке `updateInputUnit`.
/// `active = false` — реальный ввод проходит без изменений.
/// `input` — полный InputUnit, который записывается в `g_InputUnit0`.
#[derive(Clone, Copy, Debug, Default)]
pub struct InputOverride {
    pub active: bool,
    pub input: InputUnit,
}

/// Один кадр записи: полный InputUnit (m_CurrentInput) + полное состояние
/// персонажа и камеры + порядковый номер кадра + реальные флаги blade/ripper.
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
    /// Реальное удержание blade mode (keybind 8) в этом кадре — сэмпл детура
    /// `isKeybindDown`, а не реконструкция из `blade_mode_type`.
    pub blade_down: u8,
    /// Реальный фронт ripper (keybind 11) в этом кадре — сэмпл детура
    /// `isKeybindPressed`, а не перепад `ripper_enabled`.
    pub ripper_pressed: u8,
    /// Сырые клавиши (m_aKeysDown, m_aKeysPressed из ms_KeyInput) в этом кадре.
    /// Меню (weapon_select/pause/...) читает стрелки/Enter/Esc через
    /// `isKeyDown`/`isKeyPressed` (0x9D93A0/0x9D9400) из этого кэша, а НЕ из
    /// InputUnit — без захвата навигация по меню не воспроизводится.
    pub raw_down: [u32; 6],
    pub raw_pressed: [u32; 6],
}

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