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
    Parse(String),
    /// `$PAGER` is unset or blank and the default pager, `less`, is not
    /// installed. Not an error: the output goes to stdout instead.
    DefaultMissing,
    /// The pager program could not be run.
    Start { program: String, err: io::Error },
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "invalid PAGER: {err}"),
            Self::DefaultMissing => write!(
                f,
                "{DEFAULT_PAGER} not found, printing without a pager (set PAGER to choose one)"
            ),
            Self::Start { program, err } => {
                write!(f, "failed to start pager '{program}': {}", describe(err))
            }
        }
    }
}

impl std::error::Error for SpawnError {}

/// The pager used when `$PAGER` is unset or blank.
const DEFAULT_PAGER: &str = "less";

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
        let words = split(pager.unwrap_or_default()).map_err(SpawnError::Parse)?;
        let default = words.is_empty();
        let (program, args) = command(words, less_set, quit_if_one_screen);
        Self::start(&program, &args).map_err(|err| start_error(default, program, err))
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

/// The error for `program` failing to start with `err`; `default` tells
/// whether it is the default pager, whose absence is not an error.
fn start_error(default: bool, program: String, err: io::Error) -> SpawnError {
    if default && err.kind() == io::ErrorKind::NotFound {
        SpawnError::DefaultMissing
    } else {
        SpawnError::Start { program, err }
    }
}

/// The flags `less` needs to show our colors (`-R`) and to leave the output
/// on the screen (`-X`), plus `-F` to exit at once when it fits on one screen.
fn less_flags(quit_if_one_screen: bool) -> &'static str {
    if quit_if_one_screen { "-RFX" } else { "-RX" }
}

/// `$PAGER` split into words: with the word splitting rules of the shell,
/// or on Windows with those of [`split_windows`], where a backslash is the
/// path separator rather than an escape.
fn split(pager: &str) -> Result<Vec<String>, String> {
    if cfg!(windows) {
        split_windows(pager)
    } else {
        shell_words::split(pager).map_err(|e| e.to_string())
    }
}

/// Splits `s` at whitespace, keeping what is between double quotes together
/// (`"C:\Program Files\Git\usr\bin\less.exe" -R`). Backslashes are kept
/// as they are, and `""` is an empty word.
fn split_windows(s: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                word.get_or_insert_with(String::new);
            }
            c if c.is_whitespace() && !quoted => words.extend(word.take()),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    if quoted {
        return Err("missing closing quote".into());
    }
    words.extend(word);
    Ok(words)
}

/// Whether `program` is `less`, also as a path, and on Windows as
/// `less.exe` in any case.
fn is_less(program: &str) -> bool {
    let Some(name) = Path::new(program).file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if cfg!(windows) {
        name.eq_ignore_ascii_case("less") || name.eq_ignore_ascii_case("less.exe")
    } else {
        name == "less"
    }
}

