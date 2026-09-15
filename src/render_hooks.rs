//! Headless-режим прогонов: снять отрисовку, не потеряв логику.
//!
//! В drmod-rs вся покадровая логика (скрипты API, запись/воспроизведение,
//! трекинг сегмента) живёт в `HelloHud::render`, а он вызывается из хука DX9
//! `IDirect3DDevice9::Present` — рендер и логика идут в одном кадре. Поэтому
//! «headless» здесь не отдельный процесс, а три независимых выключателя
//! отрисовки, каждый из которых логику сохраняет:
//!
//! * `overlay` — мод не строит свой UI и не рисует 3D-маркеры (плюс hudhook не
//!   отправляет геометрию imgui в устройство). Логика в `render` — вся;
//! * `present` — не вызывается настоящий `Present`: кадр не блитится в окно,
//!   окно держит последнее показанное изображение. Игра результат `Present` не
//!   разбирает, кроме кода «устройство потеряно»;
//! * `draw` — вызовы отрисовки геометрии устройства (`DrawPrimitive*`)
//!   немедленно возвращают `D3D_OK`. Игра проходит весь свой кадровый код
//!   (обход сцены, состояния, `EndScene`, `Present`), но GPU ничего не считает.
//!   Это главный выигрыш: на 800×600 растрирование сцены — заметная часть
//!   времени итерации.
//!
//! Главный цикл игры подтверждён дизассемблированием (`docs/HEADLESS.md`):
//! одна итерация = `updateFrameTime` (`0xA03970`) → тик симуляции (`0xA4F560`,
//! внутри `0x61E8A0` → `updateInputUnit` ×4) → `EndScene` (`0xB9BE20`) →
//! кадровый рендер (`0x651080`) → пацер (`0xB98070`) → `Present` (`0xB97F90`).
//! То есть кадров в секунду = тиков в секунду, и снятие отрисовки ускоряет
//! прогон, не меняя подачу кадров скрипта (она идёт по тикам симуляции,
//! `api::feed_tick`).
//!
//! Все три выключателя по умолчанию выключены: обычная работа мода не меняется.

use core::ffi::c_void;
use std::mem::offset_of;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use hudhook::mh::{MH_ApplyQueued, MhHook};
use hudhook::windows::Win32::Graphics::Direct3D9::IDirect3DDevice9_Vtbl;

use crate::logger;

/// `D3D_OK` — код успеха, который возвращают заглушки отрисовки.
const D3D_OK: i32 = 0;

/// Указатель на `IDirect3DDevice9` в модуле игры (`base + 0x1B206D4`).
/// Подтверждено дважды: дизассемблированием (обёртка `Present` `0xB97F90` и
/// обёртка `EndScene` `0xB9BE20` берут устройство отсюда и зовут его vtable —
/// `[ecx+0x44]` = `Present`, `[ecx+0xA8]` = `EndScene`) и SDK
/// (`ref/mgr-plugin-sdk/game/Hw.h`: `Hw::GraphicDevice`).
const DEVICE_PTR: usize = 0x1B206D4;

/// Слот vtable `IDirect3DDevice9` по смещению поля в таблице windows-rs:
/// индекс считает компилятор, а не «магическое число» (порядок методов COM
/// фиксирован, но пусть за него отвечает `offset_of!`).
/// Значения проверены тестом `slots_match_disassembly`.
const fn vtable_slot_index(byte_offset: usize) -> usize {
    byte_offset / core::mem::size_of::<usize>()
}

const SLOT_DRAW_PRIMITIVE: usize =
    vtable_slot_index(offset_of!(IDirect3DDevice9_Vtbl, DrawPrimitive));
const SLOT_DRAW_INDEXED_PRIMITIVE: usize =
    vtable_slot_index(offset_of!(IDirect3DDevice9_Vtbl, DrawIndexedPrimitive));
const SLOT_DRAW_PRIMITIVE_UP: usize =
    vtable_slot_index(offset_of!(IDirect3DDevice9_Vtbl, DrawPrimitiveUP));
