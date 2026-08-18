//! Хуки ввода (MinHook): подача InputUnit через `updateInputUnit`,
//! эмуляция ripper/blade через `isKeybindPressed`/`isKeybindDown`,
//! чтение сырого ввода (клавиатура/мышь).

use hudhook::mh::{MH_ApplyQueued, MhHook};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;

use super::addresses;
use super::replay;
use super::types;
use crate::logger;

/// cInput::ms_KeyInput — адрес сырого ввода клавиатуры (вычисляется в `install`).
static KEY_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// cInput::ms_MouseInput — адрес сырого ввода мыши (вычисляется в `install`).
static MOUSE_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// cInput::ms_bUpdateKeyboard — флаг автообновления кэша клавиш из DirectInput.
/// Замораживается на время подачи raw-клавиш меню (иначе DirectInput
/// перезапишет наши значения в `ms_KeyInput`).
static UPDATE_KEYBOARD_ADDR: AtomicUsize = AtomicUsize::new(0);
/// Trampoline оригинальной `cInput::updateInputUnit` (устанавливается в `create_input_hook`).
static ORIG_UPDATE_INPUT_UNIT: OnceLock<unsafe extern "C" fn(*mut types::InputUnit, i32)> =
    OnceLock::new();
/// Trampoline оригинальной `cInput::isKeybindPressed`.
static ORIG_IS_KEYBIND_PRESSED: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
/// Trampoline оригинальной `cInput::isKeybindDown`.
static ORIG_IS_KEYBIND_DOWN: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
/// Эмуляция удержания keybind'ов (`isKeybindDown`): `[keybind] != 0` — детур
/// возвращает 1 для этого keybind. Индексы — `addresses::KEYBIND_*`.
static KEYBIND_HOLD: [AtomicU32; addresses::KEYBIND_TOTAL] =
    [const { AtomicU32::new(0) }; addresses::KEYBIND_TOTAL];
/// Эмуляция фронта keybind'ов (`isKeybindPressed`): `[keybind] != 0` — детур
/// возвращает 1. Флаг НЕ декрементируется на каждый вызов (игра вызывает
/// `isKeybindPressed(11)` спорадически — на разных кадрах, и счётчик
/// «растекался» на несколько переключений ripper); живёт ровно один игровой
/// тик: ставится в `script_tick` (render K), сбрасывается в начале следующего
/// `script_tick` (render K+1) или при остановке скрипта.
static KEYBIND_PRESSED: [AtomicU32; addresses::KEYBIND_TOTAL] =
    [const { AtomicU32::new(0) }; addresses::KEYBIND_TOTAL];
/// Эмуляция raw-клавиш меню (`ms_KeyInput.m_aKeysDown`): битмаски по индексам
/// 0..6. Меню читает стрелки/Enter через `isKeyDown`/`isKeyPressed`, а не
/// через keybind'ы — подача идёт записью в кэш `ms_KeyInput` (см. `apply_raw_keys`).
static RAW_KEYS_DOWN: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
/// Эмуляция raw-клавиш меню (`ms_KeyInput.m_aKeysPressed`).
static RAW_KEYS_PRESSED: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
/// Активна ли подача raw-клавиш: кэш `ms_KeyInput` заморожен
/// (`ms_bUpdateKeyboard = false`) и перезаписывается нашими битмасками.
static RAW_KEYS_ACTIVE: AtomicU32 = AtomicU32::new(0);
/// Сэмпл реального удержания blade (keybind 8) за кадр — результат оригинала
/// `isKeybindDown`, накопленный детуром. Читается записью в render.
#[cfg(debug_assertions)]
static BLADE_DOWN_SAMPLED: AtomicU32 = AtomicU32::new(0);
/// Сэмпл реального фронта ripper (keybind 11) за кадр — результат оригинала
/// `isKeybindPressed`, накопленный детуром. Читается записью в render.
#[cfg(debug_assertions)]
static RIPPER_PRESSED_SAMPLED: AtomicU32 = AtomicU32::new(0);

