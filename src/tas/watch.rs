//! Аппаратная точка останова на запись в адрес игры (`DR0` + VEH).
//!
//! Нужна, чтобы найти код, который пишет в поле (например, анимацию врага,
//! `Behavior + 0x618`): `POST /watch` взводит точку на запись по адресу, а
//! VEH (см. `lib.rs`) ловит `STATUS_SINGLE_STEP` и запоминает `Eip` писателя.
//!
//! Точка ставится на **все потоки процесса**: логику сцены (в т.ч. ИИ врагов)
//! игра может считать в рабочих потоках, а render-цикл (где обслуживается
//! заявка) — только в главном. Потоки из снапшота Toolhelp: свой — без
//! приостановки, чужие — `SuspendThread` → `SetThreadContext` → `ResumeThread`.
//!
//! Debug-only: VEH регистрируется только в debug-сборке.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{
    CONTEXT, CONTEXT_CONTROL_X86, CONTEXT_DEBUG_REGISTERS_X86, GetThreadContext, SetThreadContext,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
    THREAD_GET_CONTEXT, THREAD_SET_CONTEXT, THREAD_SUSPEND_RESUME,
};

/// `STATUS_SINGLE_STEP` — исключение от аппаратной точки останова.
pub(crate) const STATUS_SINGLE_STEP: u32 = 0x8000_0004;
/// Сколько уникальных пар (писатель, вызывающий) запомнить.
const MAX_UNIQUE: usize = 64;
/// `Behavior + 0x618` — поле текущей анимации (см. `game::player`).
pub(crate) const ENEMY_ANIM_OFFSET: usize = 0x618;
/// DR7: L0=1, RW0=01 (запись), LEN0=11 (4 байта).
const DR7_WRITE_4: u32 = 0x000D_0001;

static ARM_REQ: AtomicBool = AtomicBool::new(false);
static ARM_ADDR: AtomicUsize = AtomicUsize::new(0);
static DISARM_REQ: AtomicBool = AtomicBool::new(false);
static ARMED: AtomicBool = AtomicBool::new(false);
static WATCH_ADDR: AtomicUsize = AtomicUsize::new(0);
/// Сколько потоков получили точку (для ответа).
static ARMED_THREADS: AtomicUsize = AtomicUsize::new(0);
static HITS: [AtomicUsize; MAX_UNIQUE] = [const { AtomicUsize::new(0) }; MAX_UNIQUE];
/// Вызывающий (адрес возврата) для соответствующего `HITS[i]`: у сеттеров без
/// пролога на момент записи `[esp]` — это адрес возврата в вызывающую функцию.
static CALLERS: [AtomicUsize; MAX_UNIQUE] = [const { AtomicUsize::new(0) }; MAX_UNIQUE];
/// Цепочка возвратов со стека (кандидаты в вызывающих выше уровнем) для
/// `HITS[i]` — чтобы от сеттера дойти до самого решения.
const CHAIN_LEN: usize = 4;
static CHAINS: [[AtomicUsize; CHAIN_LEN]; MAX_UNIQUE] =
    [const { [const { AtomicUsize::new(0) }; CHAIN_LEN] }; MAX_UNIQUE];
/// Диапазон адресов модуля игры (код) — по нему фильтруем значения стека.
static MODULE_START: AtomicUsize = AtomicUsize::new(0);
static MODULE_END: AtomicUsize = AtomicUsize::new(0);
static NHITS: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicU64 = AtomicU64::new(0);
/// Адрес Behavior ближайшего врага — обновляется каждый кадр из
/// `read_nearest_enemy`, чтобы `POST /watch {"enemy": true, "off": "0x…"}` не
/// искал его сам.
static ENEMY_BEHAVIOR: AtomicUsize = AtomicUsize::new(0);

/// Обновляет адрес Behavior ближайшего врага (зовётся из чтения состояния).
pub(crate) fn set_enemy_behavior(addr: usize) {
    ENEMY_BEHAVIOR.store(addr, Ordering::Relaxed);
}

pub(crate) fn enemy_behavior() -> usize {
    ENEMY_BEHAVIOR.load(Ordering::Relaxed)
}

/// Заявка на взвод точки (обработается в `service()` на главном потоке).
pub(crate) fn request_arm(addr: usize) {
    ARM_ADDR.store(addr, Ordering::Relaxed);
    ARM_REQ.store(true, Ordering::Relaxed);
}

pub(crate) fn request_disarm() {
    DISARM_REQ.store(true, Ordering::Relaxed);
}

pub(crate) fn is_armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

pub(crate) fn watch_addr() -> usize {
    WATCH_ADDR.load(Ordering::Relaxed)
}

pub(crate) fn armed_threads() -> usize {
    ARMED_THREADS.load(Ordering::Relaxed)
}

pub(crate) fn total() -> u64 {
    TOTAL.load(Ordering::Relaxed)
}

/// Диапазон кода модуля игры — для фильтрации значений стека (зовётся из
/// `lib.rs` при инициализации).
pub(crate) fn set_module_range(start: usize, end: usize) {
    MODULE_START.store(start, Ordering::Relaxed);
    MODULE_END.store(end, Ordering::Relaxed);
}

pub(crate) fn module_range() -> (usize, usize) {
    (
        MODULE_START.load(Ordering::Relaxed),
        MODULE_END.load(Ordering::Relaxed),
    )
}