const SLOT_DRAW_INDEXED_PRIMITIVE_UP: usize =
    vtable_slot_index(offset_of!(IDirect3DDevice9_Vtbl, DrawIndexedPrimitiveUP));

/// Сигнатуры методов vtable. `HRESULT` — это `i32`, `D3DPRIMITIVETYPE` — `i32`,
/// `D3DFORMAT` — `u32` (новые типы windows-rs над теми же примитивами), поэтому
/// заглушки объявлены на примитивах: ABI совпадает, а лишних приведения нет.
type DrawPrimitiveFn = unsafe extern "system" fn(*mut c_void, i32, u32, u32) -> i32;
type DrawIndexedPrimitiveFn =
    unsafe extern "system" fn(*mut c_void, i32, i32, u32, u32, u32, u32) -> i32;
type DrawPrimitiveUpFn =
    unsafe extern "system" fn(*mut c_void, i32, u32, *const c_void, u32) -> i32;
type DrawIndexedPrimitiveUpFn = unsafe extern "system" fn(
    *mut c_void,
    i32,
    u32,
    u32,
    u32,
    *const c_void,
    u32,
    *const c_void,
    u32,
) -> i32;

/// Хотим ли пропускать отрисовку overlay мода (строить UI и 3D-маркеры).
static SKIP_OVERLAY: AtomicBool = AtomicBool::new(false);
/// Хотим ли пропускать отрисовку геометрии игры (заглушки на `DrawPrimitive*`).
static SKIP_DRAW: AtomicBool = AtomicBool::new(false);
/// Хотим ли пропускать кадровый рендер игры целиком (`0x651080`) — вместе с
/// CPU-частью (обход сцены, состояния), а не только с растеканием по GPU.
static SKIP_SCENE: AtomicBool = AtomicBool::new(false);
/// Попытка установки хуков отрисовки завершена: render-цикл по этому флагу
/// понимает, что повторять установку не нужно.
static DRAW_INSTALL_DONE: AtomicBool = AtomicBool::new(false);
/// Стоит ли хотя бы один хук отрисовки (для `GET /state`).
static DRAW_HOOKED: AtomicBool = AtomicBool::new(false);
/// Попытка установки хука кадрового рендера завершена.
static SCENE_INSTALL_DONE: AtomicBool = AtomicBool::new(false);
/// Стоит ли хук кадрового рендера (для `GET /state`).
static SCENE_HOOKED: AtomicBool = AtomicBool::new(false);

/// RVA кадрового рендера игры: зовётся ровно раз за итерацию главного цикла
/// (между `EndScene` и пацером), возврат — «кадр отрисован» (по нулю цикл может
/// завершиться, поэтому заглушка возвращает 1). Внутри — очистки, состояния и
/// отправка геометрии через обёртку D3D движка (`0xB9xxxx`), то есть вся
/// CPU-часть кадрового прохода, которую заглушки `DrawPrimitive*` не снимают.
/// Разбор вызвавшего цикла — `docs/HEADLESS.md` §1.1.
const FRAME_RENDER_RVA: usize = 0x651080;

/// Оригинал кадрового рендера (trampoline MinHook).
static FRAME_RENDER: OnceLock<unsafe extern "system" fn() -> i32> = OnceLock::new();

/// Оригиналы хуков отрисовки (trampoline MinHook); `None` — хук не поставлен,
/// вызов уходит в заглушку без оригинала.
static DRAW_PRIMITIVE: OnceLock<DrawPrimitiveFn> = OnceLock::new();
static DRAW_INDEXED_PRIMITIVE: OnceLock<DrawIndexedPrimitiveFn> = OnceLock::new();
static DRAW_PRIMITIVE_UP: OnceLock<DrawPrimitiveUpFn> = OnceLock::new();
static DRAW_INDEXED_PRIMITIVE_UP: OnceLock<DrawIndexedPrimitiveUpFn> = OnceLock::new();

