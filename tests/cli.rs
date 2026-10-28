use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

const ESC: &str = "\x1b[";

fn hl() -> Command {
    let mut cmd = Command::cargo_bin("hl").unwrap();
    cmd.env_remove("HL_THEME");
    cmd
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hl-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn passes_bytes_through_when_not_a_tty() {
    let content = fs::read("src/main.rs").unwrap();
    hl().arg("src/main.rs").assert().success().stdout(content);
}

#[test]
fn colors_output_when_forced() {
    hl().args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains(ESC));
}

#[test]
fn never_colors_when_disabled() {
    hl().args(["--color", "never", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains(ESC).not());
}

#[test]
fn numbers_lines_across_files() {
    let dir = tempdir();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    fs::write(&a, "one\ntwo\n").unwrap();
    fs::write(&b, "three\n").unwrap();
    hl().arg("-n")
        .args([&a, &b])
        .assert()
        .success()
        .stdout("     1\tone\n     2\ttwo\n     3\tthree\n");
}

#[test]
fn reports_missing_file_and_continues() {
    let dir = tempdir();
    let ok = dir.join("ok.txt");
    fs::write(&ok, "hello\n").unwrap();
    hl().arg(dir.join("missing.txt"))
        .arg(&ok)
        .assert()
        .failure()
        .stdout("hello\n")
        .stderr(predicate::str::contains("No such file or directory"));
}

#[test]
fn reads_stdin() {
    hl().write_stdin("abc\n").assert().success().stdout("abc\n");
    hl().arg("-").write_stdin("abc\n").assert().success().stdout("abc\n");
}

#[test]
fn highlights_stdin_with_explicit_language() {
    let plain = hl()
        .args(["--color", "always"])
        .write_stdin("fn main() {}\n")
        .output()
        .unwrap()
        .stdout;
    let rust = hl()
        .args(["--color", "always", "-l", "rust"])
        .write_stdin("fn main() {}\n")
        .output()
        .unwrap()
        .stdout;
    assert_ne!(plain, rust, "rust highlighting should differ from plain text");
}

#[test]
fn rejects_unknown_language() {
    hl().args(["--color", "always", "-l", "no-such-lang"])
        .write_stdin("x\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown language"));
}

#[test]
fn passes_binary_through_unmodified() {
    let dir = tempdir();
    let bin = dir.join("data.bin");
    let bytes = b"\x00\x01\x02hello\n\xff".to_vec();
    fs::write(&bin, &bytes).unwrap();
    hl().args(["--color", "always"])
        .arg(&bin)
        .assert()
        .success()
        .stdout(bytes);
}

#[test]
fn keeps_missing_trailing_newline() {
    hl().args(["--color", "always"])
        .write_stdin("no newline")
        .assert()
        .success()
        .stdout(predicate::str::ends_with("\x1b[0m"));
}

#[test]
fn lists_themes() {
    hl().arg("--list-themes")
        .assert()
        .success()
        .stdout(predicate::str::contains("base16-ocean.dark"));
}

/// Writes two fixture files exercising the cases `cat` options care about.
/// With `binary`, the first one also has control characters and high bytes.
fn cat_fixtures(prefix: &str, binary: bool) -> Vec<std::path::PathBuf> {
    let dir = tempdir();
    let first = dir.join(format!("{prefix}-1.rs"));
    let second = dir.join(format!("{prefix}-2.txt"));
    let mut bytes = b"fn main() {\r\n\tlet x = 1;\n\n\n\n  \n".to_vec();
    if binary {
        bytes.extend_from_slice(b"\x01\x1b\x7f\x80\x89\xa0\xff\n");
    }
    bytes.extend_from_slice(b"}\n\n\nno newline");
    fs::write(&first, bytes).unwrap();
    fs::write(&second, "\n\nnext\tline\n\n\n").unwrap();
    vec![first, second]
}

fn cat_output(opts: &[&str], files: &[std::path::PathBuf]) -> Vec<u8> {
    let out = std::process::Command::new("cat")
        .args(opts)
        .args(files)
        .output()
        .unwrap();
    assert!(out.status.success(), "cat {opts:?} failed");
    out.stdout
}

fn hl_output(color: &str, opts: &[&str], files: &[std::path::PathBuf]) -> Vec<u8> {
    let out = hl()
        .args(["--color", color])
        .args(opts)
        .args(files)
        .output()
        .unwrap();
    assert!(out.status.success(), "hl {opts:?} failed");
    out.stdout
}

/// Removes SGR escape sequences (`ESC [ ... m`).
fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\x1b[") {
            match bytes[i..].iter().position(|&b| b == b'm') {
                Some(end) => i += end + 1,
                None => break,
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

const CAT_OPTIONS: &[&[&str]] = &[
    &["-A"],
    &["-v"],
    &["-e"],
    &["-E"],
    &["-t"],
    &["-T"],
    &["-s"],
    &["-b"],
    &["-u"],
    &["-n"],
    &["-sn"],
    &["-sb"],
    &["-bn"],
    &["-vT"],
    &["-sA"],
    &["--show-all", "--number"],
];

#[test]
fn matches_cat_options_without_color() {
    let files = cat_fixtures("plain", true);
    for opts in CAT_OPTIONS {
        assert_eq!(
            hl_output("never", opts, &files),
            cat_output(opts, &files),
            "hl {opts:?} differs from cat"
        );
    }
}

#[test]
fn visualizing_options_disable_highlighting() {
    let files = cat_fixtures("visual", true);
    for opts in [["-A"], ["-v"], ["-e"], ["-E"], ["-t"], ["-T"]] {
        assert_eq!(
            hl_output("always", &opts, &files),
            cat_output(&opts, &files),
            "hl --color always {opts:?} differs from cat"
        );
    }
}

#[test]
fn squeeze_and_numbering_work_with_highlighting() {
    let files = cat_fixtures("color", false);
    for opts in [&["-s"][..], &["-b"], &["-sn"], &["-sb"], &["-n"]] {
        let out = hl_output("always", opts, &files);
        assert!(
            out.windows(ESC.len()).any(|w| w == ESC.as_bytes()),
            "hl --color always {opts:?} should be highlighted"
        );
        assert_eq!(
            strip_ansi(&out),
            cat_output(opts, &files),
            "hl --color always {opts:?} differs from cat"
        );
    }
}

#[test]
fn numbering_continues_a_line_across_files() {
    let dir = tempdir();
    let a = dir.join("partial-a.txt");
    let b = dir.join("partial-b.txt");
    fs::write(&a, "a").unwrap();
    fs::write(&b, "\n\nb\n").unwrap();
    hl().arg("-n")
        .args([&a, &b])
        .assert()
        .success()
        .stdout("     1\ta\n     2\t\n     3\tb\n");
}

#[test]
fn theme_has_no_short_option() {
    hl().args(["--color", "always", "--theme", "Nord", "Cargo.toml"])
        .assert()
        .success();
    // `-t` is cat's -vT now, so "Nord" is read as a file name.
    hl().args(["-t", "Nord"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("hl: Nord: No such file or directory"));
}
