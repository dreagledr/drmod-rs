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

/// Кодировка игровых кодов клавиш в словах `m_aKeysDown`/`m_aKeysPressed`
/// (`cInput::ms_KeyInput`). Единый источник истины для мода (`src/tas`) и
/// инструментов (`tools/dbdump`).
pub mod key_codes {
    /// Индекс слова: `code >> 5` (6 слов = 256 кодов).
    pub fn index(code: u32) -> usize {
        (code >> 5) as usize
    }

    /// Бит кода внутри слова. Порядок бит ОБРАТНЫЙ: старший бит — код 0
    /// (`0x8000_0000 >> (code & 31)`). Проверено live 2026-09-10: нажатие
    /// стрелки вниз (код 0x8C) меняет `down[4]` в `0x0008_0000`
    /// (`0x8000_0000 >> 12`), а не `1 << 12`.
    pub fn bit(code: u32) -> u32 {
        0x8000_0000u32 >> (code & 31)
    }
}

/// Биты действий в `InputUnit.buttons_down`/`buttons_pressed` (эмпирически,
/// подтверждено сопоставлением с сырыми клавишами/мышью в debug-логе).
/// Общие для мода (`src/tas/addresses.rs` — re-export) и инструментов
/// (`tools/dbdump` — декодирование записей в скрипты API).
pub mod input_bits {
    /// Прыжок (Space) — бит 0x10 (эмпирически, 2026-08-18: ручной прыжок даёт
    /// `cur_in down=00000010 pressed=00000010` + `space=true`, y поднимается;
    /// удержание держит down). Бит 0x1 — кнопка меню выбора оружия (открывает
    /// меню и навигирует по слотам), НЕ прыжок.
    pub const JUMP: u32 = 0x0000_0010;
    /// AR-режим (клавиша 1) — бит 0x08 в InputUnit (эмпирически, 2026-08-19).
    /// Игра кодирует AR-режим этим битом; raw-подача через кэш `ms_KeyInput`
    /// не работает (§10.3) — механизм как у прыжка: бит + фронт pressed.
    pub const AR_MODE: u32 = 0x0000_0008;
    /// Меню выбора оружия (клавиша 2) — бит 0x1 в InputUnit (эмпирически,
    /// 2026-08-19). Игра кодирует меню оружия этим битом (тот, что раньше
    /// ошибочно считался прыжком, §10.1); raw-подача через кэш `ms_KeyInput`
    /// не работает (§10.3) — механизм как у прыжка: бит + фронт pressed.
    /// Примечание (2026-08-19): меню теперь открывается прямой записью
    /// GameMenuStatus (см. api.rs) — бит 0x1 остаётся как reference.
    pub const WEAPON_SELECT: u32 = 0x0000_0001;
    /// Навигация в меню — D-Pad биты геймпада (`eInputButton` из SDK):
    /// DPAD_LEFT=0x1, DPAD_RIGHT=0x2, DPAD_DOWN=0x4, DPAD_UP=0x8.
    /// Проверено live (2026-08-19): подача бита 0x1 в открытом меню оружия
    /// двигает выбор. В геймплее эти же биты — кнопки D-Pad (0x1 = weapon
    /// select, 0x8 = AR mode), поэтому меню нужно открывать отдельно (прямая
    /// запись GameMenuStatus), а навигацию подавать после открытия.
    pub const MENU_LEFT: u32 = 0x0000_0001;
    pub const MENU_RIGHT: u32 = 0x0000_0002;
    pub const MENU_DOWN: u32 = 0x0000_0004;
    pub const MENU_UP: u32 = 0x0000_0008;
    /// Подтверждение в меню — BUTTON_A (0x10) геймпада. В геймплее A = jump,
    /// в меню A = confirm (SDK `eInputButton`). Тот же бит, что JUMP.
    pub const CONFIRM: u32 = 0x0000_0010;
    /// Отмена/назад в меню — BUTTON_B (0x20) геймпада. В геймплее B = лёгкая
    /// атака, в меню B = отмена (SDK `eInputButton`).
    pub const CANCEL: u32 = 0x0000_0020;
    /// Пауза/меню — реальный Esc кодируется этим битом (эмпирически,
    /// 2026-09-10, `debug.log` P118_BEACH: кадр смены `status_raw 1→3`
    /// (InGame→PauseMenu) имеет `cur_in down=00000100 pressed=00000100`, т.е.
    /// Esc = START-бит геймпада, а не BUTTON_B 0x20). Механизм как у `jump`:
    /// бит + фронт `pressed` на первом кадре команды.
    pub const PAUSE: u32 = 0x0000_0100;
    /// Лёгкая атака (ЛКМ)
    pub const LIGHT_ATTACK: u32 = 0x0000_0040;
    /// Тяжёлая атака (ПКМ)
    pub const HEAVY_ATTACK: u32 = 0x0000_0080;
    /// Движение вперёд (W) — сопутствует left_stick=(0,-1000)
    pub const FORWARD: u32 = 0x0040_0000;
    /// Движение назад (S) — left_stick=(0,1000). Подтверждено логом (2026-08-18):
    /// `cur_in down=00800000` при S.
    pub const BACK: u32 = 0x0080_0000;
    /// Движение влево (A) — left_stick=(-1000,0). Подтверждено логом:
    /// `cur_in down=00200000` при A.
    pub const LEFT: u32 = 0x0020_0000;
    /// Движение вправо (D) — left_stick=(1000,0). Подтверждено логом:
    /// `cur_in down=00100000` при D.
    pub const RIGHT: u32 = 0x0010_0000;
    /// Ninja run (LCtrl) — бит в InputUnit, сопутствует FORWARD при удержании
    /// LCtrl (лог: `cur_in down=00404000`). Сам keybind — KEYBIND_NINJARUN (9),
    /// подаётся через isKeybindDown; бит нужен только для декодирования логов.
    pub const NINJA_RUN: u32 = 0x0000_4000;
    /// Blade mode — бит в InputUnit (эмпирически, 2026-08-18: запись 33 в БД —
    /// реальное удержание клавиши блейда даёт `down=00400800` + фронт
    /// `pressed=00000800` на 1-м кадре). Игра кодирует блейд этим битом
    /// (как ninja_run — 0x4000); keybind-эмуляция (isKeybindDown(8)) НЕ
    /// работает для скриптов — игра читает её только в key-event обработке.
    pub const BLADE: u32 = 0x0000_0800;
    /// Subweapon (C) — бит 0x400 в InputUnit (API.md §4.2: игра кодирует
    /// под-оружие этим битом, фронт `pressed=0x400` на 1-м кадре).
    pub const SUBWEAPON: u32 = 0x0000_0400;
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