/// Включён ли пропуск отрисовки overlay мода.
pub(crate) fn skip_overlay() -> bool {
    SKIP_OVERLAY.load(Ordering::Relaxed)
}

/// Включён ли пропуск отрисовки геометрии игры (заглушки на устройстве).
pub(crate) fn skip_draw() -> bool {
    SKIP_DRAW.load(Ordering::Relaxed)
}

/// Включён ли пропуск кадрового рендера игры целиком (`0x651080`).
pub(crate) fn skip_scene() -> bool {
    SKIP_SCENE.load(Ordering::Relaxed)
}

/// Запрошен ли пропуск кадрового рендера (по этому флагу render-цикл ставит хук).
pub(crate) fn scene_wanted() -> bool {
    SKIP_SCENE.load(Ordering::Relaxed)
}

/// Завершена ли попытка установки хука кадрового рендера.
pub(crate) fn scene_install_done() -> bool {
    SCENE_INSTALL_DONE.load(Ordering::Relaxed)
}

/// Стоит ли хук кадрового рендера (для `GET /state`).
pub(crate) fn scene_hooked() -> bool {
    SCENE_HOOKED.load(Ordering::Relaxed)
}

/// Запрошен ли пропуск отрисовки геометрии (по этому флагу render-цикл ставит
/// хуки — в потоке игры, см. `DrawHooks::install`).
pub(crate) fn draw_wanted() -> bool {
    SKIP_DRAW.load(Ordering::Relaxed)
}

/// Завершена ли попытка установки хуков отрисовки (успешная или нет — повторять
/// не нужно, ошибки уже в логе).
pub(crate) fn install_done() -> bool {
    DRAW_INSTALL_DONE.load(Ordering::Relaxed)
}

/// Стоит ли хотя бы один хук отрисовки (для `GET /state`).
pub(crate) fn draw_hooked() -> bool {
    DRAW_HOOKED.load(Ordering::Relaxed)
}

/// Включён ли пропуск настоящего `Present` (флаг живёт в hudhook — он владеет
/// хуком `Present`).
pub(crate) fn skip_present() -> bool {
    hudhook::skip_present()
}

/// Снимок выключателей: `(overlay, present, draw, scene)` — «пропускать отрисовку».
pub(crate) fn state() -> (bool, bool, bool, bool) {
    (
        skip_overlay(),
        skip_present(),
        skip_draw(),
        skip_scene(),
    )
}

/// Общий сеттер для `POST /render` и меню Settings: `None` — не трогать
/// выключатель. Пишет ровно тот же runtime-стейт, что и HTTP-ручка, — источник
/// истины один.
pub(crate) fn set_skip(
    overlay: Option<bool>,
    present: Option<bool>,
    draw: Option<bool>,
    scene: Option<bool>,
) {
    if let Some(v) = overlay {
        SKIP_OVERLAY.store(v, Ordering::SeqCst);
        // Геометрию imgui тоже не отправляем: даже если UI почему-то собран,
        // в устройство он не пойдёт.
        hudhook::set_skip_draw(v);
    }
    if let Some(v) = present {
        hudhook::set_skip_present(v);
    }
    if let Some(v) = draw {
        SKIP_DRAW.store(v, Ordering::SeqCst);
    }
    if let Some(v) = scene {
        SKIP_SCENE.store(v, Ordering::SeqCst);
    }
}

/// Хуки отрисовки на vtable устройства игры. Живут в `HelloHud` (как и
/// `input_hooks`): MinHook не снимает хук при выпадении владельца, но поле
/// делает владение явным.
pub(crate) struct DrawHooks {
    #[allow(dead_code)] // keep-alive: поле не читается, хуки живут до выгрузки DLL
    hooks: Vec<MhHook>,
}

