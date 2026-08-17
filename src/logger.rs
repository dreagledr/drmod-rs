//! Логгер в `%LOCALAPPDATA%\drmod\`: `debug.log` (построчно, с таймстампом)
//! и буферизованный `state.log` (пачками, для per-frame состояния).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

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
pub(crate) fn init_state_log() {
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
pub(crate) fn log_state_line(line: &str) {
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