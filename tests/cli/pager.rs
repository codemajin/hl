//! The pager.

use std::fs;

use predicates::prelude::*;

use crate::common::{fixture, hl, tempdir};

#[test]
fn never_starts_a_pager_without_a_terminal() {
    let dir = tempdir();
    let marker = dir.path().join("paged");
    let input = fixture("sample.toml");
    let content = fs::read(&input).unwrap();
    for opts in [&["-p"][..], &["--paging=always"], &["--paging=auto"]] {
        // The marker is relative to the working directory, so that PAGER
        // holds no path that could contain spaces. There is no `touch` on
        // Windows, but the test holds there too: a pager that hl tried to
        // start would fail, and so would `hl`.
        hl().args(opts)
            .arg(&input)
            .current_dir(dir.path())
            .env("PAGER", "touch paged")
            .assert()
            .success()
            .stdout(content.clone());
        assert!(!marker.exists(), "pager started with {opts:?}");
    }
}

/// Without a terminal both orders print the input as it is; which one wins
/// is checked by the unit tests in cli.rs.
#[test]
fn accepts_p_together_with_paging() {
    let content = fs::read(fixture("sample.toml")).unwrap();
    for opts in [["-p", "--paging=never"], ["--paging=never", "-p"]] {
        hl().args(opts)
            .arg(fixture("sample.toml"))
            .assert()
            .success()
            .stdout(content.clone());
    }
}

#[test]
fn rejects_unknown_paging_mode() {
    hl().args(["--paging=sometimes"])
        .arg(fixture("sample.toml"))
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'sometimes'"));
}

/// Runs `hl --paging=always input` on a pseudo-terminal made by `script`
/// (util-linux), with `pager` as `PAGER` and `dir` as the working directory,
/// and returns its exit status and what reached the terminal. `None` when
/// there is no `script`, after saying that `test` is skipped.
#[cfg(target_os = "linux")]
fn on_a_terminal(
    test: &str,
    dir: &std::path::Path,
    pager: &str,
    input: &std::path::Path,
) -> Option<(std::process::ExitStatus, String)> {
    use std::io::{ErrorKind, Write};

    // The paths go through variables, so that no quoting is needed.
    let result = crate::common::isolated("script")
        .args([
            "-qec",
            r#""$HL_BIN" --paging=always "$HL_INPUT""#,
            "/dev/null",
        ])
        .env("HL_BIN", env!("CARGO_BIN_EXE_hl"))
        .env("HL_INPUT", input)
        .env("SHELL", "/bin/sh")
        .env("PAGER", pager)
        .current_dir(dir)
        .output();
    match result {
        Ok(out) => Some((
            out.status,
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // Written to stderr directly, as `eprintln!` would be captured.
            let _ = writeln!(
                std::io::stderr(),
                "skipping {test}: `script` (util-linux) not found"
            );
            None
        }
        Err(e) => panic!("cannot run script: {e}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn pages_the_output_on_a_terminal() {
    let dir = tempdir();
    let input = fixture("sample.toml");
    let Some((status, terminal)) = on_a_terminal(
        "pages_the_output_on_a_terminal",
        dir.path(),
        "sh -c 'cat > paged'",
        &input,
    ) else {
        return;
    };
    assert!(status.success(), "{status}: {terminal}");
    assert!(!terminal.contains("hl:"), "{terminal}");
    let paged = fs::read(dir.path().join("paged")).unwrap();
    assert_eq!(crate::common::strip_ansi(&paged), fs::read(&input).unwrap());
}

/// A pager that quits before reading everything, as `less` does when the
/// user presses `q`.
#[cfg(target_os = "linux")]
#[test]
fn a_pager_that_quits_early_is_not_an_error() {
    let dir = tempdir();
    // Far more than a pipe holds, so that writing to the pager fails.
    let input = crate::common::files(dir.path(), &[("big.txt", "line\n".repeat(200_000))]);
    let Some((status, terminal)) = on_a_terminal(
        "a_pager_that_quits_early_is_not_an_error",
        dir.path(),
        "sh -c 'head -c 10 > paged'",
        &input[0],
    ) else {
        return;
    };
    assert!(status.success(), "{status}: {terminal}");
    assert!(!terminal.contains("hl:"), "{terminal}");
    // The pager did start, and read only what it asked for.
    assert_eq!(fs::read(dir.path().join("paged")).unwrap().len(), 10);
}
