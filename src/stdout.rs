//! Whether stdout was open when `hl` started.
//!
//! On Unix, std reopens a closed stdin, stdout or stderr on `/dev/null`
//! before `main`, so a closed stdout (`hl >&-`) would silently swallow the
//! output and `hl` would exit with 0. A function run by the loader, before
//! std's own setup, looks at fd 1 first.
//!
//! On Windows, std leaves the standard handles alone, but writing to a
//! missing one (a null or invalid handle) succeeds without writing
//! anything. `check` asks for the handle itself.

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

    /// Run by the loader before `main` and before std reopens fd 1.
    extern "C" fn probe() {
        // SAFETY: F_GETFD only reads the descriptor flags of fd 1.
        if unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_GETFD) } == -1 {
            let code = io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EBADF);
            super::ERROR.store(code, Ordering::Relaxed);
        }
    }

    #[used]
    #[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".init_array"))]
    #[cfg_attr(
        target_vendor = "apple",
        unsafe(link_section = "__DATA,__mod_init_func")
    )]
    static PROBE: extern "C" fn() = probe;
}
