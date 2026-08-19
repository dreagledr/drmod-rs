//! Адреса и константы системы ввода (относительно базы модуля игры).
//!
//! `pub(super)` — используются только внутри `tas/` (hooks/replay);
//! `pub(crate)` — нужны также `lib.rs` (чтение полей Pl0000, инжекция ввода).

/// cInput::ms_KeyInput — сырой ввод клавиатуры (вспомогательный кэш,
/// игроком для движения не читается). Только для `hooks`.
pub(super) const KEY_INPUT: usize = 0x177B7C0;
/// cInput::ms_bUpdateKeyboard — флаг автообновления кэша клавиш из DirectInput.
/// Замораживается на время подачи raw-клавиш меню (см. `hooks::set_raw_key`).
pub(super) const UPDATE_KEYBOARD: usize = 0x14CDDE8;
/// cInput::ms_MouseInput — сырой ввод мыши (вспомогательный кэш).
pub(super) const MOUSE_INPUT: usize = 0x177B798;
/// cInput::ms_aControllers — массив ControllerState[4] (XInput-кэш).
#[allow(dead_code)]
pub(super) const CONTROLLERS: usize = 0x19D05F0;
/// cInput::isKeybindDown(eSaveKeybind) — проверка удержания keybind (hold,
/// для blade mode). Активация ripper её НЕ использует.
pub(super) const IS_KEYBIND_DOWN: usize = 0x61D280;
/// cInput::isKeybindPressed(eSaveKeybind) — проверка фронта нажатия keybind
/// (для toggle-действий: ripper). Активация ripper использует именно её:
/// в дизассемблере `push 0x0B; call 0x61D2D0`.
pub(super) const IS_KEYBIND_PRESSED: usize = 0x61D2D0;
/// `KeyInput::isKeyDown(int vKey)` — проверка удержания сырой клавиши
/// (thiscall, читает `ms_KeyInput.m_aKeysDown`). Меню читает стрелки/Enter/Esc
/// через неё, а не через keybind'ы. Хук: детур сам возвращает 1 для
/// эмулируемых клавиш (оригинал не вызываем — обходим thiscall).
pub(super) const IS_KEY_DOWN: usize = 0x9D93A0;
/// `KeyInput::isKeyPressed(int vKey)` — проверка фронта сырой клавиши
/// (thiscall, читает `ms_KeyInput.m_aKeysPressed`).
pub(super) const IS_KEY_PRESSED: usize = 0x9D9400;
/// eSaveKeybind — индексы в enum (см. `ref/mgr-plugin-sdk/game/Hw.h`).
/// Дизассемблирование (2026-08-18): все действия, кроме ripper, активируются
/// через `isKeybindDown` (0x61D280); `isKeybindPressed` (0x61D2D0) вызывается
/// только для RIPPERMODE (call sites 0x810599/0x8106AF).
/// Неиспользуемые константы — справочные (движение/атаки идут через
/// InputUnit-биты, а не через keybind'ы; прыжок — известная проблема §10.1).
#[allow(dead_code)]
pub(crate) const KEYBIND_FORWARD: i32 = 0;
#[allow(dead_code)]
pub(crate) const KEYBIND_BACK: i32 = 1;
#[allow(dead_code)]
pub(crate) const KEYBIND_LEFT: i32 = 2;
#[allow(dead_code)]
pub(crate) const KEYBIND_RIGHT: i32 = 3;
/// Walk (Tab) — ⚠️ игра НЕ читает через isKeybindDown (дизассемблирование
/// 2026-08-18: call sites только 1/2/3/9/20 + цикл 5..22). Ходьба кодируется
/// магнитудой стика (×0.5) — см. `script_tick` в api.rs. Справочная.
#[allow(dead_code)]
pub(crate) const KEYBIND_WALK: i32 = 4;
#[allow(dead_code)]
pub(crate) const KEYBIND_JUMP: i32 = 5;
#[allow(dead_code)]
pub(crate) const KEYBIND_LIGHT_ATTACK: i32 = 6;
#[allow(dead_code)]
pub(crate) const KEYBIND_HEAVY_ATTACK: i32 = 7;
pub(crate) const KEYBIND_BLADEMODE: i32 = 8;
pub(crate) const KEYBIND_NINJARUN: i32 = 9;
#[allow(dead_code)]
pub(crate) const KEYBIND_ACTION: i32 = 10;
pub(crate) const KEYBIND_RIPPERMODE: i32 = 11;
pub(crate) const KEYBIND_SWITCH_LOCK_ON: i32 = 12;
pub(crate) const KEYBIND_USE_SUBWEAPON: i32 = 13;
pub(crate) const KEYBIND_USE_ITEM: i32 = 14;
#[allow(dead_code)]
pub(crate) const KEYBIND_AR_MODE: i32 = 15;
#[allow(dead_code)]
pub(crate) const KEYBIND_WEAPON_SELECT_SCREEN: i32 = 16;
#[allow(dead_code)]
pub(crate) const KEYBIND_CODEC_SCREEN: i32 = 17;
#[allow(dead_code)]
pub(crate) const KEYBIND_PAUSE: i32 = 18;
pub(crate) const KEYBIND_CAMERA_RESET: i32 = 19;
pub(crate) const KEYBIND_EXECUTION: i32 = 20;
pub(crate) const KEYBIND_DEFFENSIVE_OFFENSIVE: i32 = 21;
#[allow(dead_code)]
pub(crate) const KEYBIND_FIRE_SUBWEAPON: i32 = 22;
/// Количество keybind'ов (KEYBIND_TOTAL).
pub(crate) const KEYBIND_TOTAL: usize = 23;