impl DrawHooks {
    /// Ставит заглушки на отрисовку геометрии устройства.
    ///
    /// Зовётся **только из render-цикла** (поток игры): к этому моменту
    /// устройство создано, и мы патчим код не в момент его исполнения. Ставить
    /// хуки из HTTP-потока нельзя — MinHook перезаписывает пролог функции,
    /// которая в этот момент может исполняться в игре.
    ///
    /// Хуки ставятся ровно один раз и больше не снимаются: выключение `draw`
    /// оставляет их пробросом в оригинал (одна лишняя косвенная ветка на вызов
    /// отрисовки — плата за мгновенное включение обратно).
    ///
    /// Если устройства ещё нет (vtable недоступна), возвращает пустой набор и
    /// не помечает попытку завершённой — render-цикл повторит в следующем кадре.
    pub(crate) fn install(base_addr: usize) -> Self {
        if base_addr == 0 || vtable_slot(base_addr, SLOT_DRAW_INDEXED_PRIMITIVE).is_none() {
            return Self { hooks: Vec::new() };
        }
        let mut hooks = Vec::new();
        unsafe {
            if let Some(hook) = hook_slot(
                base_addr,
                SLOT_DRAW_INDEXED_PRIMITIVE,
                "DrawIndexedPrimitive",
                draw_indexed_primitive_detour as *mut c_void,
            ) {
                let orig: DrawIndexedPrimitiveFn = std::mem::transmute(hook.trampoline());
                let _ = DRAW_INDEXED_PRIMITIVE.set(orig);
                hooks.push(hook);
            }
            if let Some(hook) = hook_slot(
                base_addr,
                SLOT_DRAW_PRIMITIVE,
                "DrawPrimitive",
                draw_primitive_detour as *mut c_void,
            ) {
                let orig: DrawPrimitiveFn = std::mem::transmute(hook.trampoline());
                let _ = DRAW_PRIMITIVE.set(orig);
                hooks.push(hook);
            }
            if let Some(hook) = hook_slot(
                base_addr,
                SLOT_DRAW_INDEXED_PRIMITIVE_UP,
                "DrawIndexedPrimitiveUP",
                draw_indexed_primitive_up_detour as *mut c_void,
            ) {
                let orig: DrawIndexedPrimitiveUpFn = std::mem::transmute(hook.trampoline());
                let _ = DRAW_INDEXED_PRIMITIVE_UP.set(orig);
                hooks.push(hook);
            }
            if let Some(hook) = hook_slot(
                base_addr,
                SLOT_DRAW_PRIMITIVE_UP,
                "DrawPrimitiveUP",
                draw_primitive_up_detour as *mut c_void,
            ) {
                let orig: DrawPrimitiveUpFn = std::mem::transmute(hook.trampoline());
                let _ = DRAW_PRIMITIVE_UP.set(orig);
                hooks.push(hook);
            }
        }
        DRAW_HOOKED.store(!hooks.is_empty(), Ordering::Relaxed);
        DRAW_INSTALL_DONE.store(true, Ordering::Relaxed);
        logger::log_line(&format!(
            "render: хуки отрисовки поставлены ({}/4) — пропуск геометрии игры",
            hooks.len()
        ));
        Self { hooks }
    }
}

/// Хук кадрового рендера игры (`0x651080`) — экспериментальный выключатель
/// `skip_scene`. Живёт в `HelloHud` как владелец (как `DrawHooks`).
pub(crate) struct SceneHook {
    #[allow(dead_code)] // keep-alive: хук живёт до выгрузки DLL
    hook: Option<MhHook>,
}

