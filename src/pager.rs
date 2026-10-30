use std::{
    env,
    io::{self, Write},
    process::{Child, ChildStdin, Command, Stdio},
};

/// A running pager process whose stdin receives our output.
///
/// Dropping it closes the stdin and waits for the pager, so the shell prompt
/// never comes back while the pager still owns the terminal.
pub struct Pager {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Pager {
    /// Starts `$PAGER`, falling back to `less -RX`. With `quit_if_one_screen`
    /// the fallback is `less -RFX`, which exits at once when the output fits
    /// on one screen; `$PAGER` is run as given either way.
    pub fn spawn(quit_if_one_screen: bool) -> io::Result<Self> {
        let (program, args) = command(env::var("PAGER").ok().as_deref(), quit_if_one_screen);
        Self::start(&program, &args)
    }

    /// Starts `program` with `args`, piping our output to its stdin.
    fn start(program: &str, args: &[String]) -> io::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    pub fn stdin(&mut self) -> &mut ChildStdin {
        self.stdin.as_mut().expect("pager stdin is piped")
    }

    /// Closes the pager's stdin and waits for the user to quit it, reporting
    /// a failure to wait that `Drop` would have to ignore.
    pub fn finish(mut self) -> io::Result<()> {
        self.close_stdin();
        self.child.wait()?;
        Ok(())
    }

    fn close_stdin(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.flush();
        }
    }
}

/// The program and arguments to run for `$PAGER` (`pager`), split at
/// whitespace, or the `less` fallback when it is unset or blank.
fn command(pager: Option<&str>, quit_if_one_screen: bool) -> (String, Vec<String>) {
    let mut parts = pager
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from);
    if let Some(program) = parts.next() {
        return (program, parts.collect());
    }
    let flags = if quit_if_one_screen { "-RFX" } else { "-RX" };
    ("less".to_string(), vec![flags.to_string()])
}

impl Drop for Pager {
    fn drop(&mut self) {
        self.close_stdin();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_less() {
        assert_eq!(command(None, false), ("less".into(), vec!["-RX".into()]));
        assert_eq!(command(None, true), ("less".into(), vec!["-RFX".into()]));
        assert_eq!(
            command(Some(""), true),
            ("less".into(), vec!["-RFX".into()])
        );
        assert_eq!(
            command(Some(" \t"), false),
            ("less".into(), vec!["-RX".into()])
        );
    }

    #[test]
    fn pager_is_split_at_whitespace() {
        assert_eq!(command(Some("more"), true), ("more".into(), vec![]));
        assert_eq!(
            command(Some("  less  -R   -S "), true),
            ("less".into(), vec!["-R".into(), "-S".into()])
        );
    }

    /// A pager that copies its input to `path` instead of the terminal.
    #[cfg(unix)]
    fn recording_pager(path: &std::path::Path) -> Pager {
        let args = [
            "-c".into(),
            r#"cat > "$0""#.into(),
            path.display().to_string(),
        ];
        Pager::start("sh", &args).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn finish_waits_for_the_pager_to_read_everything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paged");
        let mut pager = recording_pager(&path);
        pager.stdin().write_all(b"hello\n").unwrap();
        pager.finish().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello\n");
    }

    #[cfg(unix)]
    #[test]
    fn drop_waits_for_the_pager_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paged");
        let mut pager = recording_pager(&path);
        pager.stdin().write_all(b"hello\n").unwrap();
        drop(pager);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello\n");
    }

    #[test]
    fn reports_a_missing_pager() {
        let err = Pager::start("hl-no-such-pager", &[]).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn ignores_the_exit_status_of_the_pager() {
        let args = ["-c".into(), "exit 1".into()];
        Pager::start("sh", &args).unwrap().finish().unwrap();
    }
}
