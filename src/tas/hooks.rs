//! Хуки ввода (MinHook): подача InputUnit через `updateInputUnit`,
//! эмуляция ripper/blade через `isKeybindPressed`/`isKeybindDown`,
//! чтение сырого ввода (клавиатура/мышь).

use hudhook::mh::{MH_ApplyQueued, MhHook};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;

use super::addresses::{
    self, DISABLE_RIPPER_MODE, ENABLE_RIPPER_MODE, KEYBIND_BLADEMODE, KEYBIND_RIPPERMODE,
};
use super::replay;
use super::types;
use crate::logger;

/// cInput::ms_KeyInput — адрес сырого ввода клавиатуры (вычисляется в `install`).
static KEY_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// cInput::ms_MouseInput — адрес сырого ввода мыши (вычисляется в `install`).
static MOUSE_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// Trampoline оригинальной `cInput::updateInputUnit` (устанавливается в `create_input_hook`).
static ORIG_UPDATE_INPUT_UNIT: OnceLock<unsafe extern "C" fn(*mut types::InputUnit, i32)> =
    OnceLock::new();
/// Trampoline оригинальной `cInput::isKeybindPressed`.
static ORIG_IS_KEYBIND_PRESSED: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
/// Trampoline оригинальной `cInput::isKeybindDown`.
static ORIG_IS_KEYBIND_DOWN: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
/// Базовый адрес модуля игры (устанавливается в `HelloHud::new`) — нужен
/// для прямых вызовов `enableRipperMode`/`disableRipperMode`.
static BASE_ADDR: OnceLock<usize> = OnceLock::new();
/// Остаток кадров эмуляции клавиши R (ripper) в детуре.
static RIPPER_FRAMES: AtomicU32 = AtomicU32::new(0);
/// Флаг удержания blade mode (isKeybindDown, hold) в детуре.
static BLADE_HOLD: AtomicU32 = AtomicU32::new(0);

/// Запоминает базовый адрес модуля для прямых вызовов ripper.
pub(crate) fn set_base_addr(addr: usize) -> Result<(), ()> {
    BASE_ADDR.set(addr).map_err(|_| ())
}

/// Взводит эмуляцию клавиши R (ripper) на `n` кадров — сырой ввод, который
/// читается `isKeybindDown(KEYBIND_RIPPERMODE)`, а не `InputUnit`.
pub(crate) fn set_ripper_frames(n: u32) {
    RIPPER_FRAMES.store(n, Ordering::Relaxed);
}

/// Сколько кадров эмуляции R осталось (для debug-панели).
pub(crate) fn ripper_frames() -> u32 {
    RIPPER_FRAMES.load(Ordering::Relaxed)
}

/// Взводит/снимает удержание blade mode (isKeybindDown, hold-действие).
pub(crate) fn set_blade_hold(on: bool) {
    BLADE_HOLD.store(if on { 1 } else { 0 }, Ordering::Relaxed);
}

/// Удерживается ли blade mode сейчас (для debug-панели).
pub(crate) fn blade_hold() -> bool {
    BLADE_HOLD.load(Ordering::Relaxed) != 0
}

/// Сбрасывает keybind-эмуляцию (ripper/blade) — вызывается при остановке
/// воспроизведения, старте записи и входе в loading, чтобы hold-действие
/// (blade) и однокадровый фронт (ripper) не «зависали» и не подмешивались
/// в реальный ввод.
#[cfg(debug_assertions)]
pub(crate) fn clear_keybind_emulation() {
    RIPPER_FRAMES.store(0, Ordering::Relaxed);
    BLADE_HOLD.store(0, Ordering::Relaxed);
}

/// Включает Ripper Mode напрямую (обход ввода) — `Pl0000::enableRipperMode`
/// (`__thiscall`, `this` = указатель на объект игрока).
pub(crate) fn enable_ripper(player: *mut u8) {
    let Some(&base) = BASE_ADDR.get() else {
        return;
    };
    type Fn = unsafe extern "thiscall" fn(*mut u8);
    let f: Fn = unsafe { std::mem::transmute((base + ENABLE_RIPPER_MODE) as *const ()) };
    unsafe { f(player) };
}

/// Выключает Ripper Mode (`Pl0000::disableRipperMode(bool)`, `__thiscall`).
pub(crate) fn disable_ripper(player: *mut u8) {
    let Some(&base) = BASE_ADDR.get() else {
        return;
    };
    type Fn = unsafe extern "thiscall" fn(*mut u8, bool);
    let f: Fn = unsafe { std::mem::transmute((base + DISABLE_RIPPER_MODE) as *const ()) };
    unsafe { f(player, false) };
}

/// Детур `cInput::updateInputUnit` (__cdecl). Вызывает оригинал, затем для
/// `user_index == 0` перезаписывает unit нашим override (подача ввода).
///
/// Детур вызывается игрой несколько раз за кадр и должен быть ЛЁГКИМ: только
/// чтение/запись атомиков. Никакого `log_line` (chrono + файловый I/O) — при
/// рестарте это даёт рекурсию access violation (см. docs/REPLAY_FINDINGS.md).
unsafe extern "C" fn update_input_unit_detour(unit: *mut types::InputUnit, user_index: i32) {
    if let Some(&orig) = ORIG_UPDATE_INPUT_UNIT.get() {
        unsafe { orig(unit, user_index) };
    }

    if user_index != 0 {
        return;
    }

    replay::apply_override(unit);
}

/// Детур `cInput::isKeybindPressed` (__cdecl, 0x61D2D0). Для `KEYBIND_RIPPERMODE`
/// возвращает 1 (нажат фронт), пока эмуляция R активна (`RIPPER_FRAMES > 0`) —
/// тогда `handleActions` запускает штатную активацию/деактивацию ripper
/// с проверками условий и анимациями. Остальные keybind'ы идут в оригинал.
unsafe extern "C" fn is_keybind_pressed_detour(keybind: i32) -> i32 {
    if keybind == KEYBIND_RIPPERMODE && RIPPER_FRAMES.load(Ordering::Relaxed) > 0 {
        RIPPER_FRAMES.fetch_sub(1, Ordering::Relaxed);
        return 1;
    }

    if let Some(&orig) = ORIG_IS_KEYBIND_PRESSED.get() {
        return unsafe { orig(keybind) };
    }
    0
}

/// Детур `cInput::isKeybindDown` (__cdecl, 0x61D280). Для `KEYBIND_BLADEMODE`
/// возвращает 1 (удержание), пока `BLADE_HOLD` взведён — blade mode это
/// hold-действие, активируется удержанием клавиши через handleActions.
unsafe extern "C" fn is_keybind_down_detour(keybind: i32) -> i32 {
    if keybind == KEYBIND_BLADEMODE && BLADE_HOLD.load(Ordering::Relaxed) != 0 {
        return 1;
    }
    if let Some(&orig) = ORIG_IS_KEYBIND_DOWN.get() {
        return unsafe { orig(keybind) };
    }
    0
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

/// Читает сырое состояние мыши (кнопки + позиция).
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
            buttons_pressed: *(base.add(0x04).cast::<i32>()),
            position: *(base.add(0x10).cast::<[f32; 2]>()),
            last_position: *(base.add(0x20).cast::<[f32; 2]>()),
        }
    })
}