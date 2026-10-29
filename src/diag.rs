//! Messages on stderr.

use std::{io, path::Path};

/// Warns about a problem with `path` that does not stop `hl`.
pub fn warn(path: &Path, msg: &str) {
    eprintln!("hl: {}: {msg}", path.display());
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
    fn describe_keeps_other_messages() {
        let err = io::Error::other("custom");
        assert_eq!(describe(&err), "custom");
    }
}
