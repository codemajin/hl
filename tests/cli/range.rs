//! `-r`/`--line-range`.

use predicates::prelude::*;

use crate::common::{colored, files, hl, lines_fixture, tempdir};

#[test]
fn prints_a_line_range() {
    let dir = tempdir();
    let path = lines_fixture(dir.path());
    for (range, expected) in [
        ("2:4", "2\n3\n4\n"),
        ("4:", "4\n5\n"),
        (":2", "1\n2\n"),
        ("3", "3\n"),
        ("3:3", "3\n"),
        ("4:10", "4\n5\n"),
        ("6:", ""),
    ] {
        hl().args(["-r", range])
            .arg(&path)
            .assert()
            .success()
            .stdout(expected);
    }
}

/// The ranges themselves are covered by the unit tests of `parse_range` in
/// `src/cli.rs`; this checks how clap reports a rejected one.
#[test]
fn rejects_an_invalid_line_range_with_status_2() {
    hl().args(["--line-range", "5:3", "Cargo.toml"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(
            "invalid value '5:3' for '--line-range <N:M>': '5:3' ends before it starts",
        ));
}

#[test]
fn numbers_a_line_range_with_the_input_line_numbers() {
    let dir = tempdir();
    let path = files(dir.path(), &[("blank.txt", "1\n\n3\n\n\n6\n")]).remove(0);
    for (opts, expected) in [
        (&["-n"][..], "     3\t3\n     4\t\n     5\t\n     6\t6\n"),
        (&["-b"], "     2\t3\n\n\n     3\t6\n"),
        (&["-s"], "3\n\n6\n"),
        (&["-sn"], "     3\t3\n     4\t\n     6\t6\n"),
    ] {
        hl().args(["-r", "3:"])
            .args(opts)
            .arg(&path)
            .assert()
            .success()
            .stdout(expected);
    }
}

#[test]
fn applies_a_line_range_to_each_input() {
    let dir = tempdir();
    let files = files(
        dir.path(),
        &[("a.txt", "a1\na2\na3\n"), ("b.txt", "b1\nb2\n")],
    );
    hl().args(["-n", "-r", "2:"])
        .args(&files)
        .assert()
        .success()
        .stdout("     2\ta2\n     3\ta3\n     2\tb2\n");
    hl().args(["--header", "-r", ":1"])
        .args(&files)
        .assert()
        .success()
        .stdout(format!(
            "==> {} <==\na1\n\n==> {} <==\nb1\n",
            files[0].display(),
            files[1].display()
        ));
}

#[test]
fn ends_an_unfinished_line_before_the_next_input_in_a_range() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "a1\na2"), ("b.txt", "b1\nb2\n")]);
    hl().args(["-r", "2"])
        .args(&files)
        .assert()
        .success()
        .stdout("a2\nb2\n");
    // A skipped unfinished line gets no newline.
    hl().args(["-r", "1"])
        .args(&files)
        .assert()
        .success()
        .stdout("a1\nb1\n");
}

#[test]
fn applies_a_line_range_to_stdin() {
    hl().args(["-r", "2:3"])
        .write_stdin("1\n2\n3\n4\n")
        .assert()
        .success()
        .stdout("2\n3\n");
}

#[test]
fn highlights_a_line_range_as_part_of_the_whole_input() {
    let dir = tempdir();
    let path = files(
        dir.path(),
        &[("comment.rs", "/*\nfn not_code() {}\n*/\nfn main() {}\n")],
    )
    .remove(0);
    let whole = colored(hl().arg(&path));
    let whole_lines: Vec<&[u8]> = whole.split_inclusive(|&b| b == b'\n').collect();
    let range = colored(hl().args(["-r", "2:3"]).arg(&path));
    assert_eq!(range, whole_lines[1..3].concat());
    // On its own, the same text would be highlighted as code.
    let alone = colored(
        hl().args(["-l", "rust"])
            .write_stdin("fn not_code() {}\n*/\n"),
    );
    assert_ne!(range, alone);
}

#[test]
fn applies_a_line_range_without_highlighting() {
    let dir = tempdir();
    let tabs = files(dir.path(), &[("tabs.rs", "fn a() {}\n\tb\nfn c() {}\n")]);
    hl().args(["--color", "always", "-T", "-r", "2"])
        .args(&tabs)
        .assert()
        .success()
        .stdout("^Ib\n");
    hl().args(["--color", "always", "--max-size", "1", "-r", "2:"])
        .args(&tabs)
        .assert()
        .success()
        .stdout("\tb\nfn c() {}\n");
    let sjis = files(dir.path(), &[("sjis.txt", b"a\n\x82\xa0\nc\n")]);
    hl().args(["--color", "always", "-r", "2"])
        .args(&sjis)
        .assert()
        .success()
        .stdout(&b"\x82\xa0\n"[..]);
}