/// Игровые коды клавиш меню (сырой ввод `ms_KeyInput`, см. docs/REPLAY.md §2.1).
/// Меню читает их через `KeyInput::isKeyDown`/`isKeyPressed` (0x9D93A0/0x9D9400),
/// а не через keybind'ы — для подачи нужна запись в кэш `ms_KeyInput`.
pub(crate) const KEY_ENTER: u8 = 0x15;
pub(crate) const KEY_ESC: u8 = 0x8E;
pub(crate) const KEY_UP: u8 = 0x90;
pub(crate) const KEY_RIGHT: u8 = 0x91;
pub(crate) const KEY_LEFT: u8 = 0x92;
pub(crate) const KEY_DOWN: u8 = 0x93;
/// Игровые коды цифр 1/2/3 (`VK ^ 0x1F`, см. docs/REPLAY.md §2.1):
/// 1 → 0x2E, 2 → 0x2D, 3 → 0x2C. Меню оружия/кодек игра читает как сырые
/// клавиши (keybind-эмуляция не срабатывает, проверено 2026-08-18).
/// AR-режим (1) теперь идёт через бит InputUnit `0x08` (см. `input_bits::AR_MODE`).
#[allow(dead_code)]
pub(crate) const KEY_DIGIT1: u8 = 0x2E;
pub(crate) const KEY_DIGIT2: u8 = 0x2D;
pub(crate) const KEY_DIGIT3: u8 = 0x2C;
/// cInput::updateInputUnit(InputUnit*, int userIndex) — функция, которую игра
/// вызывает каждый тик для заполнения глобального InputUnit из DirectInput.
/// Хук перехватывает её и перезаписывает unit[0] после вызова оригинала.
pub(super) const UPDATE_INPUT_UNIT: usize = 0x9DAFE0;
/// Pl0000::m_CurrentInput — копия `g_InputUnit0` (смещение от объекта Pl0000).
pub(crate) const CURRENT_INPUT_OFFSET: usize = 0xCF8;
/// Глобальный InputUnit[0] (cInput) — реальный источник входа игрока.
/// Pl0000::updateInput копирует его в m_CurrentInput (гипотеза №1 FINDINGS).
pub(crate) const GLOBAL_INPUT_UNIT0: usize = 0x177B850;

/// Биты действий в `InputUnit.buttons_down`/`buttons_pressed` (эмпирически,
/// подтверждено сопоставлением с сырыми клавишами/мышью в debug-логе).
pub(crate) mod input_bits {
    /// Прыжок (Space) — бит 0x10 (эмпирически, 2026-08-18: ручной прыжок даёт
    /// `cur_in down=00000010 pressed=00000010` + `space=true`, y поднимается;
    /// удержание держит down). Бит 0x1 — кнопка меню выбора оружия (открывает
    /// меню и навигирует по слотам), НЕ прыжок.
    pub const JUMP: u32 = 0x0000_0010;
    /// AR-режим (клавиша 1) — бит 0x08 в InputUnit (эмпирически, 2026-08-19).
    /// Игра кодирует AR-режим этим битом; raw-подача через кэш `ms_KeyInput`
    /// не работает (§10.3) — механизм как у прыжка: бит + фронт pressed.
    pub const AR_MODE: u32 = 0x0000_0008;
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
}

/// Pl0000::m_fInputDirection — направление ввода (спроецировано на камеру).
pub(crate) const PL_INPUT_DIR: usize = 0xD2C;
/// Pl0000::m_nButtonJump — прыжок.
pub(crate) const PL_BUTTON_JUMP: usize = 0xE18;