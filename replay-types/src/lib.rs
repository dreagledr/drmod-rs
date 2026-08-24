//! DTO-типы снапшотов игры (ввод/состояние/камера) и их байт-конвертация.
//!
//! Разделяются между модом (`src/tas`) и инструментами (`tools/dbdump`):
//! `#[repr(C)]`-layout этих структур — это on-disk формат BLOB в таблицах
//! `replay_*_frames` (SQLite), поэтому определение живёт в одном месте, а
//! запись (`to_bytes`) и чтение (`from_bytes`) используют его напрямую.

use serde::Serialize;

/// Нормализованный ввод игрока (cInput::InputUnit).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
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

/// Полное состояние персонажа на кадр — позиция, поворот, скорость, HP,
/// анимация, оружие и активные состояния (прыжок/атаки/ниндзя/блейд/ripper).
/// Смещения из SDK (`ref/mgr-plugin-sdk`): `cParts.h` (0x50, 0x90),
/// `BehaviorAppBase.h` (0x890, якорь HP 0x870), `Pl0000.h` (0x3184, 0x40C8).
/// Проверено рантаймом: rotation.y=head (0x90), ripper (0x3184), blade (0x40C8).
/// velocity (0x890) — только вертикальная составляющая (прыжок/гравитация),
/// x/z всегда 0: горизонтального поля скорости нет (движение кинематическое).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
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

/// Состояние камеры на кадр: позиция, look-at точка, крен и view-proj матрица
/// (углы yaw/pitch выводятся из pos→lookAt офлайн или в API).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct CameraState {
    /// позиция камеры (cCameraBase.m_CameraMatrix.m_vecPosition, +0x1B0)
    pub pos: [f32; 3],
    /// точка, куда камера смотрит (m_vecLookAtPosition, +0x1C0) — из pos→lookAt
    /// вычисляются yaw/pitch (наклон камеры)
    pub look_at: [f32; 3],
    /// крен камеры (m_fRoll, +0x1F0)
    pub roll: f32,
    /// view-proj матрица (cCameraViewProj.m_viewProjectionMatrix, +0x200)
    pub view_proj: [f32; 16],
}

/// Состояние ближайшего к игроку врага на кадр (для сопоставления air-атаки
/// игрока с позицией/анимацией врага офлайн). `found == 0` — врага нет
/// (не бой, loading, список пуст) — остальные поля нулевые.
/// Смещения — как у игрока (Behavior): позиция +0x50, HP +0x870, r_anim +0x618,
/// кадр анимации +0x8B4; blade_y — мировая высота клинка (матрица части
/// Em0010_Blade, +0x44). Подробности: docs/ENEMY_TRACKING.md.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct EnemyState {
    /// мировая позиция врага (Behavior::m_vecTransPos, +0x50/0x54/0x58)
    pub pos: [f32; 3],
    /// мировая высота клинка (Em0010_Blade, матрица cParts m[3].y = +0x44)
    pub blade_y: f32,
    /// текущая анимация врага (+0x618; 65545 — «прыжок», 19 — выпад)
    pub r_anim: i32,
    /// кадр анимации (+0x8B4, растёт 0..N и сбрасывается)
    pub frame: i32,
    /// HP врага (+0x870)
    pub hp: i32,
    /// 1 — враг найден и записан, 0 — врага нет
    pub found: i32,
}

/// Сериализация `#[repr(C)]`-структуры в байты (для BLOB в SQLite).
/// Структуры состоят из f32/i32/u32 — без padding, round-trip корректен.
pub fn to_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

/// Обратная операция: байты BLOB → структура. Длина проверяется; выравнивание
/// после SQLite-транспорта не гарантировано, поэтому `read_unaligned`.
pub fn from_bytes<T: Copy>(bytes: &[u8]) -> Option<T> {
    if bytes.len() != std::mem::size_of::<T>() {
        return None;
    }
    unsafe { Some(std::ptr::read_unaligned(bytes.as_ptr() as *const T)) }
}