/// Взводит эмуляцию фронта keybind'а — детур `isKeybindPressed` возвращает 1,
/// пока флаг не сброшен (toggle-действия: ripper). Сброс — `script_tick`
/// (начало следующего render-кадра) или `clear_keybind_emulation`.
pub(crate) fn set_keybind_pressed(keybind: i32, n: u32) {
    if (0..addresses::KEYBIND_TOTAL as i32).contains(&keybind) {
        KEYBIND_PRESSED[keybind as usize].store(if n == 0 { 0 } else { 1 }, Ordering::Relaxed);
    }
}

/// Взводит/снимает эмуляцию удержания keybind'а — детур `isKeybindDown`
/// возвращает 1 для этого keybind'а (hold-действия: blade, ninja run, walk,
/// dodge).
pub(crate) fn set_keybind_hold(keybind: i32, on: bool) {
    if (0..addresses::KEYBIND_TOTAL as i32).contains(&keybind) {
        KEYBIND_HOLD[keybind as usize].store(if on { 1 } else { 0 }, Ordering::Relaxed);
    }
}

/// Взводит эмуляцию клавиши R (ripper) на `n` кадров — сырой ввод, который
/// читается `isKeybindDown(KEYBIND_RIPPERMODE)`, а не `InputUnit`.
pub(crate) fn set_ripper_frames(n: u32) {
    set_keybind_pressed(addresses::KEYBIND_RIPPERMODE, n);
}

/// Взводит/снимает удержание blade mode (isKeybindDown, hold-действие).
pub(crate) fn set_blade_hold(on: bool) {
    set_keybind_hold(addresses::KEYBIND_BLADEMODE, on);
}

/// Удерживается ли blade mode сейчас (для debug-панели).
pub(crate) fn blade_hold() -> bool {
    KEYBIND_HOLD[addresses::KEYBIND_BLADEMODE as usize].load(Ordering::Relaxed) != 0
}

/// Взводит эмуляцию raw-клавиши меню (стрелки/Enter): бит в `ms_KeyInput`
/// (down или pressed) + заморозка кэша. `pressed = true` — однократный фронт
/// (навигация в меню), `false` — удержание.
pub(crate) fn set_raw_key(code: u8, pressed: bool) {
    let index = (code >> 5) as usize;
    let bit = 1u32 << (code & 31);
    if index < 6 {
        if pressed {
            RAW_KEYS_PRESSED[index].fetch_or(bit, Ordering::Relaxed);
        } else {
            RAW_KEYS_DOWN[index].fetch_or(bit, Ordering::Relaxed);
        }
        RAW_KEYS_ACTIVE.store(1, Ordering::Relaxed);
    }
}

/// Сбрасывает keybind-эмуляцию (ripper/blade/новые входы) и raw-клавиши меню —
/// вызывается при остановке воспроизведения, старте записи, остановке
/// API-скрипта и входе в loading, чтобы hold-действия и однокадровые фронты
/// не «зависали» и не подмешивались в реальный ввод.
pub(crate) fn clear_keybind_emulation() {
    for slot in &KEYBIND_PRESSED {
        slot.store(0, Ordering::Relaxed);
    }
    for slot in &KEYBIND_HOLD {
        slot.store(0, Ordering::Relaxed);
    }
    clear_raw_keys();
}

/// Сбрасывает raw-клавиши меню и размораживает кэш клавиш. Вызывается из
/// `script_tick` каждый кадр без активных raw-команд (биты живут 1 кадр —
/// иначе меню увидит «залипшую» стрелку) и из `clear_keybind_emulation`.
pub(crate) fn clear_raw_keys() {
    for slot in &RAW_KEYS_DOWN {
        slot.store(0, Ordering::Relaxed);
    }
    for slot in &RAW_KEYS_PRESSED {
        slot.store(0, Ordering::Relaxed);
    }
    RAW_KEYS_ACTIVE.store(0, Ordering::Relaxed);
    let addr = UPDATE_KEYBOARD_ADDR.load(Ordering::Relaxed);
    if addr != 0 {
        unsafe { *(addr as *mut u8) = 1 };
    }
}

