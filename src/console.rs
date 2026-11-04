//! The Windows console: escape sequences are shown as text there unless
//! virtual terminal processing is turned on for the screen buffer.

/// Turns on virtual terminal processing for stdout when it is a Windows
/// console, so that the colors are shown rather than their escape
/// sequences. A failure is ignored: an older console shows them as text,
/// as it would without the call. Does nothing on other platforms.
pub fn enable_vt() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
            SetConsoleMode,
        };

        // SAFETY: GetStdHandle takes no pointers. GetConsoleMode writes to
        // `mode`, which lives for the call, and fails on a handle that is
        // not a console (null, invalid, a file or a pipe), which is then
        // left alone.
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut mode = 0;
            if GetConsoleMode(handle, &raw mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}
