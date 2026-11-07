//! `-H`/`--highlight-line`.

use assert_cmd::Command;
use predicates::prelude::*;

use crate::common::{colored, files, fixture, hl, lines_fixture, stdout, strip_ansi, tempdir};

const BACKGROUND: &str = "\x1b[48;";

/// The output lines of `cmd` that set a background color, counted from 1.
fn highlighted_lines(cmd: &mut Command) -> Vec<usize> {
    let out = stdout(cmd);
    out.split_inclusive(|&b| b == b'\n')
        .enumerate()
        .filter(|(_, line)| has_background(line))
        .map(|(i, _)| i + 1)
        .collect()
}

fn has_background(bytes: &[u8]) -> bool {
    bytes
        .windows(BACKGROUND.len())
        .any(|w| w == BACKGROUND.as_bytes())
}

#[test]
fn highlights_the_given_lines() {
    let dir = tempdir();
    let path = lines_fixture(dir.path());
    let out = colored(hl().args(["-H", "2"]).arg(&path));
    assert_eq!(strip_ansi(&out), b"1\n2\n3\n4\n5\n");
    let lines: Vec<&[u8]> = out.split_inclusive(|&b| b == b'\n').collect();
    assert!(lines[1].starts_with(BACKGROUND.as_bytes()), "{out:?}");
    assert!(lines[1].ends_with(b"\x1b[0m\n"), "{out:?}");
    for (opts, expected) in [
        (&["-H", "2"][..], vec![2]),
        (&["-H", "2:3", "-H", "5"], vec![2, 3, 5]),
        (&["--highlight-line", "4:"], vec![4, 5]),
        (&["-H", ":2"], vec![1, 2]),
        (&["-H", "9"], vec![]),
    ] {
        let mut cmd = hl();
        cmd.args(["--color", "always"]).args(opts).arg(&path);
        assert_eq!(highlighted_lines(&mut cmd), expected, "{opts:?}");
    }
}

#[test]
fn highlights_with_the_color_depth() {
    let dir = tempdir();
    let path = lines_fixture(dir.path());
    for (depth, expected, other) in [
        ("truecolor", "\x1b[48;2;", "\x1b[48;5;"),
        ("256", "\x1b[48;5;", "\x1b[48;2;"),
    ] {
        hl().args(["--color", "always", "--color-depth", depth, "-H", "2"])
            .arg(&path)
            .assert()
            .success()
            .stdout(predicate::str::contains(expected).and(predicate::str::contains(other).not()));
    }
}

#[test]
fn ignores_highlighted_lines_without_color() {
    let dir = tempdir();
    let path = lines_fixture(dir.path());
    hl().args(["-H", "2"])
        .arg(&path)
        .assert()
        .success()
        .stdout("1\n2\n3\n4\n5\n");
    hl().args(["--color", "always", "-E", "-H", "2"])
        .arg(&path)
        .assert()
        .success()
        .stdout("1$\n2$\n3$\n4$\n5$\n");
}

#[test]
fn highlights_the_lines_of_each_input() {
    let dir = tempdir();
    let files = files(
        dir.path(),
        &[("a.txt", "a1\na2\na3\n"), ("b.txt", "b1\nb2\n")],
    );
    let mut cmd = hl();
    cmd.args(["--color", "always", "-n", "-H", "2"])
        .args(&files);
    let out = stdout(&mut cmd);
    assert_eq!(
        strip_ansi(&out),
        b"     1\ta1\n     2\ta2\n     3\ta3\n     4\tb1\n     5\tb2\n"
    );
    assert_eq!(highlighted_lines(&mut cmd), [2, 5]);
}

#[test]
fn highlights_lines_within_a_line_range() {
    let dir = tempdir();
    let path = lines_fixture(dir.path());
    let mut cmd = hl();
    cmd.args(["--color", "always", "-n", "-r", "2:4", "-H", "1", "-H", "3"])
        .arg(&path);
    let out = stdout(&mut cmd);
    assert_eq!(strip_ansi(&out), b"     2\t2\n     3\t3\n     4\t4\n");
    // The number is highlighted along with the line.
    let lines: Vec<&[u8]> = out.split_inclusive(|&b| b == b'\n').collect();
    assert!(lines[1].starts_with(BACKGROUND.as_bytes()), "{out:?}");
    assert_eq!(highlighted_lines(&mut cmd), [2]);
}

#[test]
fn highlights_lines_printed_without_syntax_highlighting() {
    let dir = tempdir();
    let path = lines_fixture(dir.path());
    let out = colored(hl().args(["--max-size", "1", "-H", "2"]).arg(&path));
    assert_eq!(strip_ansi(&out), b"1\n2\n3\n4\n5\n");
    let lines: Vec<&[u8]> = out.split_inclusive(|&b| b == b'\n').collect();
    assert_eq!(lines[0], b"1\n");
    assert!(lines[1].starts_with(BACKGROUND.as_bytes()), "{out:?}");
    assert!(lines[1].ends_with(b"2\x1b[0m\n"), "{out:?}");
    assert_eq!(lines[2..].concat(), b"3\n4\n5\n");
}

/// `-H` takes the same values as `-r`, which the unit tests of `parse_range`
/// in `src/cli.rs` cover; this checks that `-H` uses it too.
#[test]
fn rejects_an_invalid_highlighted_line_with_status_2() {
    hl().args(["--highlight-line", "+2"])
        .arg(fixture("sample.toml"))
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(
            "invalid value '+2' for '--highlight-line <N:M>': '+2' is not a line range",
        ));
}
