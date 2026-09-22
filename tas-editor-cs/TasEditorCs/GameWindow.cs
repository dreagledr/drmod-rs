using System;
using System.Runtime.InteropServices;
using System.Threading;

/// The game's window, and the one thing the editor needs from it: the foreground.
///
/// The game reads its keyboard through `DirectInput::GetDeviceState` and gives up early when its
/// window is not foreground — so a script that presses pad buttons is not enough: the menu steps a
/// `restart` plays arrive only while the game owns the input focus. That is a measured trap in the
/// python tools (`drmod_api.py`, the module docstring), and the editor is the same client.
///
/// `SetForegroundWindow` alone is refused unless the calling process already owns the foreground, so
/// the call is made through `AttachThreadInput`: attaching this thread's input queue to the window's
/// own thread lifts the restriction. The window is restored first (a minimised game cannot take
/// focus), the whole thing is tried a few times (something else can be stealing focus back — a person
/// at the machine), and a short settle follows: the game has to process the activation before it
/// starts reading keys.
///
/// Everything here runs on the caller's thread — the UI thread, in practice. `AttachThreadInput`
/// needs a thread with an input queue, which a thread-pool thread may not have.
internal static partial class GameWindow
{
    /// The window title the game runs under (`drmod_api.GAME_TITLE`).
    internal const string Title = "METAL GEAR RISING: REVENGEANCE";

    /// `SW_RESTORE` — un-minimise, without maximising.
    const int Restore = 9;

    /// Brings the game's window to the foreground, through as many attempts as it takes.
    ///
    /// Returns whether it worked: a game that is not running at all answers `false`, and the caller
    /// says so instead of pretending the input will land.
    internal static bool Activate(string title = Title, int tries = 3)
    {
        var window = FindWindow(null, title);
        if (window == 0)
        {
            return false;
        }

        for (var attempt = 0; attempt < tries; attempt++)
        {
            ShowWindow(window, Restore);
            if (GetForegroundWindow() == window)
            {
                return true;
            }

            var target = GetWindowThreadProcessId(window, 0);
            var current = GetCurrentThreadId();
            AttachThreadInput(current, target, true);
            try
            {
                SetForegroundWindow(window);
                BringWindowToTop(window);
            }
            finally
            {
                AttachThreadInput(current, target, false);
            }

            if (GetForegroundWindow() == window)
            {
                return true;
            }

            Thread.Sleep(300 * (attempt + 1));
        }

        return GetForegroundWindow() == window;
    }

    /// Activates the window and gives the game a few frames to pick the activation up — the exact
    /// dance `drmod_api.focus_and_settle` does before it feeds menu input.
    internal static bool FocusAndSettle(string title = Title, int settleMs = 350)
    {
        var focused = Activate(title);
        Thread.Sleep(settleMs);
        return focused;
    }

    [LibraryImport("user32.dll", EntryPoint = "FindWindowW", StringMarshalling = StringMarshalling.Utf16)]
    private static partial nint FindWindow(string? className, string windowName);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool ShowWindow(nint window, int command);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool SetForegroundWindow(nint window);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool BringWindowToTop(nint window);

    [LibraryImport("user32.dll")]
    private static partial nint GetForegroundWindow();

    [LibraryImport("user32.dll")]
    private static partial uint GetWindowThreadProcessId(nint window, nint processId);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool AttachThreadInput(uint attach, uint attachTo, [MarshalAs(UnmanagedType.Bool)] bool join);

    [LibraryImport("kernel32.dll")]
    private static partial uint GetCurrentThreadId();
}
