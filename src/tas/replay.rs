//! Record/Replay — типы и адреса системы ввода (этапы 0–1: чтение и подача).
//!
//! Подача ввода работает через override глобального `InputUnit[0]`
//! (`base + 0x177B850`) в хуке `cInput::updateInputUnit` (см. `docs/REPLAY.md`).
//! Прямая запись в сырые кэши и поля `Pl0000` не работает — игрок читает
//! ввод из `g_InputUnit0`, а не из этих мест.

use super::addresses::{
    DISABLE_RIPPER_MODE, ENABLE_RIPPER_MODE, KEYBIND_BLADEMODE, KEYBIND_RIPPERMODE,
};
use super::types::{InputOverride, InputUnit};

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

const INPUT_OVERRIDE_INIT: InputOverride = InputOverride {
    active: false,
    input: InputUnit {
        buttons_down: 0,
        buttons_pressed: 0,
        buttons_released: 0,
        buttons_alternated: 0,
        left_stick: [0.0; 2],
        right_stick: [0.0; 2],
        left_trigger: 0.0,
        right_trigger: 0.0,
        valid_input: 0,
        repeat_count: 0,
    },
};

static INPUT_OVERRIDE: Mutex<InputOverride> = Mutex::new(INPUT_OVERRIDE_INIT);
static ORIG_UPDATE_INPUT_UNIT: OnceLock<unsafe extern "C" fn(*mut InputUnit, i32)> =
    OnceLock::new();
static ORIG_IS_KEYBIND_PRESSED: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
static ORIG_IS_KEYBIND_DOWN: OnceLock<unsafe extern "C" fn(i32) -> i32> = OnceLock::new();
/// Базовый адрес модуля игры (устанавливается в `HelloHud::new`) — нужен
/// в детуре для прямой записи в сырые структуры ввода.
static BASE_ADDR: OnceLock<usize> = OnceLock::new();
/// Остаток кадров эмуляции клавиши R (ripper) в детуре.
static RIPPER_FRAMES: AtomicU32 = AtomicU32::new(0);
/// Флаг удержания blade mode (isKeybindDown, hold) в детуре.
static BLADE_HOLD: AtomicU32 = AtomicU32::new(0);

/// Запоминает базовый адрес модуля для использования в детуре.
pub fn set_base_addr(addr: usize) -> Result<(), ()> {
    BASE_ADDR.set(addr).map_err(|_| ())
}

/// Взводит эмуляцию клавиши R (ripper) на `n` кадров — сырой ввод, который
/// читается `isKeybindDown(KEYBIND_RIPPERMODE)`, а не `InputUnit`.
pub fn set_ripper_frames(n: u32) {
    RIPPER_FRAMES.store(n, Ordering::Relaxed);
}

/// Сколько кадров эмуляции R осталось (для debug-панели).
pub fn ripper_frames() -> u32 {
    RIPPER_FRAMES.load(Ordering::Relaxed)
}

/// Взводит/снимает удержание blade mode (isKeybindDown, hold-действие).
pub fn set_blade_hold(on: bool) {
    BLADE_HOLD.store(if on { 1 } else { 0 }, Ordering::Relaxed);
}

/// Удерживается ли blade mode сейчас (для debug-панели).
pub fn blade_hold() -> bool {
    BLADE_HOLD.load(Ordering::Relaxed) != 0
}

/// Сбрасывает keybind-эмуляцию (ripper/blade) — вызывается при остановке
/// воспроизведения, старте записи и входе в loading, чтобы hold-действие
/// (blade) и однокадровый фронт (ripper) не «зависали» и не подмешивались
/// в реальный ввод.
#[cfg(debug_assertions)]
pub fn clear_keybind_emulation() {
    RIPPER_FRAMES.store(0, Ordering::Relaxed);
    BLADE_HOLD.store(0, Ordering::Relaxed);
}

/// Включает Ripper Mode напрямую (обход ввода) — `Pl0000::enableRipperMode`
/// (`__thiscall`, `this` = указатель на объект игрока).
pub fn enable_ripper(player: *mut u8) {
    let Some(&base) = BASE_ADDR.get() else {
        return;
    };
    type Fn = unsafe extern "thiscall" fn(*mut u8);
    let f: Fn = unsafe { std::mem::transmute((base + ENABLE_RIPPER_MODE) as *const ()) };
    unsafe { f(player) };
}

