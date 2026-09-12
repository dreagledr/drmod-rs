//! Смена фазы/подфазы (сцены) движка — вызов внутренних функций игры.
//!
//! Восстановлено дизассемблированием (2026-09-12):
//! `change_phase` (RVA `0x8E3040`, thiscall, 4 аргумента) дёргает менеджер фаз
//! (RVA `0x19C3D08`) — тот же путь, которым идёт триггерный акт
//! `Trigger::cActPhaseSubphase::execute` (RVA `0x87F150`, зовёт `0x8E3040`) и
//! переходы движка (call site `0x53A86D`); там же рядом лежит вариант `0x8E30B0`
//! (другой режим). Идентификатор фазы — числовой: `p370` = `0x370`, `pd20` =
//! `0xD20` (совпадает с `mission_id` и с константой в обработчике паузы).
//! `hash_fn` (RVA `0xA03EA0`, cdecl) — CRC32-подобный хэш имени (нижний
//! регистр), им сценарные данные сопоставляют имена объектов.
//!
//! Зачем: в PC-версии in-engine сцена (например, монолог в фазе p370) не
//! помечена скипаемой — консольный пункт «Skip» в катсценном меню ничего не
//! делает, а сцена висит на ожидании звука. Прямая смена фазы/подфазы закрывает
//! сцену штатным путём движка.

use std::ffi::CString;

/// `change_phase(this, id, arg2, arg3, mode)` (thiscall, `ret 0x10`).
const CHANGE_PHASE_RVA: usize = 0x8E3040;
/// Менеджер фаз (синглтон, `this` для `change_phase`).
const PHASE_MANAGER_RVA: usize = 0x19C3D08;
/// `hash_fn(const char*) -> u32` — хэш имени (cdecl), чистый (можно из любого потока).
const HASH_FN_RVA: usize = 0xA03EA0;
/// `request_subphase(this, name, arg)` (thiscall, `ret 8`) — заказ смены подфазы
/// (тот же путь, что у движка: пишет фазу/хэш/имя, ставит флаг и состояние
/// загрузки `0x12`). В начале — проверка `STA_EVENT`: во время события запрос
/// игнорируется (встроенная защита движка).
const REQUEST_SUBPHASE_RVA: usize = 0x94E1F0;
/// Объект состояния игры (`this` для заказа подфазы).
const STATE_OBJECT_RVA: usize = 0x14B9140;
/// `Trigger::staFlags` (флаг `STA_EVENT` = код 2 → маска `0x4000_0000`).
const STA_FLAGS_RVA: usize = 0x17EA060;
const STA_EVENT_MASK: u32 = 0x4000_0000;

/// Хэш имени, как его считает движок (для сопоставления с сценарными данными).
pub(crate) fn hash_name(base_addr: usize, name: &str) -> Option<u32> {
    let cname = CString::new(name).ok()?;
    unsafe {
        let hash_name: extern "cdecl" fn(*const i8) -> u32 =
            std::mem::transmute(base_addr + HASH_FN_RVA);
        Some(hash_name(cname.as_ptr()))
    }
}

/// Заказ смены подфазы штатной функцией движка. `clear_event` — снять перед
/// вызовом флаг `STA_EVENT` (иначе во время события заказ игнорируется).
///
/// Вызывать только из render-цикла (поток игры): движок не потокобезопасен.
pub(crate) fn order_subphase(
    base_addr: usize,
    name: &str,
    arg: u32,
    clear_event: bool,
) -> Option<()> {
    let cname = CString::new(name).ok()?;
    unsafe {
        if clear_event {
            let flags = (base_addr + STA_FLAGS_RVA) as *mut u32;
            *flags &= !STA_EVENT_MASK;
        }
        let request: extern "thiscall" fn(*mut u8, *const i8, u32) =
            std::mem::transmute(base_addr + REQUEST_SUBPHASE_RVA);
        request((base_addr + STATE_OBJECT_RVA) as *mut u8, cname.as_ptr(), arg);
    }
    Some(())
}

/// Смена фазы/подфазы. Вызывать только из render-цикла (поток игры):
/// движок не потокобезопасен.
pub(crate) fn change_phase(base_addr: usize, id: u32, arg2: i32, arg3: i32, mode: u32) {
    unsafe {
        let change_phase: extern "thiscall" fn(*mut u8, u32, i32, i32, u32) -> u32 =
            std::mem::transmute(base_addr + CHANGE_PHASE_RVA);
        change_phase((base_addr + PHASE_MANAGER_RVA) as *mut u8, id, arg2, arg3, mode);
    }
}
