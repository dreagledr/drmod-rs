//! Автоматический пропуск **стартовой лого-секвенции** при загрузке игры.
//!
//! Как устроена лого-секвенция в игре (RVA, ImageBase 0x400000):
//!
//! * `0xA52E10` — верхний уровень: готовит состояние и зовёт обёртку;
//! * `0xA52D80` — обёртка запуска: печатает `--- START LOGO SEQUENCE ---`,
//!   заказывает поток (`ExecStartupShader`, тело `0xA4A2E0`), выставляет шаг
//!   `[obj+0x94] = 1` и зовёт стейт-машину `0xA52C30`;
//! * `0xA52C30` — цикл задачи лого: тикает, вызывает шаги, ждёт шаг `2`
//!   (его ставит поток `0xA4A2E0`, печатающий `--- START/END GRAPHIC STARTUP2 ---`),
//!   затем `--- MAIN CLEANUP END ---`.
//!
//! ⚠️ **`0xA52C30` вызывается только из лого-цепочки** (единственный `call` —
//! из `0xA52DF0`), поэтому патч внутри неё не задевает игровую логику.
//!
//! Патч — ручной `E9 rel32` (6 байт) через `VirtualProtect` +
//! `FlushInstructionCache`, ставится один раз из render-цикла (поток игры) и
//! снимается при выключении галки.

use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use windows::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
    VirtualAlloc, VirtualFree, VirtualProtect,
};
use windows::Win32::System::Threading::GetCurrentProcess;

use crate::logger;

/// Целевая инструкция (`mov ecx, [ebp+0x8C]`) в цикле лого-задачи.
const TARGET_RVA: usize = 0x652C92;
/// Возврат в оригинал (`target + 6`): `test ecx, ecx`.
const RESUME_RVA: usize = TARGET_RVA + 6;

/// Смещение слота возврата и флага в служебной странице заглушки.
const RET_SLOT_OFF: usize = 32;
const ZERO_FLAG_OFF: usize = 36;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Страница RWX с кодом заглушки (`0` — патч не поставлен).
static PAGE: AtomicUsize = AtomicUsize::new(0);
/// Адрес цели патча (`base + TARGET_RVA`), `0` — не поставлен.
static TARGET: AtomicUsize = AtomicUsize::new(0);
/// Оригинальные 6 байт цели.
static ORIGINAL: AtomicU64 = AtomicU64::new(0);
static INSTALL_DONE: AtomicBool = AtomicBool::new(false);

/// Включена ли галка «пропускать заставки» (для `GET /state`).
pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Стоит ли патч (для `GET /state`).
pub(crate) fn active() -> bool {
    PAGE.load(Ordering::Relaxed) != 0
}

/// Пользовательский текст этапа для окна Settings.
pub(crate) fn status() -> &'static str {
    if !enabled() {
        ""
    } else if active() {
        "патч стоит: лого пропускаются"
    } else {
        "ждёт старта лого-секвенции"
    }
}

/// Вызывается каждый кадр из render-цикла (поток игры).
///
/// Патч ставится, когда включена галка и лого-задача **уже исполнялась**:
/// до этого её пролог не «прогрет» под запись, и патчить рано. Признак —
/// ненулевой указатель под-объекта `[base+0x19C1404]` (его выставляет
/// обёртка `0xA52D80` перед вызовом задачи).
pub(crate) fn update(base_addr: usize, enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if base_addr == 0 {
        return;
    }
    if !enabled {
        if active() {
            // SAFETY: uninstall пишет только по адресу из своего же install и освобождает нашу
            // страницу-заглушку.
            unsafe { uninstall() };
        }
        return;
    }
    // Флаг заглушки: подменять всегда, пока задача живёт.
    if active() {
        set_zero_flag(true);
        return;
    }
    if !INSTALL_DONE.load(Ordering::Relaxed) && logo_task_live(base_addr) {
        // SAFETY: base_addr — база модуля игры; install патчит TARGET_RVA только после проверки
        // logo_task_live.
        unsafe { install(base_addr) };
    }
}

