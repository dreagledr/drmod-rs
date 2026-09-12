//! Тонкий FFI вокруг MinHook (C-исходники в `vendor/minhook`, BSD-2-Clause).
//! Взят из `vendor/hudhook/src/mh.rs` основного мода — только то, что нужно
//! для одного хука, без tracing.
#![allow(dead_code)]

use core::ffi::c_void;
use std::ptr::null_mut;

#[allow(non_camel_case_types)]
#[must_use]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MH_STATUS {
    /// Не должна возвращаться.
    MH_UNKNOWN = -1,
    MH_OK = 0,
    MH_ERROR_ALREADY_INITIALIZED,
    MH_ERROR_NOT_INITIALIZED,
    MH_ERROR_ALREADY_CREATED,
    MH_ERROR_NOT_CREATED,
    MH_ERROR_ENABLED,
    MH_ERROR_DISABLED,
    MH_ERROR_NOT_EXECUTABLE,
    MH_ERROR_UNSUPPORTED_FUNCTION,
    MH_ERROR_MEMORY_ALLOC,
    MH_ERROR_MEMORY_PROTECT,
    MH_ERROR_MODULE_NOT_FOUND,
    MH_ERROR_FUNCTION_NOT_FOUND,
}

unsafe extern "system" {
    fn MH_Initialize() -> MH_STATUS;
    fn MH_CreateHook(
        pTarget: *mut c_void,
        pDetour: *mut c_void,
        ppOriginal: *mut *mut c_void,
    ) -> MH_STATUS;
    fn MH_QueueEnableHook(pTarget: *mut c_void) -> MH_STATUS;
    fn MH_ApplyQueued() -> MH_STATUS;
}

/// Один хук: цель, наш детур и трамплин оригинала.
pub(crate) struct MhHook {
    addr: *mut c_void,
    trampoline: *mut c_void,
}

impl MhHook {
    /// Создаёт хук (ставить в очередь на включение — [`MhHook::queue_enable`]).
    ///
    /// # Safety
    ///
    /// `addr` — исполняемая память игры, `hook_impl` — функция с сигнатурой,
    /// совпадающей с целевой.
    pub(crate) unsafe fn new(
        addr: *mut c_void,
        hook_impl: *mut c_void,
    ) -> Result<Self, MH_STATUS> {
        let mut trampoline = null_mut();
        let status = unsafe { MH_CreateHook(addr, hook_impl, &mut trampoline) };
        if status != MH_STATUS::MH_OK {
            return Err(status);
        }
        Ok(Self { addr, trampoline })
    }

    /// Адрес трамплина оригинала (валиден только после успешного `new`).
    pub(crate) fn trampoline(&self) -> *mut c_void {
        self.trampoline
    }

    /// # Safety
    ///
    /// Ставит хук в очередь на включение; применится в [`MH_ApplyQueued`].
    pub(crate) unsafe fn queue_enable(&self) -> Result<(), MH_STATUS> {
        let status = unsafe { MH_QueueEnableHook(self.addr) };
        if status == MH_STATUS::MH_OK {
            Ok(())
        } else {
            Err(status)
        }
    }
}

/// Инициализация MinHook (ровно один раз на процесс).
///
/// # Safety
///
/// Вызывается один раз до создания хуков.
pub(crate) unsafe fn initialize() -> Result<(), MH_STATUS> {
    match unsafe { MH_Initialize() } {
        MH_STATUS::MH_OK | MH_STATUS::MH_ERROR_ALREADY_INITIALIZED => Ok(()),
        status => Err(status),
    }
}

/// Применяет очередь включения/выключения хуков.
///
/// # Safety
///
/// Вызывается после [`MhHook::queue_enable`].
pub(crate) unsafe fn apply_queued() -> Result<(), MH_STATUS> {
    let status = unsafe { MH_ApplyQueued() };
    if status == MH_STATUS::MH_OK {
        Ok(())
    } else {
        Err(status)
    }
}