/// Уникальные пары `(Eip писателя, адрес возврата вызывающего, цепочка стека)`.
pub(crate) fn hits() -> Vec<(usize, usize, Vec<usize>)> {
    let n = NHITS.load(Ordering::Relaxed).min(MAX_UNIQUE);
    (0..n)
        .map(|i| {
            let chain = (0..CHAIN_LEN)
                .map(|k| CHAINS[i][k].load(Ordering::Relaxed))
                .filter(|v| *v != 0)
                .collect();
            (
                HITS[i].load(Ordering::Relaxed),
                CALLERS[i].load(Ordering::Relaxed),
                chain,
            )
        })
        .collect()
}

/// Обрабатывает заявки на взвод/снятие — вызывается из render-цикла.
pub(crate) fn service() {
    if ARM_REQ.swap(false, Ordering::Relaxed) {
        let addr = ARM_ADDR.load(Ordering::Relaxed);
        if addr != 0 {
            NHITS.store(0, Ordering::Relaxed);
            TOTAL.store(0, Ordering::Relaxed);
            // SAFETY: Dr-регистры ставим на потоки процесса по валидному адресу.
            let (ok, total) = unsafe { arm_all_threads(addr) };
            ARMED_THREADS.store(ok, Ordering::Relaxed);
            WATCH_ADDR.store(addr, Ordering::Relaxed);
            ARMED.store(ok > 0, Ordering::Relaxed);
            crate::logger::log_line(&format!(
                "watch: armed on write 0x{addr:08X} — {ok}/{total} потоков"
            ));
        }
    }
    if DISARM_REQ.swap(false, Ordering::Relaxed) {
        unsafe { clear_all_threads() };
        ARMED.store(false, Ordering::Relaxed);
        ARMED_THREADS.store(0, Ordering::Relaxed);
        crate::logger::log_line("watch: disarmed");
    }
}

/// VEH: наше ли это single-step. `true` — записали пару (Eip писателя, адрес
/// возврата вызывающего) и кандидатов-возвратов со стека; выполнение продолжаем.
/// Дедуп — по **паре**, а не по писателю: один сеттер зовётся из разных мест, и
/// интересны как раз разные вызывающие.
pub(crate) fn handle_single_step(eip: usize, caller: usize, chain: &[usize]) -> bool {
    if !ARMED.load(Ordering::Relaxed) {
        return false;
    }
    TOTAL.fetch_add(1, Ordering::Relaxed);
    let n = NHITS.load(Ordering::Relaxed);
    let mut known = false;
    for i in 0..n.min(MAX_UNIQUE) {
        if HITS[i].load(Ordering::Relaxed) == eip
            && CALLERS[i].load(Ordering::Relaxed) == caller
        {
            known = true;
            break;
        }
    }
    if !known && n < MAX_UNIQUE {
        HITS[n].store(eip, Ordering::Relaxed);
        CALLERS[n].store(caller, Ordering::Relaxed);
        for (k, v) in chain.iter().take(CHAIN_LEN).enumerate() {
            CHAINS[n][k].store(*v, Ordering::Relaxed);
        }
        NHITS.store(n + 1, Ordering::Relaxed);
    }
    true
}

/// Обходит потоки процесса и применяет `f` к каждому (свой — без приостановки).
unsafe fn for_each_thread(mut f: impl FnMut(HANDLE)) -> usize {
    let pid = unsafe { GetCurrentProcessId() };
    let me = unsafe { GetCurrentThreadId() };
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }) else {
        return 0;
    };
    let mut te = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut count = 0;
    if unsafe { Thread32First(snap, &mut te) }.is_ok() {
        loop {
            if te.th32OwnerProcessID == pid {
                count += 1;
                let acc = THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT;
                if let Ok(h) = unsafe { OpenThread(acc, false, te.th32ThreadID) } {
                    if te.th32ThreadID == me {
                        f(h);
                    } else if unsafe { SuspendThread(h) } != u32::MAX {
                        f(h);
                        unsafe { ResumeThread(h) };
                    }
                    let _ = unsafe { CloseHandle(h) };
                }
            }
            if unsafe { Thread32Next(snap, &mut te) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snap) };
    count
}

/// Ставит точку на все потоки; возвращает (сколько удалось, всего потоков).
unsafe fn arm_all_threads(addr: usize) -> (usize, usize) {
    let mut ok = 0;
    let total = unsafe {
        for_each_thread(|h| {
            if set_dr(h, addr) {
                ok += 1;
            }
        })
    };
    (ok, total)
}

/// Снимает точку со всех потоков.
unsafe fn clear_all_threads() {
    unsafe {
        for_each_thread(|h| {
            let _ = clear_dr(h);
        })
    };
}

/// Ставит DR0 на запись 4 байт по адресу на потоке `h` (для не-своего потока
/// `h` должен быть приостановлен).
unsafe fn set_dr(h: HANDLE, addr: usize) -> bool {
    let mut ctx: CONTEXT = unsafe { std::mem::zeroed() };
    ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS_X86 | CONTEXT_CONTROL_X86;
    if unsafe { GetThreadContext(h, &mut ctx) }.is_err() {
        return false;
    }
    ctx.Dr0 = addr as u32;
    ctx.Dr1 = 0;
    ctx.Dr2 = 0;
    ctx.Dr3 = 0;
    ctx.Dr6 = 0;
    ctx.Dr7 = DR7_WRITE_4;
    unsafe { SetThreadContext(h, &ctx) }.is_ok()
}

/// Снимает точку с потока `h`.
unsafe fn clear_dr(h: HANDLE) -> bool {
    let mut ctx: CONTEXT = unsafe { std::mem::zeroed() };
    ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS_X86 | CONTEXT_CONTROL_X86;
    if unsafe { GetThreadContext(h, &mut ctx) }.is_err() {
        return false;
    }
    ctx.Dr0 = 0;
    ctx.Dr6 = 0;
    ctx.Dr7 = 0;
    unsafe { SetThreadContext(h, &ctx) }.is_ok()
}
