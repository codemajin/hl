//! Whether stdout was open when `hl` started.
//!
//! On Unix, std reopens a closed stdin, stdout or stderr on `/dev/null`
//! before `main`, so a closed stdout (`hl >&-`) would silently swallow the
//! output and `hl` would exit with 0. A function run by the loader, before
//! std's own setup, looks at fd 1 first.
//!
//! On Windows, std leaves the standard handles alone, but writing to a
//! missing one (a null or invalid handle) succeeds without writing
//! anything. `check` asks for the handle itself. The console also shows
//! escape sequences as text unless virtual terminal processing is turned
//! on for it, which `enable_vt` does.
//!
//! These calls into the system are the only `unsafe` code of `hl`: the
//! `unsafe_code` lint is denied everywhere else (see `Cargo.toml`).
#![expect(
    unsafe_code,
    reason = "fd 1 is checked from the loader, and the Windows calls take handles"
)]

use std::{
    io,
    sync::atomic::{AtomicI32, Ordering},
};

/// The error of checking fd 1 before `main`, or 0 if it was open.
static ERROR: AtomicI32 = AtomicI32::new(0);

/// An error if stdout was closed when `hl` started. Platforms without the
/// check always get `Ok`.
pub fn check() -> io::Result<()> {
    #[cfg(windows)]
    windows::check()?;
    match ERROR.load(Ordering::Relaxed) {
        0 => Ok(()),
        code => Err(io::Error::from_raw_os_error(code)),
    }
}

/// Turns on virtual terminal processing for stdout when it is a Windows
/// console, so that the colors are shown rather than their escape
/// sequences. A failure is ignored: an older console shows them as text,
/// as it would without the call. Does nothing on other platforms.
pub fn enable_vt() {
    #[cfg(windows)]
    windows::enable_vt();
}

#[cfg(windows)]
mod windows {
    use std::io;

    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_HANDLE, HANDLE, INVALID_HANDLE_VALUE},
        System::Console::{GetStdHandle, STD_OUTPUT_HANDLE},
    };

    /// An error if the process has no stdout handle.
    pub fn check() -> io::Result<()> {
        // SAFETY: GetStdHandle takes no pointers and only reads the handle
        // of the process.
        let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        if is_missing(handle) {
            #[expect(clippy::cast_possible_wrap, reason = "a small error code")]
            return Err(io::Error::from_raw_os_error(ERROR_INVALID_HANDLE as i32));
        }
        Ok(())
    }

    /// See [`super::enable_vt`].
    pub fn enable_vt() {
        use windows_sys::Win32::System::Console::{
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, SetConsoleMode,
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

    /// Whether `handle`, as `GetStdHandle` returns it, is missing: null when
    /// the process has none, `INVALID_HANDLE_VALUE` when the call failed.
    fn is_missing(handle: HANDLE) -> bool {
        handle.is_null() || handle == INVALID_HANDLE_VALUE
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn tells_missing_handles() {
            assert!(is_missing(std::ptr::null_mut()));
            assert!(is_missing(INVALID_HANDLE_VALUE));
            // SAFETY: as in `check`.
            let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
            // The test harness runs with a stdout.
            assert!(!is_missing(handle));
        }
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_vendor = "apple"
))]
mod probe {
    use std::{io, sync::atomic::Ordering};

    /// Run by the loader before `main` and before std reopens fd 1. It
    /// must only read errno and store to an atomic (see `PROBE`).
    extern "C" fn probe() {
        // SAFETY: F_GETFD only reads the descriptor flags of fd 1.
        if unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_GETFD) } == -1 {
            let code = io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EBADF);
            super::ERROR.store(code, Ordering::Relaxed);
        }
    }

    // SAFETY: the loader calls each function in this section once, before
    // `main` and so before std's runtime setup, which reopens a closed fd 1
    // on `/dev/null`. Nothing of std that the setup prepares may be used
    // there, and `probe` only calls `fcntl`, reads errno through
    // `io::Error::last_os_error` (which does not allocate) and stores to an
    // atomic. Some loaders pass arguments (`argc`, `argv`, `envp`, ...), but
    // with the C ABI the caller cleans them up, so a function that takes
    // none, `extern "C" fn()`, is fine to call that way. The section names
    // are those of the initializer arrays of ELF and Mach-O.
    #[used]
    #[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".init_array"))]
    #[cfg_attr(
        target_vendor = "apple",
        unsafe(link_section = "__DATA,__mod_init_func")
    )]
    static PROBE: extern "C" fn() = probe;
}
