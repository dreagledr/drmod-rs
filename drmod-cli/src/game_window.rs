//! Игровое окно и единственное, что CLI нужно от него — передний план.
//!
//! Игра читает клавиатуру через `DirectInput::GetDeviceState` и рано выходит, когда
//! её окно не foreground. Значит, для скрипта с `restart` мало подать биты пада:
//! шаги меню (DIK-стрелки) дойдут, только пока игра владеет фокусом. Редактор
//! делает это же (`game_window::focus_and_settle`) перед прогоном.
//!
//! `SetForegroundWindow` отказывает, если вызывающий процесс сам не владеет
//! фокусом, поэтому окно сначала поднимается в верх z-порядка, вызов повторяется,
//! а окно предварительно разворачивается (минимизированное фокус не возьмёт).

use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::SetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, FindWindowW, GetForegroundWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
};
use windows::core::w;

/// Выводит окно игры на передний план, сколько попыток потребуется.
///
/// `false`, если игры нет вовсе — вызывающий скажет об этом, а не сделает вид,
/// что ввод дойдёт.
pub fn activate(tries: usize) -> bool {
    let Ok(window) = find() else {
        return false;
    };

    for attempt in 0..tries {
        unsafe {
            let _ = ShowWindow(window, SW_RESTORE);
        }

        if foreground() == window {
            return true;
        }

        unsafe {
            let _ = BringWindowToTop(window);
            let _ = SetForegroundWindow(window);
            let _ = SetActiveWindow(window);
        }

        if foreground() == window {
            return true;
        }

        std::thread::sleep(Duration::from_millis(300 * (attempt as u64 + 1)));
    }

    foreground() == window
}

/// Активирует окно и даёт игре пару кадров, чтобы подхватить активацию.
pub fn focus_and_settle(settle_ms: u64) -> bool {
    let focused = activate(3);
    std::thread::sleep(Duration::from_millis(settle_ms));
    focused
}

fn find() -> windows::core::Result<HWND> {
    unsafe { FindWindowW(None, w!("METAL GEAR RISING: REVENGEANCE")) }
}

fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}
