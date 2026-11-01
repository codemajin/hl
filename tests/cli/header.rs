//! `--header`: a header before each input.

use std::{fs, path::PathBuf};

use predicates::prelude::*;

use crate::common::{files, hl, stdout, strip_ansi, tempdir, utf16_with_bom};

#[test]
fn prints_a_header_before_each_file() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "one\ntwo\n"), ("b.txt", "three\n")]);
    hl().arg("--header")
        .args(&files)
        .assert()
        .success()
        .stdout(format!(
            "==> {} <==\none\ntwo\n\n==> {} <==\nthree\n",
            files[0].display(),
            files[1].display()
        ));
}

#[test]
fn prints_a_header_for_a_single_input_and_stdin() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "one\n")]);
    hl().args(["--header", "-"])
        .args(&files)
        .write_stdin("abc\n")
        .assert()
        .success()
        .stdout(format!(
            "==> standard input <==\nabc\n\n==> {} <==\none\n",
            files[0].display()
        ));
}

#[test]
fn numbers_lines_across_headers() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "one\n\n"), ("b.txt", "two\n")]);
    hl().args(["--header", "-n"])
        .args(&files)
        .assert()
        .success()
        .stdout(format!(
            "==> {} <==\n     1\tone\n     2\t\n\n==> {} <==\n     3\ttwo\n",
            files[0].display(),
            files[1].display()
        ));
}

#[test]
fn starts_a_header_on_a_new_line() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "one"), ("b.txt", "two\n")]);
    hl().args(["--header", "-n"])
        .args(&files)
        .assert()
        .success()
        .stdout(format!(
            "==> {} <==\n     1\tone\n\n==> {} <==\n     2\ttwo\n",
            files[0].display(),
            files[1].display()
        ));
}

/// Runs `hl --header` with `opts` and returns stdout without SGR sequences.
fn header_output(opts: &[&str], files: &[PathBuf]) -> String {
    let out = stdout(hl().arg("--header").args(opts).args(files));
    String::from_utf8(strip_ansi(&out)).unwrap()
}

#[test]
fn separates_files_the_same_way_in_every_mode() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "x"), ("b.txt", "y\n")]);
    let (a, b) = (files[0].display(), files[1].display());
    let expected = format!("==> {a} <==\nx\n\n==> {b} <==\ny\n");
    for opts in [
        &[][..],
        &["--strip-ansi", "always"],
        &["--color", "always"],
        &["--color", "always", "--strip-ansi", "never"],
    ] {
        assert_eq!(header_output(opts, &files), expected, "options {opts:?}");
    }
    assert_eq!(
        header_output(&["-n"], &files),
        format!("==> {a} <==\n     1\tx\n\n==> {b} <==\n     2\ty\n")
    );
}

#[test]
fn separates_unterminated_utf16_files() {
    let dir = tempdir();
    let path = files(dir.path(), &[("utf16.txt", utf16_with_bom("x", false))]).remove(0);
    let files = [path.clone(), path];
    let name = files[0].display();
    let expected = format!("==> {name} <==\nx\n\n==> {name} <==\nx");
    for opts in [&["--strip-ansi", "always"][..], &["--color", "always"]] {
        assert_eq!(header_output(opts, &files), expected, "options {opts:?}");
    }
}

#[test]
fn does_not_squeeze_across_a_header() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "one\n\n\n"), ("b.txt", "\ntwo\n")]);
    hl().args(["--header", "-s"])
        .args(&files)
        .assert()
        .success()
        .stdout(format!(
            "==> {} <==\none\n\n\n==> {} <==\n\ntwo\n",
            files[0].display(),
            files[1].display()
        ));
}

#[test]
fn prints_no_header_for_a_missing_file() {
    let dir = tempdir();
    let files = files(dir.path(), &[("ok.txt", "hello\n")]);
    hl().arg("--header")
        .arg(dir.path().join("missing.txt"))
        .args(&files)
        .assert()
        .failure()
        .stdout(format!("==> {} <==\nhello\n", files[0].display()))
        .stderr(predicate::str::contains("No such file or directory"));
}

#[test]
fn makes_the_header_bold_only_with_color() {
    hl().args(["--header", "--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\x1b[1m==> Cargo.toml <==\x1b[0m\n",
        ));
    let mut expected = b"==> Cargo.toml <==\n".to_vec();
    expected.extend(fs::read("Cargo.toml").unwrap());
    hl().args(["--header", "--color", "never", "Cargo.toml"])
        .assert()
        .success()
        .stdout(expected);
}