/// Выключает Ripper Mode (`Pl0000::disableRipperMode(bool)`, `__thiscall`).
pub fn disable_ripper(player: *mut u8) {
    let Some(&base) = BASE_ADDR.get() else {
        return;
    };
    type Fn = unsafe extern "thiscall" fn(*mut u8, bool);
    let f: Fn = unsafe { std::mem::transmute((base + DISABLE_RIPPER_MODE) as *const ()) };
    unsafe { f(player, false) };
}
/// Последнее значение m_CurrentInput.buttons_down<<32 | buttons_pressed —
/// для ловли фронтов (pressed/down) при реальном вводе.
static LAST_CUR_IN: AtomicU64 = AtomicU64::new(0);

/// Возвращает true, если (down, pressed) изменились с прошлого вызова,
/// и запоминает новые значения. Используется в render для логирования
/// однократных нажатий (прыжок/атаки), которые иначе проскакивают
/// между периодическими frame-логами.
pub fn cur_in_changed(down: u32, pressed: u32) -> bool {
    let key = ((down as u64) << 32) | pressed as u64;
    LAST_CUR_IN.swap(key, Ordering::Relaxed) != key
}

/// Устанавливает override для хука ввода (вызывается из render).
/// Логирует изменения состояния в debug.log.
pub fn set_input_override(ov: InputOverride) {
    if let Ok(mut guard) = INPUT_OVERRIDE.lock() {
        let changed = guard.active != ov.active
            || guard.input.left_stick != ov.input.left_stick
            || guard.input.right_stick != ov.input.right_stick
            || guard.input.buttons_down != ov.input.buttons_down
            || guard.input.buttons_pressed != ov.input.buttons_pressed;
        if changed {
            log_line(&format!(
                "set_override: active={} down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2})",
                ov.active,
                ov.input.buttons_down,
                ov.input.buttons_pressed,
                ov.input.left_stick[0],
                ov.input.left_stick[1],
                ov.input.right_stick[0],
                ov.input.right_stick[1]
            ));
        }
        *guard = ov;
    }
}

/// Доступ к текущему override (используется в debug-панели).
pub fn input_override() -> MutexGuard<'static, InputOverride> {
    INPUT_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Сохраняет trampoline (адрес оригинальной функции) после создания хука.
pub(super) fn set_original_update_input_unit(
    orig: unsafe extern "C" fn(*mut InputUnit, i32),
) -> Result<(), ()> {
    ORIG_UPDATE_INPUT_UNIT.set(orig).map_err(|_| ())
}

/// Сохраняет trampoline оригинальной `isKeybindPressed` после создания хука.
pub(super) fn set_original_is_keybind_pressed(orig: unsafe extern "C" fn(i32) -> i32) -> Result<(), ()> {
    ORIG_IS_KEYBIND_PRESSED.set(orig).map_err(|_| ())
}

/// Сохраняет trampoline оригинальной `isKeybindDown` после создания хука.
pub(super) fn set_original_is_keybind_down(orig: unsafe extern "C" fn(i32) -> i32) -> Result<(), ()> {
    ORIG_IS_KEYBIND_DOWN.set(orig).map_err(|_| ())
}

static LOG_MUTEX: Mutex<()> = Mutex::new(());

/// Re-entrancy guard для `log_line`. Если сам `log_line` падает (chrono или
/// файловый I/O при рестарте игры), VEH-обработчик вызовет `log_line` повторно
/// и получится бесконечная рекурсия исключений (stack overflow). Флаг запрещает
/// повторный вход: после первого фолта он остаётся взведённым, и `log_line`
/// деградирует в no-op.
static LOG_REENTRY: AtomicBool = AtomicBool::new(false);

/// Дописывает строку в `%LOCALAPPDATA%\drmod\debug.log` с таймстампом.
/// Используется для отладки хука ввода (детур/override).
pub(crate) fn log_line(line: &str) {
    if LOG_REENTRY.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(_guard) = LOG_MUTEX.lock() else {
        LOG_REENTRY.store(false, Ordering::SeqCst);
        return;
    };
    let Ok(localappdata) = std::env::var("LOCALAPPDATA") else {
        LOG_REENTRY.store(false, Ordering::SeqCst);
        return;
    };
    let path = format!("{}\\drmod\\debug.log", localappdata);
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        LOG_REENTRY.store(false, Ordering::SeqCst);
        return;
    };
    let _ = std::io::Write::write_fmt(
        &mut f,
        format_args!(
            "[{}] {}\r\n",
            chrono::Local::now().format("%H:%M:%S%.3f"),
            line
        ),
    );
    LOG_REENTRY.store(false, Ordering::SeqCst);
}

