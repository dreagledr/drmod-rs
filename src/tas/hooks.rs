//! Хуки ввода (MinHook): подача InputUnit через `updateInputUnit`,
//! эмуляция ripper/blade через `isKeybindPressed`/`isKeybindDown`,
//! чтение сырого ввода (клавиатура/мышь).

use hudhook::mh::{MH_ApplyQueued, MhHook};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

use super::addresses;
use super::replay;
use super::types;
use crate::logger;

/// cInput::ms_KeyInput — адрес сырого ввода клавиатуры (вычисляется в `install`).
static KEY_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// База модуля игры (сохранена для чтения GameMenuStatus из детуров).
static BASE_ADDR: AtomicUsize = AtomicUsize::new(0);
/// Битмап кодов клавиш, нажатия которых игра увидела в меню (результат != 0):
/// каждый код логируется один раз — так видно, какие коды реально приходят
/// с устройства (и, значит, что можно подавать для меню).
static MENU_PRESS_SEEN: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
/// cInput::ms_MouseInput — адрес сырого ввода мыши (вычисляется в `install`).
static MOUSE_INPUT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// cInput::ms_bUpdateKeyboard — флаг автообновления кэша клавиш из DirectInput.
/// Замораживается на время подачи raw-клавиш меню (иначе DirectInput
/// перезапишет наши значения в `ms_KeyInput`).
static UPDATE_KEYBOARD_ADDR: AtomicUsize = AtomicUsize::new(0);
/// Trampoline оригинальной `cInput::updateInputUnit` (устанавливается в `create_input_hook`).
static ORIG_UPDATE_INPUT_UNIT: OnceLock<unsafe extern "C" fn(*mut types::InputUnit, i32)> =
    OnceLock::new();

/// Сколько разных вызывающих геттера времени запоминаем. Планировщик шагов
/// симуляции читает время каждый кадр — его адрес и ищем среди них.
const TIME_CALLERS_MAX: usize = 24;
/// Адреса возврата вызовов `cTime::getTicks` (0 — свободный слот).
static TIME_CALLERS: [AtomicUsize; TIME_CALLERS_MAX] =
    [const { AtomicUsize::new(0) }; TIME_CALLERS_MAX];
/// Счётчики вызовов по слотам `TIME_CALLERS`.
static TIME_CALLER_HITS: [AtomicU64; TIME_CALLERS_MAX] =
    [const { AtomicU64::new(0) }; TIME_CALLERS_MAX];
/// Трамплин оригинального `getTicks`: naked-детур уходит в него хвостом.
static ORIG_TIME_TICKS: AtomicUsize = AtomicUsize::new(0);

/// Счётчик шагов для синтетических часов: инкрементится в детуре
/// `updateInputUnit` (наш шаг = один вызов ввода), то есть один шаг на кадр.
static SYNTH_STEPS: AtomicU64 = AtomicU64::new(0);
/// Шагов на момент включения синтетики и значение часов в тот момент: чтобы
/// синтетические часы продолжали реальные без скачка (потребители считают
/// разности от старта игры).
static SYNTH_BASE_STEPS: AtomicU64 = AtomicU64::new(0);
static SYNTH_BASE_TICKS: AtomicU64 = AtomicU64::new(0);
/// Ближайшее значение синтетических часов (edx:eax) для naked-детура.
static SYNTH_LO: AtomicUsize = AtomicUsize::new(0);
static SYNTH_HI: AtomicUsize = AtomicUsize::new(0);
/// Включены ли синтетические часы (`POST /dt {"steps": true}`): каждый шаг
/// двигает символьное время ровно на 16.667 мс, независимо от реального FPS.
static SYNTH_ENABLED: AtomicU32 = AtomicU32::new(0);

/// Хук `Sleep` (kernel32) для ровной сетки кадров: пацерам игры (их адреса
/// возврата известны) выдаём задержку до ближайшего дедлайна сетки — тогда
/// число кадров в секунду постоянно, а значит и подброс (он считается по
/// кадрам × дельта окна замедления риппера) перестаёт плавать.
static ORIG_SLEEP: AtomicUsize = AtomicUsize::new(0);
static SLEEP_LIMIT_ON: AtomicU32 = AtomicU32::new(0);
/// Период сетки кадров в тиках QPC (0 — не задан).
static FRAME_PERIOD_TICKS: AtomicU64 = AtomicU64::new(0);
/// Целевой темп сетки, fps (f32 в битах) — для `/state`.
static FRAME_LIMIT_FPS_BITS: AtomicU32 = AtomicU32::new(0);
/// Дедлайн следующего кадра (тики QPC).
static FRAME_DEADLINE: AtomicU64 = AtomicU64::new(0);
/// Вызовы `Sleep` из пацеров кадров (RVA возврата): `0xB980B2` — спин первого
/// пацера (`Sleep(остаток/3)`), `0x9F2A8E` — второй пацер (`Sleep(вычисленное)`).
const PACER_SLEEP_CALLERS: [usize; 2] = [0xB980B2, 0x9F2A8E];
/// RVA слота IAT для `Sleep` в exe (из дизассемблера: `mov ebx, [0x01B2D1BC]`
/// при base 0x930000 → RVA 0x11FD1BC). По нему берём адрес самой функции.
const SLEEP_IAT_RVA: usize = 0x11FD1BC;
/// Адрес функции считаем системным, если он выше этого порога: адреса модулей
/// игры лежат около 0x00930000, системные DLL — в 0x7xxxxxxx. Защита от
/// неверного слота IAT (однажды хукнули не ту функцию — игра повисла).
const SYSTEM_ADDR_MIN: usize = 0x60000000;
/// Срабатываний синтетической ветки и последнее отданное значение — для
/// проверки, что она действительно используется (`/state` → `dt`).
static SYNTH_RETURNS: AtomicU64 = AtomicU64::new(0);
static SYNTH_LAST: AtomicU64 = AtomicU64::new(0);

