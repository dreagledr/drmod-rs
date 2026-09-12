//! Лог мода в `%LOCALAPPDATA%\cutscene_skip\cutscene_skip.log` (его же читает
//! лаунчер с `--follow`).

use std::io::Write;
use std::sync::Mutex;

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

static LOG_MUTEX: Mutex<()> = Mutex::new(());

fn timestamp() -> String {
    let SYSTEMTIME {
        wHour,
        wMinute,
        wSecond,
        wMilliseconds,
        ..
    } = unsafe { GetLocalTime() };
    format!("{wHour:02}:{wMinute:02}:{wSecond:02}.{wMilliseconds:03}")
}

/// Дописывает строку с таймстампом. Ошибки I/O молча игнорируем: лог — не
/// критичный путь, а паника внутри детура хука недопустима.
pub(crate) fn log_line(line: &str) {
    let Ok(_guard) = LOG_MUTEX.lock() else {
        return;
    };
    let Some(dir) = crate::data_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("cutscene_skip.log"))
    else {
        return;
    };
    let _ = writeln!(f, "[{}] {}", timestamp(), line);
}