impl SceneHook {
    /// Ставит хук на кадровый рендер игры.
    ///
    /// Зовётся **только из render-цикла** (поток игры) — как и `DrawHooks::install`:
    /// из HTTP-потока патчить пролог исполняемой функции нельзя.
    ///
    /// ⚠️ Хук уносит **весь** кадровый проход движка (не только GPU): обход сцены,
    /// состояния, очередь кадра. Это агрессивнее заглушек `DrawPrimitive*` и по
    /// сайд-эффектам ближе к связке `draw`+`present`, поэтому отдельный флаг.
    pub(crate) fn install(base_addr: usize) -> Self {
        if base_addr == 0 {
            return Self { hook: None };
        }
        let target = (base_addr + FRAME_RENDER_RVA) as *mut c_void;
        let hook = match unsafe { MhHook::new(target, frame_render_detour as *mut c_void) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!("render: кадровый рендер 0x{FRAME_RENDER_RVA:X}: \
                                           MH_CreateHook FAIL {e:?}"));
                SCENE_INSTALL_DONE.store(true, Ordering::Relaxed);
                return Self { hook: None };
            }
        };
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!("render: кадровый рендер: queue_enable FAIL {e:?}"));
            SCENE_INSTALL_DONE.store(true, Ordering::Relaxed);
            return Self { hook: None };
        }
        let _ = unsafe { MH_ApplyQueued() };
        let orig: unsafe extern "system" fn() -> i32 = unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = FRAME_RENDER.set(orig);
        SCENE_HOOKED.store(true, Ordering::Relaxed);
        SCENE_INSTALL_DONE.store(true, Ordering::Relaxed);
        logger::log_line(&format!(
            "render: кадровый рендер игры → заглушка (base+0x{FRAME_RENDER_RVA:X} = 0x{:08X}, \
             trampoline 0x{:08X}) — снятие CPU-части кадрового прохода",
            base_addr + FRAME_RENDER_RVA,
            hook.trampoline() as usize
        ));
        Self { hook: Some(hook) }
    }
}

/// Заглушка кадрового рендера: `1` = «кадр отрисован» (нулевой возврат заставил
/// бы главный цикл завершиться).
unsafe extern "system" fn frame_render_detour() -> i32 {
    if skip_scene() {
        return 1;
    }
    match FRAME_RENDER.get() {
        Some(&orig) => unsafe { orig() },
        None => 1,
    }
}

/// Адрес метода устройства из живой vtable игры. `None` — устройство (или его
/// vtable) ещё не создано.
fn vtable_slot(base_addr: usize, slot: usize) -> Option<*mut c_void> {
    if base_addr == 0 {
        return None;
    }
    unsafe {
        let device = *((base_addr + DEVICE_PTR) as *const *const usize);
        if device.is_null() {
            return None;
        }
        let vtable = *device;
        if vtable == 0 {
            return None;
        }
        Some(*((vtable as *const usize).add(slot)) as *mut c_void)
    }
}

/// Ставит один MinHook на метод устройства и сразу его включает.
unsafe fn hook_slot(
    base_addr: usize,
    slot: usize,
    name: &str,
    detour: *mut c_void,
) -> Option<MhHook> {
    let target = vtable_slot(base_addr, slot)?;
    let hook = match unsafe { MhHook::new(target, detour) } {
        Ok(h) => h,
        Err(e) => {
            logger::log_line(&format!(
                "render: {name} (slot {slot}, target 0x{:08X}): MH_CreateHook FAIL {e:?}",
                target as usize
            ));
            return None;
        }
    };
    if let Err(e) = unsafe { hook.queue_enable() } {
        logger::log_line(&format!("render: {name}: queue_enable FAIL {e:?}"));
        return None;
    }
    let _ = unsafe { MH_ApplyQueued() };
    logger::log_line(&format!(
        "render: {name} → заглушка (slot {slot}, target 0x{:08X}, trampoline 0x{:08X})",
        target as usize,
        hook.trampoline() as usize
    ));
    Some(hook)
}

unsafe extern "system" fn draw_primitive_detour(
    device: *mut c_void,
    primitive_type: i32,
    start_vertex: u32,
    primitive_count: u32,
) -> i32 {
    if skip_draw() {
        return D3D_OK;
    }
    match DRAW_PRIMITIVE.get() {
        Some(&orig) => unsafe { orig(device, primitive_type, start_vertex, primitive_count) },
        None => D3D_OK,
    }
}

