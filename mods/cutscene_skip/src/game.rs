//! Адреса и вызовы игры, нужные скипу катсцены.
//!
//! Источник истины — `src/game/cutscene_skip.rs` и `src/game/phase.rs`
//! основного мода drmod-rs; здесь минимальный порт (синхронизировать при
//! правках там). Все адреса — RVA от базы модуля игры.

use std::ffi::CString;

/// `Trigger::staFlags`: word0 — коды 0..31, следующий dword — коды 32..63.
pub(crate) const STA_FLAGS: usize = 0x17EA060;
/// `GameMenuStatus` (1 = InGame, 6 = CutscenePause, 12 = Pause1).
pub(crate) const MENU_STATUS: usize = 0x17E9F9C;
/// Шаг жизненного цикла катсценного меню (0..6), живёт при статусе 6.
pub(crate) const MENU_STEP: usize = 0x17EA118;
/// Указатель на живой объект `cEventPauseMenu`.
pub(crate) const MENU_OBJ: usize = 0x17EA140;
/// Текущая подфаза: её хэш (объект состояния `+0x38`).
pub(crate) const SUB_HASH: usize = 0x14B9178;
/// `cSlowRateManager::updateFrameTime` (thiscall) — пер-кадровая точка, раз за
/// итерацию главного цикла `0xA52510`.
pub(crate) const FRAME_TIME_UPDATE: usize = 0xA03970;

/// `request_subphase(this, name, arg)` (thiscall, `ret 8`) — заказ смены
/// подфазы штатным путём движка; во время события (`STA_EVENT`) игнорируется.
const REQUEST_SUBPHASE_RVA: usize = 0x94E1F0;
/// Объект состояния игры (`this` для заказа подфазы).
const STATE_OBJECT_RVA: usize = 0x14B9140;
/// `Trigger::staFlags`: флаг `STA_EVENT` (код 2) — блокирует заказ подфазы.
const STA_EVENT_MASK: u32 = 0x4000_0000;

/// Заказ смены подфазы штатной функцией движка. `clear_event` — снять перед
/// вызовом `STA_EVENT` (иначе во время события заказ игнорируется).
///
/// Вызывать только из потока игры: движок не потокобезопасен.
pub(crate) fn order_subphase(
    base_addr: usize,
    name: &str,
    arg: u32,
    clear_event: bool,
) -> Option<()> {
    let cname = CString::new(name).ok()?;
    unsafe {
        if clear_event {
            let flags = (base_addr + STA_FLAGS) as *mut u32;
            *flags &= !STA_EVENT_MASK;
        }
        let request: extern "thiscall" fn(*mut u8, *const i8, u32) =
            std::mem::transmute(base_addr + REQUEST_SUBPHASE_RVA);
        request((base_addr + STATE_OBJECT_RVA) as *mut u8, cname.as_ptr(), arg);
    }
    Some(())
}

pub(crate) fn read_u32(addr: usize) -> u32 {
    unsafe { *(addr as *const u32) }
}

pub(crate) fn read_i32(addr: usize) -> i32 {
    unsafe { *(addr as *const i32) }
}

pub(crate) fn read_usize(addr: usize) -> usize {
    unsafe { *(addr as *const usize) }
}

pub(crate) fn write_u32(addr: usize, value: u32) {
    unsafe { *(addr as *mut u32) = value };
}

/// Проверяет, что адрес указывает на committed и читаемую память, — защита от
/// dangling-указателя объекта меню (порт `game::is_readable_ptr`).
pub(crate) fn is_readable_ptr(addr: usize) -> bool {
    use windows::Win32::System::Memory::{
        MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_GUARD,
        PAGE_PROTECTION_FLAGS, PAGE_READONLY, PAGE_READWRITE, VirtualQuery,
    };

    const PAGE_READABLE: PAGE_PROTECTION_FLAGS = PAGE_PROTECTION_FLAGS(
        PAGE_READONLY.0 | PAGE_READWRITE.0 | PAGE_EXECUTE_READ.0 | PAGE_EXECUTE_READWRITE.0,
    );

    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    let ok = unsafe {
        VirtualQuery(
            Some(addr as *const core::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    // PAGE_GUARD — комбинированный флаг: чтение guard-страницы даёт
    // STATUS_GUARD_PAGE_VIOLATION, хотя бит «readable» может стоять.
    ok != 0 && (mbi.Protect & PAGE_READABLE).0 != 0 && (mbi.Protect & PAGE_GUARD).0 == 0
}