/// Читает сэмпл удержания blade за прошедший тик (накоплен детуром
/// `isKeybindDown`). Сбрасывается в конце render-кадра через
/// `reset_keybind_samples`.
#[cfg(debug_assertions)]
pub(crate) fn read_blade_down_sampled() -> bool {
    BLADE_DOWN_SAMPLED.load(Ordering::Relaxed) != 0
}

/// Читает сэмпл фронта ripper за прошедший тик (накоплен детуром
/// `isKeybindPressed`). Сбрасывается в конце render-кадра через
/// `reset_keybind_samples`.
#[cfg(debug_assertions)]
pub(crate) fn read_ripper_pressed_sampled() -> bool {
    RIPPER_PRESSED_SAMPLED.load(Ordering::Relaxed) != 0
}

/// Сбрасывает сэмплы blade/ripper — вызывается в конце каждого render-кадра,
/// чтобы следующий тик накапливал сэмплы с нуля.
#[cfg(debug_assertions)]
pub(crate) fn reset_keybind_samples() {
    BLADE_DOWN_SAMPLED.store(0, Ordering::Relaxed);
    RIPPER_PRESSED_SAMPLED.store(0, Ordering::Relaxed);
}

/// Детур `cInput::updateInputUnit` (__cdecl). Вызывает оригинал, затем для
/// `user_index == 0` перезаписывает unit нашим override (подача ввода) и
/// подаёт raw-клавиши меню в кэш `ms_KeyInput`.
///
/// Детур вызывается игрой несколько раз за кадр и должен быть ЛЁГКИМ: только
/// чтение/запись атомиков и памяти. Никакого `log_line` (chrono + файловый
/// I/O) — при рестарте это даёт рекурсию access violation (см. docs/REPLAY_FINDINGS.md).
unsafe extern "C" fn update_input_unit_detour(unit: *mut types::InputUnit, user_index: i32) {
    if let Some(&orig) = ORIG_UPDATE_INPUT_UNIT.get() {
        unsafe { orig(unit, user_index) };
    }

    if user_index != 0 {
        return;
    }

    // Подача raw-клавиш меню: кэш ms_KeyInput заморожен и перезаписан нашими
    // битмасками — меню читает стрелки/Enter через isKeyDown/isKeyPressed
    // (0x9D93A0/0x9D9400), а не через keybind'ы.
    if RAW_KEYS_ACTIVE.load(Ordering::Relaxed) != 0 {
        apply_raw_keys();
    }

    replay::apply_override(unit);
}

/// Записывает эмулируемые raw-клавиши в `ms_KeyInput` и замораживает кэш
/// (`ms_bUpdateKeyboard = false`), чтобы DirectInput не перезаписал их.
/// Вызывается из детура `updateInputUnit` после оригинала — до `handleActions`,
/// который читает кэш через `isKeyDown`/`isKeyPressed`.
fn apply_raw_keys() {
    let addr = KEY_INPUT_ADDR.load(Ordering::Relaxed);
    if addr == 0 {
        return;
    }
    let k = addr as *mut types::KeyInput;
    unsafe {
        for i in 0..6 {
            (*k).keys_down[i] = RAW_KEYS_DOWN[i].load(Ordering::Relaxed);
            (*k).keys_pressed[i] = RAW_KEYS_PRESSED[i].load(Ordering::Relaxed);
        }
    }
    let ua = UPDATE_KEYBOARD_ADDR.load(Ordering::Relaxed);
    if ua != 0 {
        unsafe { *(ua as *mut u8) = 0 };
    }
}