unsafe extern "system" fn draw_indexed_primitive_detour(
    device: *mut c_void,
    primitive_type: i32,
    base_vertex_index: i32,
    min_vertex_index: u32,
    num_vertices: u32,
    start_index: u32,
    primitive_count: u32,
) -> i32 {
    if skip_draw() {
        return D3D_OK;
    }
    match DRAW_INDEXED_PRIMITIVE.get() {
        Some(&orig) => unsafe {
            orig(
                device,
                primitive_type,
                base_vertex_index,
                min_vertex_index,
                num_vertices,
                start_index,
                primitive_count,
            )
        },
        None => D3D_OK,
    }
}

unsafe extern "system" fn draw_primitive_up_detour(
    device: *mut c_void,
    primitive_type: i32,
    primitive_count: u32,
    vertex_data: *const c_void,
    vertex_stride: u32,
) -> i32 {
    if skip_draw() {
        return D3D_OK;
    }
    match DRAW_PRIMITIVE_UP.get() {
        Some(&orig) => unsafe {
            orig(
                device,
                primitive_type,
                primitive_count,
                vertex_data,
                vertex_stride,
            )
        },
        None => D3D_OK,
    }
}

unsafe extern "system" fn draw_indexed_primitive_up_detour(
    device: *mut c_void,
    primitive_type: i32,
    min_vertex_index: u32,
    num_vertices: u32,
    primitive_count: u32,
    index_data: *const c_void,
    index_format: u32,
    vertex_data: *const c_void,
    vertex_stride: u32,
) -> i32 {
    if skip_draw() {
        return D3D_OK;
    }
    match DRAW_INDEXED_PRIMITIVE_UP.get() {
        Some(&orig) => unsafe {
            orig(
                device,
                primitive_type,
                min_vertex_index,
                num_vertices,
                primitive_count,
                index_data,
                index_format,
                vertex_data,
                vertex_stride,
            )
        },
        None => D3D_OK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Слоты vtable должны совпадать с теми, что читает сама игра: смещение
    /// `Present` — `0x44` (обёртка `0xB97F90`), `EndScene` — `0xA8` (обёртка
    /// `0xB9BE20`). Оба подтверждены дизассемблированием; здесь это страховка
    /// от смены порядка полей в биндингах windows-rs.
    #[test]
    fn slots_match_disassembly() {
        assert_eq!(
            vtable_slot_index(offset_of!(IDirect3DDevice9_Vtbl, Present)),
            0x44 / 4
        );
        assert_eq!(
            vtable_slot_index(offset_of!(IDirect3DDevice9_Vtbl, EndScene)),
            0xA8 / 4
        );
        assert_eq!(SLOT_DRAW_PRIMITIVE, 0x144 / 4);
        assert_eq!(SLOT_DRAW_INDEXED_PRIMITIVE, 0x148 / 4);
        assert_eq!(SLOT_DRAW_PRIMITIVE_UP, 0x14C / 4);
        assert_eq!(SLOT_DRAW_INDEXED_PRIMITIVE_UP, 0x150 / 4);
    }

    /// Выключатели — общий runtime-стейт для HTTP и меню: `None` не трогает
    /// поле, `false` возвращает обычную работу.
    #[test]
    fn skip_flags_round_trip() {
        let before = state();
        set_skip(Some(true), Some(true), None, Some(true));
        assert_eq!(state(), (true, true, before.2, true));
        set_skip(None, None, Some(true), Some(false));
        assert!(skip_draw() && draw_wanted());
        assert!(!skip_scene() && !scene_wanted());
        set_skip(Some(false), Some(false), Some(false), Some(false));
        assert_eq!(state(), (false, false, false, false));
        set_skip(Some(before.0), Some(before.1), Some(before.2), Some(before.3));
    }

    /// Без базы модуля адрес устройства не читаем вообще: `None` (а не
    /// разыменование по мусорному адресу) — на этом стоит `DrawHooks::install`.
    #[test]
    fn vtable_slot_without_base_is_none() {
        assert!(vtable_slot(0, SLOT_DRAW_INDEXED_PRIMITIVE).is_none());
    }
}
