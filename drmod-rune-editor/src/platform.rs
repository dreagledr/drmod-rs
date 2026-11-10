//! The game window, as far as the editor needs it.
//!
//! A script's own `restart` plays the pause menu, and the menu reads the keyboard through the
//! game's DirectInput poll — which gives up while the game's window is not foreground. Bringing
//! the window forward is therefore part of a run on Windows. On Linux there is nothing to focus:
//! the editor still edits and still talks HTTP, but a real run needs the game on Windows.

#[cfg(windows)]
mod game_window;

/// Brings the game's window forward and lets it process the activation.
///
/// `None` where there is no window to focus (this build runs elsewhere), so a caller can tell
/// "not applicable" from "tried and failed".
pub fn focus_and_settle(settle_ms: u64) -> Option<bool> {
    #[cfg(windows)]
    {
        Some(game_window::focus_and_settle(settle_ms))
    }

    #[cfg(not(windows))]
    {
        let _ = settle_ms;
        None
    }
}