/// Детур `cInput::isKeybindPressed` (__cdecl, 0x61D2D0). Для эмулируемых
/// keybind'ов (`KEYBIND_PRESSED[keybind] != 0`) возвращает 1 (нажат фронт) —
/// тогда `handleActions` запускает штатную активацию/деактивацию toggle-действий
/// (ripper, lock-on, меню) с проверками условий и анимациями. Остальные
/// keybind'ы идут в оригинал. Результат оригинала для `KEYBIND_RIPPERMODE`
/// накапливается в `RIPPER_PRESSED_SAMPLED` — запись читает реальный фронт.
///
/// Флаг не декрементируется: игра вызывает `isKeybindPressed(11)` спорадически
/// (disable-проверка и enable-проверка — на разных кадрах), и счётчик
/// «растекался» — каждый остаток давал отдельное переключение ripper.
/// Сброс — в начале следующего `script_tick` (см. api.rs).
unsafe extern "C" fn is_keybind_pressed_detour(keybind: i32) -> i32 {
    if (0..addresses::KEYBIND_TOTAL as i32).contains(&keybind)
        && KEYBIND_PRESSED[keybind as usize].load(Ordering::Relaxed) != 0
    {
        return 1;
    }

    let result = if let Some(&orig) = ORIG_IS_KEYBIND_PRESSED.get() {
        unsafe { orig(keybind) }
    } else {
        0
    };
    #[cfg(debug_assertions)]
    if keybind == addresses::KEYBIND_RIPPERMODE && result != 0 {
        RIPPER_PRESSED_SAMPLED.fetch_or(1, Ordering::Relaxed);
    }
    result
}

/// Детур `cInput::isKeybindDown` (__cdecl, 0x61D280). Для эмулируемых
/// keybind'ов (`KEYBIND_HOLD[keybind] != 0`) возвращает 1 (удержание) —
/// hold-действия (blade mode, ninja run, walk, dodge) активируются через
/// `handleActions`. Остальные keybind'ы идут в оригинал. Результат оригинала
/// для `KEYBIND_BLADEMODE` накапливается в `BLADE_DOWN_SAMPLED` — запись
/// читает реальное удержание из этого сэмпла.
unsafe extern "C" fn is_keybind_down_detour(keybind: i32) -> i32 {
    if (0..addresses::KEYBIND_TOTAL as i32).contains(&keybind)
        && KEYBIND_HOLD[keybind as usize].load(Ordering::Relaxed) != 0
    {
        return 1;
    }
    let result = if let Some(&orig) = ORIG_IS_KEYBIND_DOWN.get() {
        unsafe { orig(keybind) }
    } else {
        0
    };
    #[cfg(debug_assertions)]
    if keybind == addresses::KEYBIND_BLADEMODE && result != 0 {
        BLADE_DOWN_SAMPLED.fetch_or(1, Ordering::Relaxed);
    }
    result
}

/// Сохраняет trampoline (адрес оригинальной функции) после создания хука.
fn set_original_update_input_unit(
    orig: unsafe extern "C" fn(*mut types::InputUnit, i32),
) -> Result<(), ()> {
    ORIG_UPDATE_INPUT_UNIT.set(orig).map_err(|_| ())
}

/// Сохраняет trampoline оригинальной `isKeybindPressed` после создания хука.
fn set_original_is_keybind_pressed(orig: unsafe extern "C" fn(i32) -> i32) -> Result<(), ()> {
    ORIG_IS_KEYBIND_PRESSED.set(orig).map_err(|_| ())
}

/// Сохраняет trampoline оригинальной `isKeybindDown` после создания хука.
fn set_original_is_keybind_down(orig: unsafe extern "C" fn(i32) -> i32) -> Result<(), ()> {
    ORIG_IS_KEYBIND_DOWN.set(orig).map_err(|_| ())
}

/// MinHook-хуки ввода. Поля приватные: хуки живут, пока живёт структура
/// (деструктор `MhHook` снимает хук), наружу выставляются только функции
/// чтения сырого ввода.
#[allow(dead_code)] // keep-alive: поля не читаются, но Drop снимает хуки
pub struct InputHooks {
    input: Option<MhHook>,
    keybind: Option<MhHook>,
    keybind_down: Option<MhHook>,
}