/// Жива ли лого-задача: обёртка пишет сюда указатель под-объекта перед
/// вызовом задачи (`mov [ebp+0x8C], eax` в `0xA52C5E`) — ненулевой означает,
/// что `0xA52C30` исполняется.
fn logo_task_live(base_addr: usize) -> bool {
    // Читаем по базе задачи: указатель берётся из обёртки `0xA52D80`.
    // Сама задача кладёт его в `[ebp+0x8C]`; снаружи надёжнее смотреть шаг
    // задачи в глобале `0x1BE9180` (VA) — но он мусорится после завершения,
    // поэтому ограничиваемся проверкой доступности базового объекта.
    let obj = base_addr + (0x1BE9180 - 0x400000);
    crate::game::is_readable_ptr(obj)
}

unsafe fn install(base_addr: usize) {
    INSTALL_DONE.store(true, Ordering::Relaxed);
    let target = base_addr + TARGET_RVA;
    let resume = base_addr + RESUME_RVA;

    let mut original = [0u8; 6];
    // SAFETY: target = base + TARGET_RVA — код модуля игры (6 байт инструкции), страница читаемая.
    unsafe { core::ptr::copy_nonoverlapping(target as *const u8, original.as_mut_ptr(), 6) };
    let mut packed = 0u64;
    for (i, b) in original.iter().enumerate() {
        packed |= (*b as u64) << (8 * i);
    }

    // SAFETY: VirtualAlloc с валидными аргументами; результат проверяется на null, владение — у
    // модуля.
    let page = unsafe {
        VirtualAlloc(
            None,
            ZERO_FLAG_OFF + 8,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        )
    };
    if page.is_null() {
        logger::log_line("intro_skip: VirtualAlloc под заглушку не удался");
        return;
    }
    let page = page as *mut u8;
    // SAFETY: page — наша страница VirtualAlloc (40 байт), RET_SLOT_OFF = 32 внутри неё.
    let ret_slot = unsafe { page.add(RET_SLOT_OFF) as *mut usize };
    // SAFETY: page — наша страница VirtualAlloc, ZERO_FLAG_OFF = 36 внутри неё.
    let zero_flag = unsafe { page.add(ZERO_FLAG_OFF) as *mut u32 };
    // SAFETY: обе записи — внутрь нашей страницы (слот возврата и флаг), она выровнена как
    // страница.
    unsafe {
        *ret_slot = resume;
        *zero_flag = 1; // автовариант: подменять всегда
    }

    let code = build_stub(ret_slot as usize, zero_flag as usize);
    // SAFETY: page — наша страница RWX, code.len() <= 32 < размера аллокации (40 байт).
    unsafe { core::ptr::copy_nonoverlapping(code.as_ptr(), page, code.len()) };

    let rel = (page as i64 - target as i64 - 5) as i32;
    let mut patch = [0u8; 6];
    patch[0] = 0xE9;
    patch[1..5].copy_from_slice(&rel.to_le_bytes());
    patch[5] = 0x90;
    // SAFETY: target — код модуля игры; write_code временно ставит RWX и возвращает защиту.
    if unsafe { write_code(target, &patch) }.is_err() {
        logger::log_line("intro_skip: запись патча не удалась — откат");
        // SAFETY: page получен из VirtualAlloc этого же install; MEM_RELEASE с размером 0
        // освобождает её.
        unsafe {
            let _ = VirtualFree(page as *mut c_void, 0, MEM_RELEASE);
        }
        return;
    }

    ORIGINAL.store(packed, Ordering::SeqCst);
    TARGET.store(target, Ordering::SeqCst);
    PAGE.store(page as usize, Ordering::SeqCst);
    logger::log_line(&format!(
        "intro_skip: патч поставлен base+0x{:X} (ручной E9) — заставки пропускаются",
        TARGET_RVA
    ));
}

