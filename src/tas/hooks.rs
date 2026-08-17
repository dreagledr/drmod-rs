//! Хуки ввода (MinHook): подача InputUnit через `updateInputUnit`,
//! эмуляция ripper/blade через `isKeybindPressed`/`isKeybindDown`,
//! чтение сырого ввода (клавиатура/мышь).

use hudhook::mh::{MH_ApplyQueued, MhHook};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::replay;
use super::types;

/// cInput::ms_KeyInput — адрес сырого ввода клавиатуры (вычисляется в `install`).
static KEY_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// cInput::ms_MouseInput — адрес сырого ввода мыши (вычисляется в `install`).
static MOUSE_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);

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
                base_addr + replay::KEY_INPUT
            },
            Ordering::Relaxed,
        );
        MOUSE_INPUT_ADDR.store(
            if base_addr == 0 {
                0
            } else {
                base_addr + replay::MOUSE_INPUT
            },
            Ordering::Relaxed,
        );

        let input = Self::create_input_hook(base_addr);
        let keybind = Self::create_keybind_hook(base_addr);
        let keybind_down = Self::create_keybind_down_hook(base_addr);

        replay::log_line(&format!(
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
            replay::log_line("create_input_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + replay::UPDATE_INPUT_UNIT) as *mut c_void;
        let detour = replay::update_input_unit_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                replay::log_line(&format!(
                    "create_input_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "C" fn(*mut types::InputUnit, i32) =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = replay::set_original_update_input_unit(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            replay::log_line(&format!("create_input_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        replay::log_line(&format!(
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
            replay::log_line("create_keybind_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + replay::IS_KEYBIND_PRESSED) as *mut c_void;
        let detour = replay::is_keybind_pressed_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                replay::log_line(&format!(
                    "create_keybind_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "C" fn(i32) -> i32 =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = replay::set_original_is_keybind_pressed(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            replay::log_line(&format!("create_keybind_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        replay::log_line(&format!(
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
            replay::log_line("create_keybind_down_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + replay::IS_KEYBIND_DOWN) as *mut c_void;
        let detour = replay::is_keybind_down_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                replay::log_line(&format!(
                    "create_keybind_down_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "C" fn(i32) -> i32 =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = replay::set_original_is_keybind_down(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            replay::log_line(&format!("create_keybind_down_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        replay::log_line(&format!(
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