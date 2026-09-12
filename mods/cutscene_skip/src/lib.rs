//! Скип in-engine катсцены как самостоятельный мод: DLL с MinHook-хуком на
//! кадровый обновитель времени игры (`cSlowRateManager::updateFrameTime`).
//! Лаунчер — в `src/main.rs`, логика скипа — в `src/skip.rs`.
//!
//! Хук на `updateFrameTime` даёт ровно один вызов за итерацию главного цикла
//! игры — этого достаточно кадровому автомату (и это поток игры, где движок
//! можно звать; см. `game::order_subphase`).

mod game;
mod log;
mod minhook;
mod skip;

use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::core::PCSTR;

/// Заголовок окна игры (лаунчер ищет по нему окно).
pub const DEFAULT_TITLE: &str = "METAL GEAR RISING: REVENGEANCE";
/// Имя exe игры (имя файла и имя процесса).
pub const PROCESS_NAME: &str = "METAL GEAR RISING REVENGEANCE.exe";
/// Имя нашей DLL (в неё встраивается лаунчером).
pub const LIB_NAME: &str = "cutscene_skip_lib.dll";

const DATA_DIR_NAME: &str = "cutscene_skip";

/// Каталог мода: `%LOCALAPPDATA%\cutscene_skip` (лог и распакованная DLL).
pub fn data_dir() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA")
        .ok()
        .map(|local| PathBuf::from(local).join(DATA_DIR_NAME))
}

/// Путь к лог-файлу мода (его читает лаунчер с `--follow`).
pub fn log_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("cutscene_skip.log"))
}

/// База модуля игры (`GetModuleHandleA(null)`).
static BASE: AtomicUsize = AtomicUsize::new(0);
/// Автомат скипа — живёт в потоке игры (детур), поэтому под мьютексом.
static STATE: Mutex<skip::CutsceneSkip> = Mutex::new(skip::CutsceneSkip::new());
/// Трамплин оригинала `updateFrameTime`.
static ORIG_FRAME_TIME: OnceLock<unsafe extern "thiscall" fn(*mut u8, i32, f32)> = OnceLock::new();

/// Детур `cSlowRateManager::updateFrameTime` (thiscall, 0xA03970). Вызывает
/// оригинал и крутит автомат скипа; логи только на переходах этапов, чтобы
/// кадровый путь оставался лёгким.
unsafe extern "thiscall" fn frame_time_detour(this: *mut u8, flag: i32, rate: f32) {
    if let Some(&orig) = ORIG_FRAME_TIME.get() {
        unsafe { orig(this, flag, rate) };
    }
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }
    let menu_status = game::read_i32(base + game::MENU_STATUS);
    if let Ok(mut state) = STATE.lock() {
        state.update(base, menu_status);
    }
}

/// Ставит хук. Вызывается из отдельного потока: в `DllMain` держится loader
/// lock, и грузить/патчить код там нельзя.
fn install() {
    let base = unsafe { GetModuleHandleA(PCSTR::null()) }
        .map(|h| h.0 as usize)
        .unwrap_or(0);
    if base == 0 {
        log::log_line("cutscene_skip: не нашёл базовый адрес модуля игры");
        return;
    }
    BASE.store(base, Ordering::Relaxed);

    if let Err(e) = unsafe { minhook::initialize() } {
        log::log_line(&format!("cutscene_skip: MH_Initialize: {e:?}"));
        return;
    }

    let target = (base + game::FRAME_TIME_UPDATE) as *mut c_void;
    let hook = match unsafe { minhook::MhHook::new(target, frame_time_detour as *mut c_void) } {
        Ok(hook) => hook,
        Err(e) => {
            log::log_line(&format!(
                "cutscene_skip: MH_CreateHook 0x{:08X}: {e:?}",
                target as usize
            ));
            return;
        }
    };
    let trampoline: unsafe extern "thiscall" fn(*mut u8, i32, f32) =
        unsafe { std::mem::transmute(hook.trampoline()) };
    if ORIG_FRAME_TIME.set(trampoline).is_err() {
        log::log_line("cutscene_skip: трамплин уже сохранён — хук не переустанавливаю");
        return;
    }
    if let Err(e) = unsafe { hook.queue_enable() } {
        log::log_line(&format!("cutscene_skip: queue_enable: {e:?}"));
        return;
    }
    if let Err(e) = unsafe { minhook::apply_queued() } {
        log::log_line(&format!("cutscene_skip: MH_ApplyQueued: {e:?}"));
        return;
    }
    // MhHook не имеет Drop — MinHook держит хук сам, хранить обёртку не нужно.
    log::log_line(&format!(
        "cutscene_skip: хук установлен (base=0x{base:08X}, target=0x{:08X})",
        target as usize
    ));
}

/// Точка входа DLL. Тяжёлую работу (MinHook, поиск базы) делаем в отдельном
/// потоке.
///
/// # Safety
///
/// Вызывается загрузчиком Windows с `reason == 1` (`DLL_PROCESS_ATTACH`); в
/// этот момент исполняется под loader lock, поэтому здесь нельзя грузить код.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    _hmodule: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        std::thread::spawn(install);
    }
    1
}