impl InputHooks {
    /// Устанавливает все хуки ввода и запоминает адреса сырого ввода.
    /// Каждый хук логирует свой результат в debug.log.
    pub fn new(base_addr: usize) -> Self {
        KEY_INPUT_ADDR.store(
            if base_addr == 0 {
                0
            } else {
                base_addr + addresses::KEY_INPUT
            },
            Ordering::Relaxed,
        );
        MOUSE_INPUT_ADDR.store(
            if base_addr == 0 {
                0
            } else {
                base_addr + addresses::MOUSE_INPUT
            },
            Ordering::Relaxed,
        );
        UPDATE_KEYBOARD_ADDR.store(
            if base_addr == 0 {
                0
            } else {
                base_addr + addresses::UPDATE_KEYBOARD
            },
            Ordering::Relaxed,
        );

        let input = Self::create_input_hook(base_addr);
        let keybind = Self::create_keybind_hook(base_addr);
        let keybind_down = Self::create_keybind_down_hook(base_addr);

        logger::log_line(&format!(
            "=== drmod init === base=0x{:08X} input_hook={} keybind_hook={} keybind_down_hook={}",
            base_addr,
            if input.is_some() { "OK" } else { "FAIL" },
            if keybind.is_some() { "OK" } else { "FAIL" },
            if keybind_down.is_some() { "OK" } else { "FAIL" }
        ));

        Self {
            input,
            keybind,
            keybind_down,
        }
    }

    /// Устанавливает MinHook на `cInput::updateInputUnit` (0x9DAFE0):
    /// после вызова оригинала детур перезаписывает InputUnit игрока.
    fn create_input_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_input_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + addresses::UPDATE_INPUT_UNIT) as *mut c_void;
        let detour = update_input_unit_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_input_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "C" fn(*mut types::InputUnit, i32) =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = set_original_update_input_unit(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!("create_input_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        logger::log_line(&format!(
            "create_input_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
    }

    /// Устанавливает MinHook на `cInput::isKeybindPressed` (0x61D2D0):
    /// подменяет результат для toggle-действий (ripper), чтобы handleActions
    /// запускал их штатным путём (с условиями и анимациями).
    fn create_keybind_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_keybind_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + addresses::IS_KEYBIND_PRESSED) as *mut c_void;
        let detour = is_keybind_pressed_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_keybind_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "C" fn(i32) -> i32 =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = set_original_is_keybind_pressed(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!("create_keybind_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        logger::log_line(&format!(
            "create_keybind_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
    }

    /// Устанавливает MinHook на `cInput::isKeybindDown` (0x61D280):
    /// подменяет результат для hold-действий (blade mode).
    fn create_keybind_down_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_keybind_down_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + addresses::IS_KEYBIND_DOWN) as *mut c_void;
        let detour = is_keybind_down_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_keybind_down_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "C" fn(i32) -> i32 =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = set_original_is_keybind_down(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!("create_keybind_down_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        logger::log_line(&format!(
            "create_keybind_down_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
    }
}

/// Читает сырой ввод клавиатуры: (m_aKeysDown, m_aKeysPressed).
/// `None`, если адрес не установлен (base_addr == 0).
pub fn read_keys() -> Option<([u32; 6], [u32; 6])> {
    let addr = KEY_INPUT_ADDR.load(Ordering::Relaxed);
    if addr == 0 {
        return None;
    }
    let k: types::KeyInput = unsafe { (addr as *const types::KeyInput).read() };
    Some((k.keys_down, k.keys_pressed))
}

/// Читает сырое состояние мыши (зажатые кнопки).
/// `None`, если адрес не установлен (base_addr == 0).
pub fn read_mouse() -> Option<types::MouseState> {
    let addr = MOUSE_INPUT_ADDR.load(Ordering::Relaxed);
    if addr == 0 {
        return None;
    }
    let base = addr as *const u8;
    Some(unsafe {
        types::MouseState {
            buttons: *(base.cast::<i32>()),
        }
    })
}