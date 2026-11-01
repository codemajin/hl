//! Messages on stderr.

use std::{
    borrow::Cow,
    fmt::{self, Write},
    io::{self, Write as _},
    path::Path,
};

/// Prints `hl: msg` on stderr. A failed write is ignored: there is nowhere
/// left to report it, and it must change neither the output nor the exit
/// code (`eprintln!` would panic, for example on a closed pipe).
pub fn note(msg: impl fmt::Display) {
    let _ = writeln!(io::stderr().lock(), "hl: {msg}");
}

/// Warns about a problem with `path` that does not stop `hl`.
pub fn warn(path: &Path, msg: &str) {
    note(format_args!("{}: {msg}", display_safe(path)));
}

/// `path` as shown to the user, with its control characters made visible so
/// that a crafted file name cannot control the terminal.
pub fn display_safe(path: &Path) -> Cow<'_, str> {
    match path.to_string_lossy() {
        Cow::Borrowed(s) => escape_controls(s),
        Cow::Owned(s) => Cow::Owned(escape_controls(&s).into_owned()),
    }
}

/// `s` with the C0 controls, DEL and the C1 controls written as `\x1b`.
pub fn escape_controls(s: &str) -> Cow<'_, str> {
    // `char::is_control` is exactly C0, DEL and C1.
    if !s.contains(char::is_control) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c.is_control() {
            let _ = write!(out, "\\x{:02x}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Strips the "(os error N)" suffix to match coreutils-style messages.
pub fn describe(err: &io::Error) -> String {
    let msg = err.to_string();
    match msg.find(" (os error") {
        Some(i) => msg[..i].to_string(),
        None => msg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_strips_the_os_error_code() {
        let err = io::Error::from_raw_os_error(2);
        let full = err.to_string();
        let msg = describe(&err);
        assert!(
            full.starts_with(&msg) && full.contains("(os error 2)"),
            "{full}"
        );
        assert!(!msg.contains("os error"), "{msg}");
    }

    #[test]
    fn escapes_control_characters() {
        assert_eq!(
            escape_controls("a\x1b]0;t\x07\tb\x7f\u{9b}c"),
            r"a\x1b]0;t\x07\x09b\x7f\x9bc"
        );
    }

    #[test]
    fn keeps_text_without_control_characters() {
        let text = "日本語 é\u{a0}~";
        assert!(matches!(escape_controls(text), Cow::Borrowed(s) if s == text));
    }

    #[test]
    fn displays_paths_safely() {
        assert_eq!(display_safe(Path::new("dir/a\x1b[2Jb")), r"dir/a\x1b[2Jb");
        assert_eq!(display_safe(Path::new("dir/a.rs")), "dir/a.rs");
    }

    #[cfg(unix)]
    #[test]
    fn displays_non_utf8_paths_safely() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        let path = Path::new(OsStr::from_bytes(b"\xff\x1b"));
        assert_eq!(display_safe(path), "\u{FFFD}\\x1b");
    }

    #[test]
    fn describe_keeps_other_messages() {
        let err = io::Error::other("custom");
        assert_eq!(describe(&err), "custom");
    }
}
