//! Whether stdout was open when `hl` started.
//!
//! Before `main`, std reopens a closed stdin, stdout or stderr on
//! `/dev/null`, so a closed stdout (`hl >&-`) would silently swallow the
//! output and `hl` would exit with 0. A function run by the loader, before
//! std's own setup, looks at fd 1 first.

use std::{
    io,
    sync::atomic::{AtomicI32, Ordering},
};

/// The error of checking fd 1 before `main`, or 0 if it was open.
static ERROR: AtomicI32 = AtomicI32::new(0);

/// An error if stdout was closed when `hl` started. Platforms without the
/// check always get `Ok`.
pub fn check() -> io::Result<()> {
    match ERROR.load(Ordering::Relaxed) {
        0 => Ok(()),
        code => Err(io::Error::from_raw_os_error(code)),
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
