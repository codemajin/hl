//! Reading the inputs and coloring the output.

use std::fs;

use predicates::prelude::*;

use crate::common::{ESC, NOT_FOUND, colored, files, fixture, hl, tempdir};

#[test]
fn passes_bytes_through_when_not_a_tty() {
    let content = fs::read(fixture("sample.rs")).unwrap();
    hl().arg(fixture("sample.rs"))
        .assert()
        .success()
        .stdout(content);
}

#[test]
fn colors_output_when_forced() {
    hl().args(["--color", "always"])
        .arg(fixture("sample.toml"))
        .assert()
        .success()
        .stdout(predicate::str::contains(ESC));
}

#[test]
fn never_colors_when_disabled() {
    hl().args(["--color", "never"])
        .arg(fixture("sample.toml"))
        .assert()
        .success()
        .stdout(predicate::str::contains(ESC).not());
}

#[test]
fn reports_missing_file_and_continues() {
    let dir = tempdir();
    let ok = files(dir.path(), &[("ok.txt", "hello\n")]);
    hl().arg(dir.path().join("missing.txt"))
        .args(&ok)
        .assert()
        .code(1)
        .stdout("hello\n")
        .stderr(predicate::str::contains(NOT_FOUND));
}

#[test]
fn reports_a_directory_and_continues() {
    let dir = tempdir();
    let ok = files(dir.path(), &[("ok.txt", "hello\n")]);
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    hl().arg(&sub)
        .args(&ok)
        .assert()
        .code(1)
        .stdout("hello\n")
        .stderr(format!("hl: {}: Is a directory\n", sub.display()));
}

#[test]
fn reads_stdin() {
    hl().write_stdin("abc\n").assert().success().stdout("abc\n");
    hl().arg("-")
        .write_stdin("abc\n")
        .assert()
        .success()
        .stdout("abc\n");
}

#[test]
fn highlights_stdin_with_explicit_language() {
    let source = "fn main() {}\n";
    let dir = tempdir();
    let file = files(dir.path(), &[("main.rs", source)]);
    let expected = colored(hl().args(&file));
    let rust = colored(hl().args(["-l", "rust"]).write_stdin(source));
    assert_eq!(
        rust, expected,
        "-l rust should highlight stdin like a .rs file"
    );
    let plain = colored(hl().write_stdin(source));
    assert_ne!(
        plain, expected,
        "stdin should not be detected as Rust without -l"
    );
}

#[test]
fn rejects_unknown_language() {
    hl().args(["--color", "always", "-l", "no-such-lang"])
        .write_stdin("x\n")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown language"));
}

#[test]
fn passes_binary_through_unhighlighted() {
    let dir = tempdir();
    let bytes = b"\x00\x01\x02hello\n\xff";
    let bin = files(dir.path(), &[("data.bin", bytes)]);
    hl().args(&bin).assert().success().stdout(&bytes[..]);
    // Only the control characters that act on the terminal are removed.
    hl().args(["--color", "always"])
        .args(&bin)
        .assert()
        .success()
        .stdout(&b"\x00hello\n\xff"[..]);
}

#[test]
fn keeps_missing_trailing_newline() {
    hl().args(["--color", "always"])
        .write_stdin("no newline")
        .assert()
        .success()
        .stdout(predicate::str::ends_with("\x1b[0m"));
}

/// A line without a newline, longer than the chunks it is read in and than
/// `--max-line-length`, is numbered and printed whole, with or without
/// color. That a chunk at a time is held in memory (issue 024) is checked
/// by the unit tests in `src/lines.rs`.
#[test]
fn numbers_a_long_line_without_newline() {
    let input = vec![b'x'; 256 * 1024 + 1];
    let mut expected = b"     1\t".to_vec();
    expected.extend_from_slice(&input);
    for color in ["never", "always"] {
        let out = hl()
            .args(["--color", color, "-l", "rust", "-n"])
            .write_stdin(input.clone())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert!(out == expected, "--color={color}: output differs");
    }
}
