//! The game's window, and the one thing the editor needs from it: the foreground.
//!
//! The game reads its keyboard through `DirectInput::GetDeviceState` and gives up early when its
//! window is not foreground — so a script that presses pad buttons is not enough: the menu steps a
//! `restart` plays arrive only while the game owns the input focus. That is a measured trap in the
//! python tools, and the editor is the same client.
//!
//! `SetForegroundWindow` alone is refused unless the calling process already owns the foreground,
//! so the call is retried and the window pushed to the top of the z-order first — the same dance
//! `drmod_api.focus_and_settle` does before it feeds menu input. The window is restored first (a
//! minimised game cannot take focus), the whole thing is tried a few times (something else can be
//! stealing focus back — a person at the machine), and a short settle follows: the game has to
//! process the activation before it starts reading keys.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, SetActiveWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, FindWindowW, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};
use windows::core::w;

/// The window title the game runs under.
pub const TITLE: &str = "METAL GEAR RISING: REVENGEANCE";

/// Brings the game's window to the foreground, through as many attempts as it takes.
///
/// Returns whether it worked: a game that is not running at all answers `false`, and the caller
/// says so instead of pretending the input will land.
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
            // Pushing the window to the top of the z-order first is what makes the following
            // foreground request likely to be granted: a window that is already on top is one the
            // user is plausibly looking at.
            let _ = BringWindowToTop(window);
            let _ = SetForegroundWindow(window);
            let _ = SetActiveWindow(window);

            // The game itself has to own the focus, not merely be on top: `GetForegroundWindow` is
            // the question the DirectInput poll asks, and the comparison below is what reads it.
            let _ = GetForegroundWindow();
        }

        if foreground() == window {
            return true;
        }

        std::thread::sleep(std::time::Duration::from_millis(300 * (attempt as u64 + 1)));
    }

    foreground() == window
}

/// Activates the window and gives the game a few frames to pick the activation up.
pub fn focus_and_settle(settle_ms: u64) -> bool {
    let focused = activate(3);
    std::thread::sleep(std::time::Duration::from_millis(settle_ms));
    focused
}

/// Whether the game's window exists at all — the cheap check a pane can make without touching
/// focus.
pub fn running() -> bool {
    find().is_ok()
}

fn find() -> windows::core::Result<HWND> {
    unsafe { FindWindowW(None, w!("METAL GEAR RISING: REVENGEANCE")) }
}

fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// Whether a window is minimised — a game that is cannot take focus until it is restored, which
/// [`activate`] does.
pub fn minimised(window: HWND) -> bool {
    unsafe { IsIconic(window).as_bool() }
}

/// The window handle as a number, for callers that only need to compare two of them.
pub fn handle_value() -> Option<isize> {
    find().ok().map(|window| window.0 as isize)
}

/// The thread that owns a window — the pair `AttachThreadInput` needs, and the value that says
/// whether the game's input queue and this process's are already joined.
pub fn owning_thread(window: HWND) -> u32 {
    unsafe { GetWindowThreadProcessId(window, None) }
}

/// Whatever window currently owns the keyboard focus.
pub fn active_window() -> HWND {
    unsafe { GetActiveWindow() }
}
