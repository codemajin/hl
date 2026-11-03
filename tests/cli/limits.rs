//! `--max-size` and `--max-line-length`: where highlighting stops, and how
//! sizes are parsed from the options and the environment.

use std::{
    fs,
    path::{Path, PathBuf},
};

use predicates::prelude::*;

use crate::common::{colored, files, has_escape, hl, stdout, strip_ansi, tempdir};

/// A JS file with a short line, a `long`-byte line and another short line.
/// Returns the path, the head that should stay highlighted and the tail
/// (from the long line on) that should be printed as is.
fn long_line_fixture(dir: &Path, name: &str, long: usize) -> (PathBuf, Vec<u8>, Vec<u8>) {
    let head = b"let a = 1;\n".to_vec();
    let mut tail = "x".repeat(long).into_bytes();
    tail.extend_from_slice(b"\nlet b = 2;\n");
    let path = files(dir, &[(name, [head.as_slice(), &tail].concat())]).remove(0);
    (path, head, tail)
}

fn assert_plain_from_long_line(out: &[u8], head: &[u8], tail: &[u8]) {
    assert!(
        out.ends_with(tail),
        "the long line and later should be printed as is"
    );
    let colored = &out[..out.len() - tail.len()];
    assert!(
        has_escape(colored),
        "lines before the long line should be highlighted"
    );
    assert_eq!(strip_ansi(colored), head);
}

#[test]
fn prints_large_file_without_highlighting() {
    let dir = tempdir();
    let content = "fn main() {}\n".repeat(100);
    let path = files(dir.path(), &[("large.rs", &content)]);
    hl().args(["--color", "always", "--max-size", "1K"])
        .args(&path)
        .assert()
        .success()
        .stdout(content);
}

#[test]
fn stops_highlighting_large_stdin_at_the_limit() {
    let line = "fn main() {}\n";
    let input = line.repeat(100);
    let out = colored(
        hl().args(["-l", "rust", "--max-size", "1K"])
            .write_stdin(input.clone()),
    );
    // The line that crosses 1024 bytes and everything after it are plain.
    let highlighted_lines = 1024 / line.len();
    let tail = &input.as_bytes()[highlighted_lines * line.len()..];
    assert!(out.ends_with(tail));
    let colored = &out[..out.len() - tail.len()];
    assert!(has_escape(colored));
    assert_eq!(
        strip_ansi(colored),
        line.repeat(highlighted_lines).as_bytes()
    );
}

#[test]
fn stops_highlighting_at_a_long_line() {
    let dir = tempdir();
    let (path, head, tail) = long_line_fixture(dir.path(), "long.js", 100);
    let out = colored(hl().args(["--max-line-length", "64"]).arg(&path));
    assert_plain_from_long_line(&out, &head, &tail);
}

#[test]
fn highlights_a_line_exactly_at_the_limit() {
    let dir = tempdir();
    let (path, _, _) = long_line_fixture(dir.path(), "exact.js", 64);
    let out = colored(hl().args(["--max-line-length", "64"]).arg(&path));
    assert!(
        out.ends_with(b"\x1b[0m\n"),
        "the last line should be highlighted"
    );
}

#[test]
fn limits_line_length_by_default() {
    let dir = tempdir();
    let (path, head, tail) = long_line_fixture(dir.path(), "default.js", 20 * 1024);
    let out = colored(hl().arg(&path));
    assert_plain_from_long_line(&out, &head, &tail);
}

#[test]
fn zero_disables_the_limits() {
    let dir = tempdir();
    let (path, _, _) = long_line_fixture(dir.path(), "unlimited.js", 100);
    let out = colored(
        hl().args(["--max-size", "0", "--max-line-length", "0"])
            .arg(&path),
    );
    assert!(
        out.ends_with(b"\x1b[0m\n"),
        "the last line should be highlighted"
    );
}

#[test]
fn reads_limits_from_env() {
    let dir = tempdir();
    let (path, head, tail) = long_line_fixture(dir.path(), "env.js", 100);
    let out = colored(hl().env("HL_MAX_LINE_LENGTH", "64").arg(&path));
    assert_plain_from_long_line(&out, &head, &tail);

    let content = fs::read(&path).unwrap();
    hl().env("HL_MAX_SIZE", "64")
        .args(["--color", "always"])
        .arg(&path)
        .assert()
        .success()
        .stdout(content);
}

#[test]
fn keeps_numbering_and_squeezing_across_the_switch() {
    let dir = tempdir();
    let mut content = b"let a = 1;\n\n\n".to_vec();
    content.extend_from_slice("x".repeat(100).as_bytes());
    content.extend_from_slice(b"\n\n\nlet b = 2;\nno newline");
    let files = files(dir.path(), &[("switch.js", content)]);
    for opts in [&["-n"][..], &["-b"], &["-s"], &["-sn"]] {
        let out = colored(
            hl().args(["--max-line-length", "64"])
                .args(opts)
                .args(&files),
        );
        assert!(
            has_escape(&out),
            "hl {opts:?} should highlight the first line"
        );
        assert_eq!(
            strip_ansi(&out),
            stdout(hl().args(opts).args(&files)),
            "hl {opts:?} should print the same text as without color"
        );
    }
}

/// The sizes themselves are covered by the unit tests of `parse_size` in
/// `src/cli.rs`; this checks how clap reports a rejected one.
#[test]
fn rejects_an_invalid_size_with_status_2() {
    hl().args(["--max-size=1.5K", "Cargo.toml"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(
            "invalid value '1.5K' for '--max-size <SIZE>': not a size: use a whole number with an optional K, M or G suffix",
        ));
}

#[test]
fn reports_an_invalid_env_value_by_the_variable_name() {
    for (var, value, message) in [
        ("HL_MAX_SIZE", "abc", "not a size"),
        ("HL_MAX_LINE_LENGTH", "1.5K", "not a size"),
        (
            "HL_COLOR_DEPTH",
            "16",
            "(possible values: auto, truecolor, 256)",
        ),
    ] {
        hl().env(var, value)
            .arg("Cargo.toml")
            .assert()
            .code(2)
            .stdout("")
            .stderr(
                predicate::str::contains(format!("invalid value '{value}' for {var}"))
                    .and(predicate::str::contains(message))
                    .and(predicate::str::contains("'--").not()),
            );
    }
}

#[test]
fn ignores_an_invalid_env_value_overridden_on_the_command_line() {
    hl().env("HL_MAX_SIZE", "abc")
        .args(["--max-size", "1K", "Cargo.toml"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn reports_an_invalid_option_before_an_invalid_env_value() {
    hl().env("HL_MAX_SIZE", "abc")
        .args(["--max-line-length=x", "Cargo.toml"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "invalid value 'x' for '--max-line-length <LEN>'",
        ));
}