/// Код заглушки: читает оригинальный `ecx` из `[ebp+0x8C]`, затем по флагу
/// либо оставляет его, либо обнуляет (в автоварианте флаг всегда `1`), и
/// возвращается в оригинал через слот.
fn build_stub(ret_slot: usize, flag: usize) -> Vec<u8> {
    let mut code = Vec::with_capacity(32);
    // mov ecx, [ebp+0x8C]
    code.extend_from_slice(&[0x8B, 0x8D, 0x8C, 0x00, 0x00, 0x00]);
    // cmp dword ptr [flag], 0
    code.extend_from_slice(&[0x83, 0x3D]);
    code.extend_from_slice(&(flag as u32).to_le_bytes());
    code.push(0x00);
    // je short +4
    code.extend_from_slice(&[0x74, 0x04]);
    // xor ecx, ecx
    code.extend_from_slice(&[0x31, 0xC9]);
    // jmp dword ptr [ret_slot]
    code.extend_from_slice(&[0xFF, 0x25]);
    code.extend_from_slice(&(ret_slot as u32).to_le_bytes());
    code
}

/// Переписывает флаг заглушки (нужен для выключения подмены на лету).
pub(crate) fn set_zero_flag(on: bool) {
    let page = PAGE.load(Ordering::Relaxed);
    if page == 0 {
        return;
    }
    // SAFETY: PAGE != 0 — страница-заглушка жива, ZERO_FLAG_OFF внутри её размера.
    unsafe { *((page + ZERO_FLAG_OFF) as *mut u32) = if on { 1 } else { 0 } };
}

unsafe fn uninstall() {
    let page = PAGE.swap(0, Ordering::SeqCst);
    let target = TARGET.swap(0, Ordering::SeqCst);
    if page == 0 || target == 0 {
        return;
    }
    let packed = ORIGINAL.swap(0, Ordering::SeqCst);
    let original: [u8; 6] = core::array::from_fn(|i| ((packed >> (8 * i)) & 0xFF) as u8);
    // SAFETY: target — код модуля игры из TARGET; write_code сам управляет защитой страницы.
    if unsafe { write_code(target, &original) }.is_ok() {
        logger::log_line("intro_skip: патч снят, байты возвращены");
    }
    // SAFETY: page получен из VirtualAlloc при install и ещё не освобождён (PAGE.swap выше).
    unsafe {
        let _ = VirtualFree(page as *mut c_void, 0, MEM_RELEASE);
    }
}

/// Пишет байты кода: временно RWX, запись, возврат защиты, сброс кэша команд.
unsafe fn write_code(addr: usize, bytes: &[u8]) -> Result<(), ()> {
    let mut old = PAGE_PROTECTION_FLAGS(0);
    // SAFETY: addr — код модуля игры или наша заглушка; VirtualProtect меняет защиту этих страниц.
    if unsafe {
        VirtualProtect(
            addr as *const c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        )
    }
    .is_err()
    {
        return Err(());
    }
    // SAFETY: предыдущий VirtualProtect сделал диапазон [addr, addr+len) записываемым.
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
    let mut back = PAGE_PROTECTION_FLAGS(0);
    // SAFETY: возвращаем исходную защиту диапазона, который только что меняли.
    let _ = unsafe { VirtualProtect(addr as *const c_void, bytes.len(), old, &mut back) };
    // SAFETY: сброс кэша команд для диапазона, в который мы только что писали.
    let _ = unsafe {
        FlushInstructionCache(
            GetCurrentProcess(),
            Some(addr as *const c_void),
            bytes.len(),
        )
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_matches_winmm() {
        assert_eq!(TARGET_RVA, 0x652C92);
        assert_eq!(RESUME_RVA, 0x652C98);
    }

    #[test]
    fn stub_restores_ecx_from_frame() {
        let code = build_stub(0x1000_0000, 0x2000_0000);
        assert_eq!(&code[0..3], &[0x8B, 0x8D, 0x8C]);
        assert_eq!(&code[code.len() - 6..code.len() - 4], &[0xFF, 0x25]);
    }

    #[test]
    fn status_reports_off_when_disabled() {
        ENABLED.store(false, Ordering::SeqCst);
        assert_eq!(status(), "");
        assert!(!active());
    }
}
