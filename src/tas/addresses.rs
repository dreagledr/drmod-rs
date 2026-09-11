//! Адреса и константы системы ввода (относительно базы модуля игры).
//!
//! `pub(super)` — используются только внутри `tas/` (hooks/replay);
//! `pub(crate)` — нужны также `lib.rs` (чтение полей Pl0000, инжекция ввода).

/// cInput::ms_KeyInput — сырой ввод клавиатуры (вспомогательный кэш,
/// игроком для движения не читается). Только для `hooks`.
pub(super) const KEY_INPUT: usize = 0x177B7C0;
/// cInput::ms_InputKeys (char[256]) — сырое состояние клавиш из DirectInput
/// (индекс — DIK-код). Игра маппит его в `ms_KeyInput` (игровые коды).
pub(crate) const INPUT_KEYS: usize = 0x19D06F8;
/// cInput::ms_bUpdateKeyboard — флаг автообновления кэша клавиш из DirectInput.
/// Замораживается на время подачи raw-клавиш меню (см. `hooks::set_raw_key`).
pub(super) const UPDATE_KEYBOARD: usize = 0x14CDDE8;
/// Функция опроса клавиатуры: `Acquire` + `IDirectInputDevice8::GetDeviceState(
/// 0x100, ms_InputKeys)` (vtable+0x24). Игра зовёт её только когда окно в
/// фокусе, поэтому `ms_InputKeys`/`ms_KeyInput` — производные и перезаписываются
/// каждый кадр. Хук этой функции + подмешивание наших DIK-битов после
/// оригинала даёт игре «настоящее» нажатие (меню читает именно этот путь).
pub(crate) const KEYBOARD_POLL: usize = 0x9D9670;
/// Коды клавиш DirectInput (DIK) для меню: мод подмешивает их в
/// `ms_InputKeys` после опроса устройства (`hooks::set_dik_mask`).
pub(crate) const DIK_UP: u32 = 0xC8;
pub(crate) const DIK_DOWN: u32 = 0xD0;
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

/// Игровые коды клавиш (сырой ввод `ms_KeyInput`, см. docs/REPLAY.md §2.1).
/// Меню читает их через `KeyInput::isKeyDown`/`isKeyPressed` (0x9D93A0/0x9D9400),
/// а не через keybind'ы. Коды сверены с дизассемблированием функции 0x8AC570
/// (маппинг битов InputUnit на isKeyDown, 2026-08-19):
/// бит 0x01→0x8D, 0x02→0x8E, 0x04→0x8C, 0x08→0x8F, 0x400→0x92, 0x2000→0x93.
/// Примечание (2026-08-19): навигация в меню работает напрямую через D-Pad
/// биты InputUnit (0x1/0x2/0x4/0x8) — хук isKeyDown не требуется для
/// menu_*/confirm. Константы оставлены для reference и raw-клавиш
/// (codec/pause через цифру 3 / Esc).
#[allow(dead_code)]
pub(crate) const KEY_ENTER: u8 = 0x8C;
#[allow(dead_code)]
pub(crate) const KEY_ESC: u8 = 0x8E;
#[allow(dead_code)]
pub(crate) const KEY_UP: u8 = 0x90;
#[allow(dead_code)]
pub(crate) const KEY_RIGHT: u8 = 0x91;
#[allow(dead_code)]
pub(crate) const KEY_LEFT: u8 = 0x92;
#[allow(dead_code)]
pub(crate) const KEY_DOWN: u8 = 0x93;
/// Клавиша weapon_select (открытие меню оружия) — игровой код 0x8D.
/// Из дизассемблирования функции 0x8AC570: бит 0x01 в InputUnit маппится
/// на `isKeyDown(0x8D)`. Это НЕ клавиша "2" (0x2D), а кнопка геймпада
/// (DPAD_LEFT), которая на геймпаде открывает weapon select.
#[allow(dead_code)]
pub(crate) const KEY_WEAPON_SELECT: u8 = 0x8D;
/// Клавиши D-pad-действий из дизассемблирования 0x8AC570 (бит InputUnit →
/// код клавиши, которую игра проверяет через `isKeyDown`): DPAD_DOWN 0x04 →
/// 0x8C, DPAD_RIGHT 0x02 → 0x8E (он же Esc), DPAD_UP 0x08 → 0x8F (он же
/// codec), DPAD_LEFT 0x01 → 0x8D (weapon select).
#[allow(dead_code)]
pub(crate) const KEY_DPAD_DOWN: u8 = 0x8C;
#[allow(dead_code)]
pub(crate) const KEY_DPAD_RIGHT_ESC: u8 = 0x8E;
/// Клавиша codec (предположительно) — игровой код 0x8F.
#[allow(dead_code)]
pub(crate) const KEY_CODEC: u8 = 0x8F;
/// Игровые коды цифр 1/2/3 (`VK ^ 0x1F`, см. docs/REPLAY.md §2.1):
/// 1 → 0x2E, 2 → 0x2D, 3 → 0x2C. Кодек игра читает как сырую клавишу
/// (keybind-эмуляция не срабатывает, проверено 2026-08-18).
/// AR-режим (1) идёт через бит InputUnit `0x08` (`input_bits::AR_MODE`),
/// меню оружия (2) — через прямую запись GameMenuStatus (см. api.rs).
#[allow(dead_code)]
pub(crate) const KEY_DIGIT1: u8 = 0x2E;
#[allow(dead_code)]
pub(crate) const KEY_DIGIT2: u8 = 0x2D;
#[allow(dead_code)]
pub(crate) const KEY_DIGIT3: u8 = 0x2C;
/// cInput::updateInputUnit(InputUnit*, int userIndex) — функция, которую игра
/// вызывает каждый тик для заполнения глобального InputUnit из DirectInput.
/// Хук перехватывает её и перезаписывает unit[0] после вызова оригинала.
pub(super) const UPDATE_INPUT_UNIT: usize = 0x9DAFE0;
/// Pl0000::m_CurrentInput — копия `g_InputUnit0` (смещение от объекта Pl0000).
pub(crate) const CURRENT_INPUT_OFFSET: usize = 0xCF8;
/// `GameMenuStatus` (enum 0–18, см. `game::GameMenuStatus`): 1 = InGame,
/// 3 = PauseMenu, 9 = SelectWeaponMenu. Игра перестаёт обновлять InputUnit,
/// когда статус != 1 (в паузе) — используется для диагностики меню.
pub(crate) const GAME_MENU_STATUS: usize = 0x17E9F9C;
/// Глобальный InputUnit[0] (cInput) — реальный источник входа игрока.
/// Pl0000::updateInput копирует его в m_CurrentInput (гипотеза №1 FINDINGS).
pub(crate) const GLOBAL_INPUT_UNIT0: usize = 0x177B850;

/// Биты действий в `InputUnit.buttons_down`/`buttons_pressed` — общие с
/// инструментами (dbdump декодирует записи в скрипты API), определены в
/// `drmod-replay-types` (единый источник истины).
pub(crate) use drmod_replay_types::input_bits;

/// Pl0000::m_fInputDirection — направление ввода (спроецировано на камеру).
pub(crate) const PL_INPUT_DIR: usize = 0xD2C;
/// Pl0000::m_nButtonJump — прыжок.
pub(crate) const PL_BUTTON_JUMP: usize = 0xE18;