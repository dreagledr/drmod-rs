//! Адреса и константы системы ввода (относительно базы модуля игры).
//!
//! `pub(super)` — используются только внутри `tas/` (hooks/replay);
//! `pub(crate)` — нужны также `lib.rs` (чтение полей Pl0000, инжекция ввода).

/// cInput::ms_KeyInput — сырой ввод клавиатуры (вспомогательный кэш,
/// игроком для движения не читается). Только для `hooks`.
pub(super) const KEY_INPUT: usize = 0x177B7C0;
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
/// eSaveKeybind::KEYBIND_RIPPERMODE (индекс в enum, см. Hw.h).
pub(super) const KEYBIND_RIPPERMODE: i32 = 11;
/// eSaveKeybind::KEYBIND_BLADEMODE.
pub(super) const KEYBIND_BLADEMODE: i32 = 8;
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
    /// Прыжок (Space)
    pub const JUMP: u32 = 0x0000_0001;
    /// Лёгкая атака (ЛКМ)
    pub const LIGHT_ATTACK: u32 = 0x0000_0040;
    /// Тяжёлая атака (ПКМ)
    pub const HEAVY_ATTACK: u32 = 0x0000_0080;
    /// Движение вперёд (W) — сопутствует left_stick=(0,-1000)
    pub const FORWARD: u32 = 0x0040_0000;
}

/// Pl0000::m_fInputDirection — направление ввода (спроецировано на камеру).
pub(crate) const PL_INPUT_DIR: usize = 0xD2C;
/// Pl0000::m_nButtonJump — прыжок.
pub(crate) const PL_BUTTON_JUMP: usize = 0xE18;