/// Буфер отдельного лога состояния (velocity/rotation/heading/ripper/blade/...).
/// Пишется пачками, чтобы не открывать файл 60 раз в секунду.
static STATE_LOG_BUF: Mutex<Vec<String>> = Mutex::new(Vec::new());
static STATE_LOG_COUNT: AtomicU32 = AtomicU32::new(0);
const STATE_LOG_FLUSH_EVERY: u32 = 60;

/// Перезатирает `%LOCALAPPDATA%\drmod\state.log` при старте мода и очищает
/// буфер — чтобы лог не рос между сессиями.
pub fn init_state_log() {
    let Ok(localappdata) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let dir = format!("{}\\drmod", localappdata);
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{}\\state.log", dir);
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = std::io::Write::write_fmt(
            &mut f,
            format_args!(
                "[{}] state log started\r\nf=frame pos=(x,y,z) vel=(x,y,z, vertical-only) prev=(0x900, last pos) rot=(x,y,z) heading dir ripper blade ninja jump cam=(x,y,z)\r\n",
                chrono::Local::now().format("%H:%M:%S%.3f")
            ),
        );
    }
    if let Ok(mut buf) = STATE_LOG_BUF.lock() {
        buf.clear();
    }
    STATE_LOG_COUNT.store(0, Ordering::Relaxed);
}

/// Дописывает строку состояния в буфер; флашится в `state.log` каждые
/// `STATE_LOG_FLUSH_EVERY` строк.
pub fn log_state_line(line: &str) {
    if let Ok(mut buf) = STATE_LOG_BUF.lock() {
        buf.push(line.to_string());
    }
    if STATE_LOG_COUNT.fetch_add(1, Ordering::Relaxed) % STATE_LOG_FLUSH_EVERY
        == STATE_LOG_FLUSH_EVERY - 1
    {
        flush_state_log();
    }
}

/// Сбрасывает буфер состояния в `state.log` (append).
fn flush_state_log() {
    let Ok(localappdata) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let path = format!("{}\\drmod\\state.log", localappdata);
    let Ok(mut buf) = STATE_LOG_BUF.lock() else {
        return;
    };
    if buf.is_empty() {
        return;
    }
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    for line in buf.drain(..) {
        let _ = std::io::Write::write_fmt(&mut f, format_args!("{}\r\n", line));
    }
}

/// Детур `cInput::updateInputUnit` (__cdecl). Вызывает оригинал, затем для
/// `user_index == 0` перезаписывает unit нашим override (подача ввода).
///
/// Детур вызывается игрой несколько раз за кадр и должен быть ЛЁГКИМ: только
/// чтение/запись атомиков. Никакого `log_line` (chrono + файловый I/O) — при
/// рестарте это даёт рекурсию access violation (см. docs/REPLAY_FINDINGS.md).
pub(super) unsafe extern "C" fn update_input_unit_detour(unit: *mut InputUnit, user_index: i32) {
    if let Some(&orig) = ORIG_UPDATE_INPUT_UNIT.get() {
        unsafe { orig(unit, user_index) };
    }

    if user_index != 0 {
        return;
    }

    if let Ok(guard) = INPUT_OVERRIDE.lock()
        && guard.active
    {
        unsafe { *unit = guard.input };
    }
}

/// Детур `cInput::isKeybindPressed` (__cdecl, 0x61D2D0). Для `KEYBIND_RIPPERMODE`
/// возвращает 1 (нажат фронт), пока эмуляция R активна (`RIPPER_FRAMES > 0`) —
/// тогда `handleActions` запускает штатную активацию/деактивацию ripper
/// с проверками условий и анимациями. Остальные keybind'ы идут в оригинал.
pub(super) unsafe extern "C" fn is_keybind_pressed_detour(keybind: i32) -> i32 {
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
pub(super) unsafe extern "C" fn is_keybind_down_detour(keybind: i32) -> i32 {
    if keybind == KEYBIND_BLADEMODE && BLADE_HOLD.load(Ordering::Relaxed) != 0 {
        return 1;
    }
    if let Some(&orig) = ORIG_IS_KEYBIND_DOWN.get() {
        return unsafe { orig(keybind) };
    }
    0
}