/// Вызовы времени из пацеров кадров (RVA возврата): им нужны РЕАЛЬНЫЕ часы,
/// иначе они зациклятся/заснут на «замёрзшем» времени. Первый пацер —
/// `0xB980B7` (спин в цикле `Sleep`) и `0xB98008` (замер кадра внутри
/// `getTimeMs`); второй — `0x9F2A57`/`0x9F2A6A` (меряет кадр и зовёт
/// `Sleep(вычисленное)`).
///
/// ⚠️ Все зарегистрированные вызывающие сырого геттера — это пацеры (проверено
/// 2026-09-11), поэтому синтетические часы через него до симуляции не доходят:
/// время симуляции идёт через `cSlowRateManager::m_fTickDifference` (его мы
/// фиксируем в `POST /dt`) и реальное накопление. Точечная точка вставки —
/// писатель `m_fTickDifference`, а не геттер.
const PACER_CALLERS: [usize; 4] = [0xB980B7, 0xB98008, 0x9F2A57, 0x9F2A6A];
/// RVA глобалов модуля времени (см. `tools/disasm/README.md`): частота QPC и
/// «секунд на тик». Считаем по ним длину шага в тиках.
const TIME_QPF_RVA: usize = 0x19D4F20;
const TIME_SEC_PER_TICK_RVA: usize = 0x19D4F14;
/// Номинал движка: 60 шагов в символьной секунде.
const STEP_HZ: f64 = 60.0;

/// Решает, отдавать ли вызывающему синтетические часы вместо реальных.
/// `true` — в `SYNTH_LO/HI` лежит значение для edx:eax; `false` — пусть идёт в
/// оригинал (пацер кадров).
extern "C" fn synth_time(caller: usize) -> u32 {
    if SYNTH_ENABLED.load(Ordering::Relaxed) == 0 {
        return 0;
    }
    let base = BASE_ADDR.load(Ordering::Relaxed);
    let rva = caller.wrapping_sub(base);
    if PACER_CALLERS.contains(&rva) {
        return 0;
    }
    if base == 0 {
        return 0;
    }
    // Тиков QPC на шаг: частота / 60 (если частота ещё не прочитана — по
    // «секунд на тик»).
    let mut ticks_per_step = 0.0f64;
    let freq = unsafe { *((base + TIME_QPF_RVA) as *const u64) };
    if freq != 0 {
        ticks_per_step = freq as f64 / STEP_HZ;
    } else {
        let spt = unsafe { *((base + TIME_SEC_PER_TICK_RVA) as *const f32) };
        if spt > 0.0 {
            ticks_per_step = (1.0 / 60.0 / spt as f64).round();
        }
    }
    if ticks_per_step < 1.0 {
        return 0;
    }
    let steps = SYNTH_STEPS.load(Ordering::Relaxed);
    let base_steps = SYNTH_BASE_STEPS.load(Ordering::Relaxed);
    let base_ticks = SYNTH_BASE_TICKS.load(Ordering::Relaxed);
    let now = base_ticks
        .saturating_add(((steps.saturating_sub(base_steps)) as f64 * ticks_per_step) as u64);
    SYNTH_LO.store(now as u32 as usize, Ordering::Relaxed);
    SYNTH_HI.store((now >> 32) as u32 as usize, Ordering::Relaxed);
    SYNTH_RETURNS.fetch_add(1, Ordering::Relaxed);
    SYNTH_LAST.store(now, Ordering::Relaxed);
    1
}

