use std::{
    fmt,
    io::{self, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
};

use crate::diag::describe;

/// A running pager process whose stdin receives our output.
///
/// Dropping it closes the stdin and waits for the pager, so the shell prompt
/// never comes back while the pager still owns the terminal.
pub struct Pager {
    child: Child,
    stdin: Option<ChildStdin>,
}

/// Why the pager could not be started.
#[derive(Debug)]
pub enum SpawnError {
    /// `$PAGER` has an unclosed quote.
    Parse(shell_words::ParseError),
    /// The pager program could not be run.
    Start { program: String, err: io::Error },
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "invalid PAGER: {err}"),
            Self::Start { program, err } => {
                write!(f, "failed to start pager '{program}': {}", describe(err))
            }
        }
    }
}

impl std::error::Error for SpawnError {}

impl Pager {
    /// Starts the pager for `pager`, the value of `$PAGER` (see [`command`]).
    /// `less_set` tells whether `$LESS` is set, and `quit_if_one_screen`
    /// whether the pager should exit at once when the output fits on one
    /// screen.
    pub fn spawn(
        pager: Option<&str>,
        less_set: bool,
        quit_if_one_screen: bool,
    ) -> Result<Self, SpawnError> {
        let (program, args) =
            command(pager, less_set, quit_if_one_screen).map_err(SpawnError::Parse)?;
        Self::start(&program, &args).map_err(|err| SpawnError::Start { program, err })
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

/// The flags `less` needs to show our colors (`-R`) and to leave the output
/// on the screen (`-X`), plus `-F` to exit at once when it fits on one screen.
fn less_flags(quit_if_one_screen: bool) -> &'static str {
    if quit_if_one_screen { "-RFX" } else { "-RX" }
}

/// The program and arguments to run for `$PAGER` (`pager`), split with the
/// word splitting rules of the shell, or `less` when it is unset or blank.
///
/// A bare `less` (no arguments, `$LESS` unset) gets the same flags as the
/// fallback; otherwise the user's own options are respected.
fn command(
    pager: Option<&str>,
    less_set: bool,
    quit_if_one_screen: bool,
) -> Result<(String, Vec<String>), shell_words::ParseError> {
    let mut words = shell_words::split(pager.unwrap_or_default())?.into_iter();
    let Some(program) = words.next() else {
        return Ok(("less".into(), vec![less_flags(quit_if_one_screen).into()]));
    };
    let mut args: Vec<String> = words.collect();
    let is_less = Path::new(&program).file_name().is_some_and(|n| n == "less");
    if is_less && args.is_empty() && !less_set {
        args.push(less_flags(quit_if_one_screen).into());
    }
    Ok((program, args))
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

    /// `command` with `$LESS` unset, unwrapped.
    fn cmd(pager: Option<&str>, quit_if_one_screen: bool) -> (String, Vec<String>) {
        command(pager, false, quit_if_one_screen).unwrap()
    }

    fn words(program: &str, args: &[&str]) -> (String, Vec<String>) {
        (program.into(), args.iter().map(|&a| a.into()).collect())
    }

    #[test]
    fn falls_back_to_less() {
        assert_eq!(cmd(None, false), words("less", &["-RX"]));
        assert_eq!(cmd(None, true), words("less", &["-RFX"]));
        assert_eq!(cmd(Some(""), true), words("less", &["-RFX"]));
        assert_eq!(cmd(Some(" \t"), false), words("less", &["-RX"]));
        // The fallback is the same with `$LESS` set.
        assert_eq!(command(None, true, true).unwrap(), words("less", &["-RFX"]));
    }

    #[test]
    fn pager_is_split_like_the_shell() {
        assert_eq!(cmd(Some("more"), true), words("more", &[]));
        assert_eq!(
            cmd(Some("  less  -R   -S "), true),
            words("less", &["-R", "-S"])
        );
        assert_eq!(
            cmd(Some("sh -c 'exit 3'"), false),
            words("sh", &["-c", "exit 3"])
        );
        assert_eq!(
            cmd(Some(r#""/opt/my pager/bin/pg" -x"#), false),
            words("/opt/my pager/bin/pg", &["-x"])
        );
    }

    #[test]
    fn rejects_an_unclosed_quote() {
        assert!(command(Some("sh -c 'exit 3"), false, false).is_err());
        let err = Pager::spawn(Some("sh -c 'exit 3"), false, false)
            .err()
            .unwrap();
        assert_eq!(err.to_string(), "invalid PAGER: missing closing quote");
    }

    #[test]
    fn bare_less_gets_the_default_flags() {
        assert_eq!(cmd(Some("less"), false), words("less", &["-RX"]));
        assert_eq!(cmd(Some("less"), true), words("less", &["-RFX"]));
        assert_eq!(
            cmd(Some("/usr/bin/less"), true),
            words("/usr/bin/less", &["-RFX"])
        );
    }

    #[test]
    fn respects_the_users_less_options() {
        // Arguments of its own.
        assert_eq!(cmd(Some("less -S"), true), words("less", &["-S"]));
        // Options in `$LESS`.
        assert_eq!(
            command(Some("less"), true, true).unwrap(),
            words("less", &[])
        );
        // Other programs, even with a similar name.
        assert_eq!(cmd(Some("lesspipe"), true), words("lesspipe", &[]));
        assert_eq!(cmd(Some("most"), true), words("most", &[]));
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
    fn reports_a_missing_pager_by_name() {
        let err = Pager::spawn(Some("hl-no-such-pager --flag"), false, false)
            .err()
            .unwrap();
        let SpawnError::Start {
            program,
            err: source,
        } = &err
        else {
            panic!("{err:?}");
        };
        assert_eq!(program, "hl-no-such-pager");
        assert_eq!(source.kind(), io::ErrorKind::NotFound);
        let msg = err.to_string();
        assert!(
            msg.starts_with("failed to start pager 'hl-no-such-pager': "),
            "{msg}"
        );
        assert!(!msg.contains("os error"), "{msg}");
        #[cfg(unix)]
        assert_eq!(
            msg,
            "failed to start pager 'hl-no-such-pager': No such file or directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ignores_the_exit_status_of_the_pager() {
        let args = ["-c".into(), "exit 1".into()];
        Pager::start("sh", &args).unwrap().finish().unwrap();
    }
}
