//! Типы данных (DTO) системы ввода и записи/воспроизведения —
//! снимки памяти игры и кадры записи.

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