/// Регистрирует адрес возврата и синтетические часы (вызывается из детура).
extern "C" fn time_caller_record(addr: usize) {
    for i in 0..TIME_CALLERS_MAX {
        let slot = TIME_CALLERS[i].load(Ordering::Relaxed);
        if slot == addr {
            TIME_CALLER_HITS[i].fetch_add(1, Ordering::Relaxed);
            return;
        }
        if slot == 0
            && TIME_CALLERS[i]
                .compare_exchange(0, addr, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            TIME_CALLER_HITS[i].fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
}

/// Детур геттера сырых тиков времени (`base + 0x9F8230`). Naked — адрес возврата
/// надо взять из стека до того, как компилятор построит свой кадр; дальше
/// хвостовой `jmp` в оригинал, поэтому edx:eax (результат) уходит вызывающему
/// без изменений.
#[unsafe(naked)]
unsafe extern "C" fn time_ticks_detour() -> u64 {
    core::arch::naked_asm!(
        "mov edx, [esp]",           // адрес возврата в вызывающего
        "push edx",                 // аргумент для регистратора/решателя (cdecl)
        "push edx",                 // выравнивание стека под SSE
        "call {record}",
        "call {synth}",             // eax != 0 → отдаём синтетические часы
        "add esp, 8",
        "test eax, eax",
        "jz 2f",
        "mov eax, [{lo}]",          // синтетика: edx:eax = база + шаг × 16.667 мс
        "mov edx, [{hi}]",
        "ret",
        "2:",
        "jmp dword ptr [{orig}]",   // иначе хвостом в оригинал (пацер кадров)
        record = sym time_caller_record,
        synth = sym synth_time,
        lo = sym SYNTH_LO,
        hi = sym SYNTH_HI,
        orig = sym ORIG_TIME_TICKS,
    )
}

/// Включает/выключает синтетические часы («шаг = 16.667 мс»). Возвращает
/// предыдущее состояние. `real_ticks` — текущее значение реальных часов (по
/// нему синтетика продолжается без скачка).
pub(crate) fn set_synthetic_clock(on: bool) -> bool {
    let was = SYNTH_ENABLED.swap(if on { 1 } else { 0 }, Ordering::Relaxed) != 0;
    if on && !was {
        // Стартуем от реальных часов: читаем их оригинальным геттером.
        let orig = ORIG_TIME_TICKS.load(Ordering::Relaxed);
        let base_ticks = if orig != 0 {
            let f: unsafe extern "C" fn() -> u64 = unsafe { std::mem::transmute(orig) };
            unsafe { f() }
        } else {
            0
        };
        SYNTH_BASE_TICKS.store(base_ticks, Ordering::Relaxed);
        SYNTH_BASE_STEPS.store(SYNTH_STEPS.load(Ordering::Relaxed), Ordering::Relaxed);
        SYNTH_LO.store(base_ticks as u32 as usize, Ordering::Relaxed);
        SYNTH_HI.store((base_ticks >> 32) as u32 as usize, Ordering::Relaxed);
    }
    was
}

/// Шаг симуляции для синтетических часов (вызывается из детура ввода).
pub(crate) fn note_step() {
    SYNTH_STEPS.fetch_add(1, Ordering::Relaxed);
}

/// Идут ли сейчас синтетические часы (для `/state`).
pub(crate) fn synthetic_clock_on() -> bool {
    SYNTH_ENABLED.load(Ordering::Relaxed) != 0
}

/// Тики игрового таймера (через оригинальный геттер) и частота QPC.
fn game_ticks() -> u64 {
    let orig = ORIG_TIME_TICKS.load(Ordering::Relaxed);
    if orig == 0 {
        return 0;
    }
    let f: unsafe extern "C" fn() -> u64 = unsafe { std::mem::transmute(orig) };
    unsafe { f() }
}

fn qpc_freq() -> u64 {
    let base = BASE_ADDR.load(Ordering::Relaxed);
    if base == 0 {
        return 0;
    }
    unsafe { *((base + TIME_QPF_RVA) as *const u64) }
}

/// Включает/выключает ровную сетку кадров (`fps` = целевой темп, 0 — выключить).
/// Период считается в тиках QPC по частоте движка.
pub(crate) fn set_frame_limit(fps: f32) -> bool {
    let freq = qpc_freq();
    if fps <= 0.0 || freq == 0 {
        SLEEP_LIMIT_ON.store(0, Ordering::Relaxed);
        FRAME_PERIOD_TICKS.store(0, Ordering::Relaxed);
        FRAME_LIMIT_FPS_BITS.store(0, Ordering::Relaxed);
        return false;
    }
    let period = (freq as f64 / fps as f64).round() as u64;
    FRAME_PERIOD_TICKS.store(period.max(1), Ordering::Relaxed);
    FRAME_LIMIT_FPS_BITS.store(fps.to_bits(), Ordering::Relaxed);
    FRAME_DEADLINE.store(0, Ordering::Relaxed);
    SLEEP_LIMIT_ON.store(1, Ordering::Relaxed);
    true
}

/// Идёт ли сейчас ровная сетка кадров.
pub(crate) fn frame_limit_on() -> bool {
    SLEEP_LIMIT_ON.load(Ordering::Relaxed) != 0
}

/// Целевой темп сетки кадров (0 — выключена).
pub(crate) fn frame_limit_fps() -> f32 {
    f32::from_bits(FRAME_LIMIT_FPS_BITS.load(Ordering::Relaxed))
}

/// Решает, сколько спать вызывающему `Sleep`: пацерам кадров выдаём задержку до
/// ближайшего дедлайна сетки, остальным — сколько просили.
extern "C" fn pacer_sleep_ms(caller: usize, ms: u32) -> u32 {
    if SLEEP_LIMIT_ON.load(Ordering::Relaxed) == 0 {
        return ms;
    }
    let period = FRAME_PERIOD_TICKS.load(Ordering::Relaxed);
    if period == 0 {
        return ms;
    }
    let rva = caller.wrapping_sub(BASE_ADDR.load(Ordering::Relaxed));
    if !PACER_SLEEP_CALLERS.contains(&rva) {
        return ms;
    }
    let now = game_ticks();
    if now == 0 {
        return ms;
    }
    let deadline = FRAME_DEADLINE.load(Ordering::Relaxed);
    if deadline == 0 || deadline <= now {
        // Старт сетки или отстали: следующий дедлайн от текущего кадра.
        FRAME_DEADLINE.store(now + period, Ordering::Relaxed);
        return 0;
    }
    FRAME_DEADLINE.store(deadline + period, Ordering::Relaxed);
    let freq = qpc_freq();
    if freq == 0 {
        return ms;
    }
    let wait_ms = ((deadline - now) as f64 * 1000.0 / freq as f64).ceil() as u32;
    wait_ms.min(200)
}

/// Детур `Sleep` (stdcall). Naked: берём адрес возврата и аргумент до кадра
/// компилятора, спрашиваем у решателя фактическую задержку и зовём оригинал.
#[unsafe(naked)]
unsafe extern "system" fn sleep_detour(_ms: u32) {
    core::arch::naked_asm!(
        "mov eax, [esp]",           // адрес возврата в вызывающего
        "mov edx, [esp+4]",         // запрошенные миллисекунды (stdcall)
        "push eax",
        "push edx",
        "call {helper}",            // -> eax: сколько спать на самом деле
        "add esp, 8",
        "push eax",
        "call dword ptr [{orig}]",  // оригинальный Sleep (сам снимет аргумент)
        "ret",
        helper = sym pacer_sleep_ms,
        orig = sym ORIG_SLEEP,
    )
}

/// Диагностика синтетических часов: (срабатываний, последнее значение).
pub(crate) fn synthetic_clock_stats() -> (u64, u64) {
    (
        SYNTH_RETURNS.load(Ordering::Relaxed),
        SYNTH_LAST.load(Ordering::Relaxed),
    )
}

/// Снимок вызывающих геттер времени: (адрес возврата, счётчик), по убыванию.
pub(crate) fn time_callers() -> Vec<(usize, u64)> {
    let mut v = Vec::new();
    for i in 0..TIME_CALLERS_MAX {
        let addr = TIME_CALLERS[i].load(Ordering::Relaxed);
        if addr != 0 {
            v.push((addr, TIME_CALLER_HITS[i].load(Ordering::Relaxed)));
        }
    }
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}
/// Trampoline оригинальной `cInput::isKeybindPressed`.
static ORIG_IS_KEYBIND_PRESSED: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
/// Trampoline оригинальной `cInput::isKeybindDown`.
static ORIG_IS_KEYBIND_DOWN: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
/// Trampoline оригинальной `KeyInput::isKeyDown` (thiscall, 0x9D93A0).
/// Первый аргумент — `this` (KeyInput*), второй — vKey.
static ORIG_IS_KEY_DOWN: OnceLock<unsafe extern "thiscall" fn(*const u8, i32) -> i32> =
    OnceLock::new();
/// Trampoline оригинальной `KeyInput::isKeyPressed` (thiscall, 0x9D9400).
static ORIG_IS_KEY_PRESSED: OnceLock<unsafe extern "thiscall" fn(*const u8, i32) -> i32> =
    OnceLock::new();
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
/// true, если мы записали свои биты в `ms_KeyInput` (чтобы вернуть нули и не
/// оставить «залипшую» клавишу — в паузе игра кэш сама не обновляет).
static RAW_KEYS_IN_CACHE: AtomicU32 = AtomicU32::new(0);
/// Сэмпл реального удержания blade (keybind 8) за кадр — результат оригинала
/// `isKeybindDown`, накопленный детуром. Читается записью в render.
#[cfg(debug_assertions)]
static BLADE_DOWN_SAMPLED: AtomicU32 = AtomicU32::new(0);
/// Сэмпл реального фронта ripper (keybind 11) за кадр — результат оригинала
/// `isKeybindPressed`, накопленный детуром. Читается записью в render.
#[cfg(debug_assertions)]
static RIPPER_PRESSED_SAMPLED: AtomicU32 = AtomicU32::new(0);
/// Сэмпл реальных сырых клавиш меню за кадр (`ms_KeyInput.m_aKeysDown`) —
/// накоплен детуром `updateInputUnit` ПОСЛЕ оригинала (игра заполнила кэш из
/// DirectInput), ДО перезаписи нашими RAW_KEYS. Читается записью в render:
/// меню навигируется стрелками/Enter через `isKeyDown`/`isKeyPressed`
/// (0x9D93A0/0x9D9400) из этого кэша, а не из InputUnit.
#[cfg(debug_assertions)]
static RAW_DOWN_SAMPLED: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
#[cfg(debug_assertions)]
static RAW_PRESSED_SAMPLED: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];

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
#[allow(dead_code)]
pub(crate) fn set_raw_key(code: u8, pressed: bool) {
    let index = drmod_replay_types::key_codes::index(u32::from(code));
    let bit = drmod_replay_types::key_codes::bit(u32::from(code));
    if index < 6 {
        if pressed {
            RAW_KEYS_PRESSED[index].fetch_or(bit, Ordering::Relaxed);
        } else {
            RAW_KEYS_DOWN[index].fetch_or(bit, Ordering::Relaxed);
        }
        RAW_KEYS_ACTIVE.store(1, Ordering::Relaxed);
    }
}

/// Подаёт сырые клавиши меню целиком (m_aKeysDown/m_aKeysPressed из записи
/// кадра) + заморозка кэша. Вызывается playback'ом каждый кадр; биты живут
/// до следующего кадра (`clear_raw_keys`) — как в API-скриптах.
pub(crate) fn set_raw_keys(down: [u32; 6], pressed: [u32; 6]) {
    for i in 0..6 {
        RAW_KEYS_DOWN[i].store(down[i], Ordering::Relaxed);
        RAW_KEYS_PRESSED[i].store(pressed[i], Ordering::Relaxed);
    }
    RAW_KEYS_ACTIVE.store(1, Ordering::Relaxed);
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
    unstick_raw_keys_cache();
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

/// Сбрасывает сэмплы blade/ripper/raw — вызывается в конце каждого
/// render-кадра, чтобы следующий тик накапливал сэмплы с нуля.
#[cfg(debug_assertions)]
pub(crate) fn reset_keybind_samples() {
    BLADE_DOWN_SAMPLED.store(0, Ordering::Relaxed);
    RIPPER_PRESSED_SAMPLED.store(0, Ordering::Relaxed);
    for i in 0..6 {
        RAW_DOWN_SAMPLED[i].store(0, Ordering::Relaxed);
        RAW_PRESSED_SAMPLED[i].store(0, Ordering::Relaxed);
    }
}

/// Читает сэмпл реальных сырых клавиш меню за прошедший тик:
/// (m_aKeysDown, m_aKeysPressed) из ms_KeyInput, накопленные детуром
/// `updateInputUnit` ПОСЛЕ оригинала. Сбрасывается в конце render-кадра через
/// `reset_keybind_samples`.
#[cfg(debug_assertions)]
pub(crate) fn read_raw_keys_sampled() -> ([u32; 6], [u32; 6]) {
    let mut down = [0u32; 6];
    let mut pressed = [0u32; 6];
    for i in 0..6 {
        down[i] = RAW_DOWN_SAMPLED[i].load(Ordering::Relaxed);
        pressed[i] = RAW_PRESSED_SAMPLED[i].load(Ordering::Relaxed);
    }
    (down, pressed)
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

    // Сэмпл реальных сырых клавиш меню: игра заполнила ms_KeyInput из
    // DirectInput (оригинал). Накопливаем ДО перезаписи нашими RAW_KEYS —
    // запись читает, что реально нажимал игрок (стрелки/Enter/Esc).
    #[cfg(debug_assertions)]
    {
        let addr = KEY_INPUT_ADDR.load(Ordering::Relaxed);
        if addr != 0 {
            let k = addr as *const types::KeyInput;
            unsafe {
                for i in 0..6 {
                    RAW_DOWN_SAMPLED[i].fetch_or((*k).keys_down[i], Ordering::Relaxed);
                    RAW_PRESSED_SAMPLED[i].fetch_or((*k).keys_pressed[i], Ordering::Relaxed);
                }
            }
        }
    }

    // Подача кадра воспроизведения (вариант B, debug-only): детур сам берёт
    // следующий кадр из PLAYBACK_FEED в тике симуляции — момент применения
    // ввода не зависит от фазы Present. Если буфер пуст (воспроизведение не
    // активно) — ввод API-скрипта из очереди (тоже по тикам, см.
    // `api::feed_tick`), иначе обычный override из render (debug-инжекция).
    #[cfg(debug_assertions)]
    let playback_fed = replay::feed_playback(unit);
    #[cfg(not(debug_assertions))]
    let playback_fed = false;
    if !playback_fed && !crate::api::feed_tick(unit) {
        replay::apply_override(unit);
    }

    // Подача raw-клавиш меню: кэш ms_KeyInput заморожен и перезаписан нашими
    // битмасками — меню читает стрелки/Enter через isKeyDown/isKeyPressed
    // (0x9D93A0/0x9D9400), а не через keybind'ы.
    if RAW_KEYS_ACTIVE.load(Ordering::Relaxed) != 0 {
        apply_raw_keys();
    }
}

/// Записывает эмулируемые raw-клавиши в `ms_KeyInput` и замораживает кэш
/// (`ms_bUpdateKeyboard = false`), чтобы DirectInput не перезаписал их.
/// В геймплее вызывается из детура `updateInputUnit` после оригинала — до
/// `handleActions`, который читает кэш через `isKeyDown`/`isKeyPressed`.
/// В меню детур не выполняется (игра не гоняет тик ввода), поэтому
/// `script_tick` зовёт эту функцию из render-потока: тогда кэш обновляем мы,
/// а DirectInput его не трогает (меню читает именно кэш — проверено
/// 2026-09-10: реальные нажатия меню видны как `menu: press isKeyDown(0x8C)=1`).
pub(crate) fn apply_raw_keys() {
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
    RAW_KEYS_IN_CACHE.store(1, Ordering::Relaxed);
    let ua = UPDATE_KEYBOARD_ADDR.load(Ordering::Relaxed);
    if ua != 0 {
        unsafe { *(ua as *mut u8) = 0 };
    }
}

/// Возвращает нули в `ms_KeyInput`, если мы туда писали, — иначе в меню
/// останется «залипшая» клавиша (игра кэш в паузе сама не обновляет).
fn unstick_raw_keys_cache() {
    if RAW_KEYS_IN_CACHE.swap(0, Ordering::Relaxed) == 0 {
        return;
    }
    let addr = KEY_INPUT_ADDR.load(Ordering::Relaxed);
    if addr == 0 {
        return;
    }
    let k = addr as *mut types::KeyInput;
    unsafe {
        for i in 0..6 {
            (*k).keys_down[i] = 0;
            (*k).keys_pressed[i] = 0;
        }
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

/// Диагностика меню: логирует первое нажатие (результат != 0), которое игра
/// увидела в меню — так видно, какие коды реально приходят с устройства
/// (`menu: press isKeyDown(0x8C)=1` — так была найдена кодировка бит).
fn log_menu_press(kind: &str, vkey: i32, result: i32) {
    if result == 0 {
        return;
    }
    if let Some((index, bit, status)) = menu_key_slot(vkey)
        && MENU_PRESS_SEEN[index].fetch_or(bit, Ordering::Relaxed) & bit == 0
    {
        logger::log_line(&format!(
            "menu: press {}(0x{:X})=1 при статусе {}",
            kind, vkey, status
        ));
    }
}

/// (индекс битмапа, бит, статус меню) — только если игра не в геймплее.
fn menu_key_slot(vkey: i32) -> Option<(usize, u32, i32)> {
    let base = BASE_ADDR.load(Ordering::Relaxed);
    if base == 0 {
        return None;
    }
    let status = unsafe { *((base + addresses::GAME_MENU_STATUS) as *const i32) };
    if status == 1 {
        return None; // геймплей — это не меню
    }
    let index = ((vkey as u32 & 0xFF) >> 5) as usize;
    if index >= 8 {
        return None;
    }
    Some((index, drmod_replay_types::key_codes::bit(vkey as u32), status))
}

/// Снимки сырого состояния клавиатуры для логирования изменений (диагностика:
/// по ним видно, какое состояние меняет реальное нажатие в меню).
static PREV_KEYS_DOWN: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
static PREV_KEYS_PRESSED: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
static PREV_INPUT_KEYS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];

/// Диагностика: логирует изменения `ms_KeyInput` (игровые коды) и
/// `ms_InputKeys` (DirectInput, DIK-коды). Вызывается из render-цикла: по этим
/// строкам видно, какое состояние меняет реальное нажатие клавиши — и, значит,
/// что нужно подавать, чтобы меню увидело ввод.
pub(crate) fn log_key_state_changes() {
    let ki = KEY_INPUT_ADDR.load(Ordering::Relaxed);
    if ki != 0 {
        let k = ki as *const types::KeyInput;
        let (down, pressed) = unsafe { ((*k).keys_down, (*k).keys_pressed) };
        for i in 0..6 {
            let prev = PREV_KEYS_DOWN[i].swap(down[i], Ordering::Relaxed);
            if prev != down[i] {
                logger::log_line(&format!(
                    "keys: ms_KeyInput.down[{i}] {prev:08X} -> {:08X}",
                    down[i]
                ));
            }
            let prev = PREV_KEYS_PRESSED[i].swap(pressed[i], Ordering::Relaxed);
            if prev != pressed[i] {
                logger::log_line(&format!(
                    "keys: ms_KeyInput.pressed[{i}] {prev:08X} -> {:08X}",
                    pressed[i]
                ));
            }
        }
    }
    let base = BASE_ADDR.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }
    let addr = base + addresses::INPUT_KEYS;
    for byte in 0..256usize {
        let value = unsafe { *((addr + byte) as *const u8) };
        let word = byte >> 5;
        let bit = 1u32 << (byte & 31);
        let had = PREV_INPUT_KEYS[word].load(Ordering::Relaxed) & bit != 0;
        let has = value != 0;
        if had != has {
            if has {
                PREV_INPUT_KEYS[word].fetch_or(bit, Ordering::Relaxed);
            } else {
                PREV_INPUT_KEYS[word].fetch_and(!bit, Ordering::Relaxed);
            }
            logger::log_line(&format!(
                "keys: ms_InputKeys[0x{byte:02X}] {} (DIK)",
                if has { "down" } else { "up" }
            ));
        }
    }
}

/// DIK-биты, которые мы подмешиваем в `ms_InputKeys` после опроса DirectInput
/// (битмап по DIK-кодам: 8 слов = 256 кодов). Подаются из `script_tick`.
static EMULATED_DIK: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static ORIG_KEYBOARD_POLL: OnceLock<unsafe extern "thiscall" fn(*const u8)> = OnceLock::new();

/// Подаёт DIK-клавиши (DirectInput, 8 слов = 256 кодов) — их подмешает детур
/// `KEYBOARD_POLL` после опроса устройства, и игра увидит клавишу как реальную
/// (меню читает именно этот путь; запись в кэши не работает — игра их
/// перезаписывает каждый кадр, проверено 2026-09-10).
pub(crate) fn set_dik_mask(mask: [u32; 8]) {
    for (slot, value) in EMULATED_DIK.iter().zip(mask) {
        slot.store(value, Ordering::Relaxed);
    }
}

/// Снимает поданные DIK-клавиши.
pub(crate) fn clear_dik_mask() {
    set_dik_mask([0; 8]);
}

/// Детур опроса клавиатуры (`base+0x9D9670`): вызывает оригинал (DirectInput
/// `GetDeviceState` заполняет `ms_InputKeys`), затем подмешивает наши DIK-биты.
unsafe extern "thiscall" fn keyboard_poll_detour(this: *const u8) {
    if let Some(&orig) = ORIG_KEYBOARD_POLL.get() {
        unsafe { orig(this) };
    }
    let base = BASE_ADDR.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }
    let keys = (base + addresses::INPUT_KEYS) as *mut u8;
    for (word_index, word) in EMULATED_DIK.iter().enumerate() {
        let word = word.load(Ordering::Relaxed);
        if word == 0 {
            continue;
        }
        for bit in 0..32 {
            if word & (1 << bit) == 0 {
                continue;
            }
            let dik = (word_index * 32 + bit) as usize;
            unsafe { *keys.add(dik) |= 0x80 };
        }
    }
}

/// Сохраняет trampoline оригинала опроса клавиатуры.
fn set_original_keyboard_poll(
    orig: unsafe extern "thiscall" fn(*const u8),
) -> Result<(), ()> {
    ORIG_KEYBOARD_POLL.set(orig).map_err(|_| ())
}

/// Детур `KeyInput::isKeyDown` (thiscall, 0x9D93A0). Для эмулируемых клавиш
/// (`RAW_KEYS_DOWN[index] & bit != 0`) возвращает 1 — меню-клавиши
/// (weapon_select/pause/confirm/codec/menu_*) работают через функцию 0x8AC570,
/// которая вызывает `isKeyDown` с игровыми кодами клавиш (0x8D/0x8E/0x8C/0x8F/
/// 0x90..0x93). Остальные клавиши идут в оригинал.
unsafe extern "thiscall" fn is_key_down_detour(this: *const u8, vkey: i32) -> i32 {
    let index = drmod_replay_types::key_codes::index(vkey as u32);
    let bit = drmod_replay_types::key_codes::bit(vkey as u32);
    if index < 6 && RAW_KEYS_DOWN[index].load(Ordering::Relaxed) & bit != 0 {
        crate::logger::log_line(&format!("isKeyDown(0x{:X}) -> 1 (emulated)", vkey));
        return 1;
    }
    let result = if let Some(&orig) = ORIG_IS_KEY_DOWN.get() {
        unsafe { orig(this, vkey) }
    } else {
        0
    };
    log_menu_press("isKeyDown", vkey, result);
    result
}

/// Детур `KeyInput::isKeyPressed` (thiscall, 0x9D9400). Аналогично
/// `is_key_down_detour`, но для фронта нажатия (`RAW_KEYS_PRESSED`).
unsafe extern "thiscall" fn is_key_pressed_detour(this: *const u8, vkey: i32) -> i32 {
    let index = drmod_replay_types::key_codes::index(vkey as u32);
    let bit = drmod_replay_types::key_codes::bit(vkey as u32);
    if index < 6 && RAW_KEYS_PRESSED[index].load(Ordering::Relaxed) & bit != 0 {
        return 1;
    }
    let result = if let Some(&orig) = ORIG_IS_KEY_PRESSED.get() {
        unsafe { orig(this, vkey) }
    } else {
        0
    };
    log_menu_press("isKeyPressed", vkey, result);
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

/// Сохраняет trampoline оригинальной `isKeyDown` после создания хука.
fn set_original_is_key_down(
    orig: unsafe extern "thiscall" fn(*const u8, i32) -> i32,
) -> Result<(), ()> {
    ORIG_IS_KEY_DOWN.set(orig).map_err(|_| ())
}

/// Сохраняет trampoline оригинальной `isKeyPressed` после создания хука.
fn set_original_is_key_pressed(
    orig: unsafe extern "thiscall" fn(*const u8, i32) -> i32,
) -> Result<(), ()> {
    ORIG_IS_KEY_PRESSED.set(orig).map_err(|_| ())
}

/// MinHook-хуки ввода. Поля приватные: хуки живут, пока живёт структура
/// (деструктор `MhHook` снимает хук), наружу выставляются только функции
/// чтения сырого ввода.
#[allow(dead_code)] // keep-alive: поля не читаются, но Drop снимает хуки
pub struct InputHooks {
    input: Option<MhHook>,
    keybind: Option<MhHook>,
    keybind_down: Option<MhHook>,
    key_down: Option<MhHook>,
    key_pressed: Option<MhHook>,
    keyboard_poll: Option<MhHook>,
    /// Диагностический хук геттера времени (только debug): собирает вызывающих,
    /// по ним ищется планировщик шагов симуляции.
    time_ticks: Option<MhHook>,
    /// Хук `Sleep`: ровная сетка кадров (пацеры получают задержку до дедлайна).
    sleep: Option<MhHook>,
}

impl InputHooks {
    /// Устанавливает все хуки ввода и запоминает адреса сырого ввода.
    /// Каждый хук логирует свой результат в debug.log.
    pub fn new(base_addr: usize) -> Self {
        BASE_ADDR.store(base_addr, Ordering::Relaxed);
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
        let key_down = Self::create_key_down_hook(base_addr);
        let key_pressed = Self::create_key_pressed_hook(base_addr);
        let keyboard_poll = Self::create_keyboard_poll_hook(base_addr);
        // Хук времени — диагностика (сбор вызывающих геттера), в release не нужен.
        #[cfg(debug_assertions)]
        let time_ticks = Self::create_time_hook(base_addr);
        #[cfg(not(debug_assertions))]
        let time_ticks = None;
        // Хук Sleep ОТКЛЮЧЁН (2026-09-11): хук на системную функцию, которую
        // дёргают все потоки процесса, крашил игру (патч кода из-под других
        // потоков + риск в ABI naked-детура). Правильная точка — не `Sleep`, а
        // функции самих пацеров игры (0xB98070 и 0x9F2A40): их зовёт только
        // игровой поток, поэтому патч безопасен. См. tools/disasm/README.md.
        let sleep: Option<MhHook> = None;

        logger::log_line(&format!(
            "=== drmod init === base=0x{:08X} input_hook={} keybind_hook={} keybind_down_hook={} key_down_hook={} key_pressed_hook={} keyboard_poll_hook={}",
            base_addr,
            if input.is_some() { "OK" } else { "FAIL" },
            if keybind.is_some() { "OK" } else { "FAIL" },
            if keybind_down.is_some() { "OK" } else { "FAIL" },
            if key_down.is_some() { "OK" } else { "FAIL" },
            if key_pressed.is_some() { "OK" } else { "FAIL" },
            if keyboard_poll.is_some() { "OK" } else { "FAIL" }
        ));

        Self {
            input,
            keybind,
            keybind_down,
            key_down,
            key_pressed,
            keyboard_poll,
            time_ticks,
            sleep,
        }
    }

    /// Ставит MinHook на `Sleep` (адрес из слота IAT игры): пацерам кадров
    /// выдаём задержку до дедлайна ровной сетки (см. `set_frame_limit`).
    ///
    /// ⚠️ НЕ ИСПОЛЬЗУЕТСЯ (2026-09-11): хук на системный `Sleep` крашил игру.
    /// Патчить функцию, которую дёргают все потоки процесса, небезопасно.
    /// Ровную сетку надо ставить хуком на сами пацеры игры (`0xB98070`,
    /// `0x9F2A40`) — их вызывает только игровой поток.
    #[allow(dead_code)]
    fn create_sleep_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_sleep_hook: base_addr=0");
            return None;
        }
        let slot = (base_addr + SLEEP_IAT_RVA) as *const usize;
        let target = unsafe { *slot } as *mut c_void;
        if (target as usize) < SYSTEM_ADDR_MIN {
            logger::log_line(&format!(
                "create_sleep_hook: по слоту 0x{:08X} лежит 0x{:08X} — это не системная \
                 функция, хук не ставим",
                base_addr + SLEEP_IAT_RVA, target as usize
            ));
            return None;
        }
        let detour = sleep_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_sleep_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        ORIG_SLEEP.store(hook.trampoline() as usize, Ordering::Relaxed);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!("create_sleep_hook: queue_enable FAIL {e:?}"));
            return None;
        }
        logger::log_line(&format!(
            "sleep_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
    }

    /// Ставит MinHook на геттер времени `cTime::getTicks` (0x9F8230): детур
    /// запоминает адреса возврата вызывающих, по ним ищется планировщик шагов
    /// симуляции (см. `tools/disasm/README.md`, «Модуль времени движка»).
    fn create_time_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_time_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + addresses::TIME_GET_TICKS) as *mut c_void;
        let detour = time_ticks_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_time_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        ORIG_TIME_TICKS.store(hook.trampoline() as usize, Ordering::Relaxed);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!("create_time_hook: queue_enable FAIL {e:?}"));
            return None;
        }
        logger::log_line(&format!(
            "time_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
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

    /// Устанавливает MinHook на `KeyInput::isKeyDown` (thiscall, 0x9D93A0):
    /// для эмулируемых клавиш (weapon_select/pause/confirm/menu_*) возвращает 1.
    fn create_key_down_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_key_down_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + addresses::IS_KEY_DOWN) as *mut c_void;
        let detour = is_key_down_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_key_down_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "thiscall" fn(*const u8, i32) -> i32 =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = set_original_is_key_down(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!("create_key_down_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        logger::log_line(&format!(
            "create_key_down_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
    }

    /// Устанавливает MinHook на опрос клавиатуры (`base+0x9D9670`, DirectInput
    /// `GetDeviceState`): после оригинала подмешивает наши DIK-биты
    /// (`set_dik_mask`) — так игра видит клавиши меню как реальные.
    fn create_keyboard_poll_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_keyboard_poll_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + addresses::KEYBOARD_POLL) as *mut c_void;
        let hook = match unsafe { MhHook::new(target, keyboard_poll_detour as *mut c_void) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_keyboard_poll_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "thiscall" fn(*const u8) =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = set_original_keyboard_poll(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!(
                "create_keyboard_poll_hook: queue_enable FAIL err={:?}",
                e
            ));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        logger::log_line(&format!(
            "create_keyboard_poll_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
    }

    /// Устанавливает MinHook на `KeyInput::isKeyPressed` (thiscall, 0x9D9400):
    /// для эмулируемых клавиш (фронт нажатия) возвращает 1.
    fn create_key_pressed_hook(base_addr: usize) -> Option<MhHook> {
        use core::ffi::c_void;

        if base_addr == 0 {
            logger::log_line("create_key_pressed_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + addresses::IS_KEY_PRESSED) as *mut c_void;
        let detour = is_key_pressed_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                logger::log_line(&format!(
                    "create_key_pressed_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "thiscall" fn(*const u8, i32) -> i32 =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = set_original_is_key_pressed(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            logger::log_line(&format!(
                "create_key_pressed_hook: queue_enable FAIL err={:?}",
                e
            ));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        logger::log_line(&format!(
            "create_key_pressed_hook: OK target=0x{:08X} trampoline=0x{:08X}",
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