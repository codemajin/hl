//! Errors writing to stdout and stderr: full disks, closed pipes and a
//! closed stdout.

#[cfg(unix)]
use std::fs;

use crate::common::raw_hl;
#[cfg(unix)]
use crate::common::{files, strip_ansi, tempdir};

#[cfg(target_os = "linux")]
#[test]
fn reports_write_errors_without_blaming_the_input() {
    let full = fs::File::create("/dev/full").unwrap();
    let output = raw_hl(&["src/main.rs", "Cargo.toml"])
        .stdout(full)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "hl: write error: No space left on device\n"
    );
}

/// The commands that print something and exit without reading the inputs.
const EARLY_EXIT_COMMANDS: [&[&str]; 5] = [
    &["--completions", "zsh"],
    &["--man"],
    &["--list-languages"],
    &["--list-themes"],
    &["--build-cache"],
];

/// `raw_hl` with stderr captured.
fn early_exit_command(args: &[&str]) -> std::process::Command {
    let mut cmd = raw_hl(args);
    cmd.stderr(std::process::Stdio::piped());
    cmd
}

#[cfg(target_os = "linux")]
#[test]
fn reports_write_errors_of_early_exit_commands() {
    for args in EARLY_EXIT_COMMANDS {
        let output = early_exit_command(args)
            .stdout(fs::File::create("/dev/full").unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "hl: write error: No space left on device\n",
            "{args:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn clear_cache_prints_nothing_to_fail_on() {
    let output = early_exit_command(&["--clear-cache"])
        .stdout(fs::File::create("/dev/full").unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn early_exit_commands_stop_quietly_when_the_pipe_closes() {
    for args in EARLY_EXIT_COMMANDS {
        let mut child = early_exit_command(args)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "", "{args:?}");
    }
}

/// A pipe whose reading end is already closed, so writing to it fails with
/// a broken pipe.
#[cfg(unix)]
fn closed_pipe() -> std::io::PipeWriter {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    writer
}

#[cfg(unix)]
#[test]
fn keeps_the_exit_code_when_stderr_is_a_closed_pipe() {
    let status = raw_hl(&["nonexistent-file"])
        .stderr(closed_pipe())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
}

/// The warnings about the config file come before any output.
#[cfg(unix)]
#[test]
fn prints_the_input_when_warnings_cannot_be_written() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "foo = 1\n")]);
    let output = raw_hl(&["--theme", "nope", "--color=always", "Cargo.toml"])
        .env("HL_CONFIG", &config[0])
        .stderr(closed_pipe())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(strip_ansi(&output.stdout), fs::read("Cargo.toml").unwrap());
}

/// `cmd` with its stdout closed, as with `hl >&-`.
#[cfg(unix)]
fn with_closed_stdout(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::{
        fd::{FromRawFd, OwnedFd},
        unix::process::CommandExt,
    };
    // SAFETY: fd 1 of the child is not used by anything else before exec,
    // and closing it is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            drop(OwnedFd::from_raw_fd(1));
            Ok(())
        })
    }
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
#[test]
fn reports_a_closed_stdout_as_a_write_error() {
    let normal: &[&str] = &["Cargo.toml"];
    for args in EARLY_EXIT_COMMANDS.into_iter().chain([normal]) {
        let output = with_closed_stdout(&mut early_exit_command(args))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "hl: write error: Bad file descriptor\n",
            "{args:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn clear_cache_does_not_need_stdout() {
    let output = with_closed_stdout(&mut early_exit_command(&["--clear-cache"]))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}