/// The program and arguments to run for the `words` of `$PAGER`, or `less`
/// when there are none.
///
/// A bare `less` (no arguments, `$LESS` unset) gets the same flags as the
/// fallback; otherwise the user's own options are respected.
fn command(words: Vec<String>, less_set: bool, quit_if_one_screen: bool) -> (String, Vec<String>) {
    let mut words = words.into_iter();
    let Some(program) = words.next() else {
        return (
            DEFAULT_PAGER.into(),
            vec![less_flags(quit_if_one_screen).into()],
        );
    };
    let mut args: Vec<String> = words.collect();
    if is_less(&program) && args.is_empty() && !less_set {
        args.push(less_flags(quit_if_one_screen).into());
    }
    (program, args)
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

    /// The command for `pager`, the value of `$PAGER`.
    fn full(
        pager: Option<&str>,
        less_set: bool,
        quit_if_one_screen: bool,
    ) -> Result<(String, Vec<String>), String> {
        let words = split(pager.unwrap_or_default())?;
        Ok(command(words, less_set, quit_if_one_screen))
    }

    /// The command with `$LESS` unset, unwrapped.
    fn cmd(pager: Option<&str>, quit_if_one_screen: bool) -> (String, Vec<String>) {
        full(pager, false, quit_if_one_screen).unwrap()
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
        assert_eq!(full(None, true, true).unwrap(), words("less", &["-RFX"]));
    }

    #[test]
    fn pager_is_split_into_words() {
        assert_eq!(cmd(Some("more"), true), words("more", &[]));
        assert_eq!(
            cmd(Some("  less  -R   -S "), true),
            words("less", &["-R", "-S"])
        );
        assert_eq!(
            cmd(Some(r#""/opt/my pager/bin/pg" -x"#), false),
            words("/opt/my pager/bin/pg", &["-x"])
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn pager_is_split_like_the_shell() {
        assert_eq!(
            cmd(Some("sh -c 'exit 3'"), false),
            words("sh", &["-c", "exit 3"])
        );
        assert_eq!(cmd(Some(r"a\ b c"), false), words("a b", &["c"]));
    }

    #[test]
    fn splits_windows_commands_keeping_backslashes() {
        let split = |s| split_windows(s).unwrap();
        assert_eq!(split(r"C:\tools\less.exe -R"), [r"C:\tools\less.exe", "-R"]);
        assert_eq!(
            split(r#""C:\Program Files\Git\usr\bin\less.exe" -R"#),
            [r"C:\Program Files\Git\usr\bin\less.exe", "-R"]
        );
        assert_eq!(split(r#"a" b "c"#), ["a b c"]);
        assert_eq!(split(r#"pg "" x"#), ["pg", "", "x"]);
        assert_eq!(split("'a b'"), ["'a", "b'"]);
        assert_eq!(split(" \t "), Vec::<String>::new());
        assert_eq!(
            split_windows(r#""C:\x y"#).unwrap_err(),
            "missing closing quote"
        );
    }

    #[test]
    fn rejects_an_unclosed_quote() {
        assert!(full(Some(r#"sh -c "exit 3"#), false, false).is_err());
        let err = Pager::spawn(Some(r#"sh -c "exit 3"#), false, false)
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

    #[cfg(windows)]
    #[test]
    fn bare_less_exe_gets_the_default_flags() {
        for pager in [
            "less.exe",
            "LESS.EXE",
            r"C:\tools\less.exe",
            r#""C:\Program Files\less""#,
        ] {
            let (_, args) = cmd(Some(pager), true);
            assert_eq!(args, ["-RFX"], "{pager}");
        }
    }

    #[test]
    fn respects_the_users_less_options() {
        // Arguments of its own.
        assert_eq!(cmd(Some("less -S"), true), words("less", &["-S"]));
        // Options in `$LESS`.
        assert_eq!(full(Some("less"), true, true).unwrap(), words("less", &[]));
        // Other programs, even with a similar name.
        assert_eq!(cmd(Some("lesspipe"), true), words("lesspipe", &[]));
        assert_eq!(cmd(Some("most"), true), words("most", &[]));
    }

    #[test]
    fn a_missing_default_pager_is_not_an_error() {
        let not_found = || io::Error::from(io::ErrorKind::NotFound);
        let err = start_error(true, "less".into(), not_found());
        assert!(matches!(err, SpawnError::DefaultMissing), "{err:?}");
        assert_eq!(
            err.to_string(),
            "less not found, printing without a pager (set PAGER to choose one)"
        );
        // A pager the user chose is an error, even if it is `less`.
        let err = start_error(false, "less".into(), not_found());
        assert!(matches!(err, SpawnError::Start { .. }), "{err:?}");
        // So is a default pager that is there but fails to start.
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let err = start_error(true, "less".into(), denied);
        assert!(matches!(err, SpawnError::Start { .. }), "{err:?}");
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
