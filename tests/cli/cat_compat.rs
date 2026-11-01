//! The options of `cat`: notation, numbering and squeezing.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::common::{colored, files, has_escape, hl, stdout, strip_ansi, tempdir};

#[test]
fn numbers_lines_across_files() {
    let dir = tempdir();
    let files = files(dir.path(), &[("a.txt", "one\ntwo\n"), ("b.txt", "three\n")]);
    hl().arg("-n")
        .args(&files)
        .assert()
        .success()
        .stdout("     1\tone\n     2\ttwo\n     3\tthree\n");
}

/// Writes two fixture files exercising the cases `cat` options care about:
/// a CRLF ending, tabs, runs of blank lines, a line of spaces (not blank),
/// and a first file without a final newline, whose last line the second
/// file finishes. With `binary`, the first one also has control characters
/// and high bytes.
fn cat_fixtures(dir: &Path, prefix: &str, binary: bool) -> Vec<PathBuf> {
    let mut first = b"fn main() {\r\n\tlet x = 1;\n\n\n\n  \n".to_vec();
    if binary {
        first.extend_from_slice(b"\x01\x1b\x7f\x80\x89\xa0\xff\n");
    }
    first.extend_from_slice(b"}\n\n\nno newline");
    files(
        dir,
        &[
            (format!("{prefix}-1.rs").as_str(), first),
            (
                format!("{prefix}-2.txt").as_str(),
                b"\n\nnext\tline\n\n\n".to_vec(),
            ),
        ],
    )
}

/// The uncolored output of `hl` with `opts` on `files`.
fn plain(opts: &[&str], files: &[PathBuf]) -> Vec<u8> {
    stdout(hl().args(opts).args(files))
}
/// What the notation options print for `cat_fixtures(.., true)`, as GNU
/// coreutils 9.x `cat` does. One output line per source line.
const NOTATION_CASES: &[(&str, &[u8])] = &[
    (
        "-A",
        b"fn main() {^M$\n\
          ^Ilet x = 1;$\n\
          $\n\
          $\n\
          $\n  \
          $\n\
          ^A^[^?M-^@M-^IM- M-^?$\n\
          }$\n\
          $\n\
          $\n\
          no newline$\n\
          $\n\
          next^Iline$\n\
          $\n\
          $\n",
    ),
    (
        "-v",
        b"fn main() {^M\n\
          \tlet x = 1;\n\
          \n\
          \n\
          \n  \
          \n\
          ^A^[^?M-^@M-^IM- M-^?\n\
          }\n\
          \n\
          \n\
          no newline\n\
          \n\
          next\tline\n\
          \n\
          \n",
    ),
    (
        // A CRLF ending shows as `^M$` even without -v.
        "-E",
        b"fn main() {^M$\n\
          \tlet x = 1;$\n\
          $\n\
          $\n\
          $\n  \
          $\n\
          \x01\x1b\x7f\x80\x89\xa0\xff$\n\
          }$\n\
          $\n\
          $\n\
          no newline$\n\
          $\n\
          next\tline$\n\
          $\n\
          $\n",
    ),
    (
        "-T",
        b"fn main() {\r\n\
          ^Ilet x = 1;\n\
          \n\
          \n\
          \n  \
          \n\
          \x01\x1b\x7f\x80\x89\xa0\xff\n\
          }\n\
          \n\
          \n\
          no newline\n\
          \n\
          next^Iline\n\
          \n\
          \n",
    ),
];

/// These options show the input in cat's notation, which the highlighting
/// would break, so colored output is the same.
#[test]
fn shows_the_cat_notation() {
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "notation", true);
    for &(opt, expected) in NOTATION_CASES {
        assert_eq!(plain(&[opt], &files), expected, "hl {opt}");
        assert_eq!(
            colored(hl().arg(opt).args(&files)),
            expected,
            "hl --color always {opt}"
        );
    }
}

#[test]
fn combines_the_notation_options() {
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "combined", true);
    for (short, long) in [
        (&["-e"][..], &["-v", "-E"][..]),
        (&["-t"], &["-v", "-T"]),
        (&["-A"], &["-v", "-E", "-T"]),
        (&["--show-all"], &["-A"]),
    ] {
        assert_eq!(
            plain(short, &files),
            plain(long, &files),
            "hl {short:?} should be hl {long:?}"
        );
    }
}

/// What the numbering and squeezing options print for
/// `cat_fixtures(.., false)`. A line of spaces is not blank, and the last
/// line of the first file is numbered once, as the second file ends it.
const NUMBERING_CASES: &[(&[&str], &[u8])] = &[
    (
        &["-s"],
        b"fn main() {\r\n\
          \tlet x = 1;\n\
          \n  \
          \n\
          }\n\
          \n\
          no newline\n\
          \n\
          next\tline\n\
          \n",
    ),
    (
        &["-b"],
        b"     1\tfn main() {\r\n\
          \x20    2\t\tlet x = 1;\n\
          \n\
          \n\
          \n\
          \x20    3\t  \n\
          \x20    4\t}\n\
          \n\
          \n\
          \x20    5\tno newline\n\
          \n\
          \x20    6\tnext\tline\n\
          \n\
          \n",
    ),
    (
        &["-n"],
        b"     1\tfn main() {\r\n\
          \x20    2\t\tlet x = 1;\n\
          \x20    3\t\n\
          \x20    4\t\n\
          \x20    5\t\n\
          \x20    6\t  \n\
          \x20    7\t}\n\
          \x20    8\t\n\
          \x20    9\t\n\
          \x20   10\tno newline\n\
          \x20   11\t\n\
          \x20   12\tnext\tline\n\
          \x20   13\t\n\
          \x20   14\t\n",
    ),
    (
        &["-sn"],
        b"     1\tfn main() {\r\n\
          \x20    2\t\tlet x = 1;\n\
          \x20    3\t\n\
          \x20    4\t  \n\
          \x20    5\t}\n\
          \x20    6\t\n\
          \x20    7\tno newline\n\
          \x20    8\t\n\
          \x20    9\tnext\tline\n\
          \x20   10\t\n",
    ),
    (
        &["-sb"],
        b"     1\tfn main() {\r\n\
          \x20    2\t\tlet x = 1;\n\
          \n\
          \x20    3\t  \n\
          \x20    4\t}\n\
          \n\
          \x20    5\tno newline\n\
          \n\
          \x20    6\tnext\tline\n\
          \n",
    ),
];

#[test]
fn numbers_and_squeezes_lines_like_cat() {
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "numbering", false);
    for &(opts, expected) in NUMBERING_CASES {
        assert_eq!(plain(opts, &files), expected, "hl {opts:?}");
        let colored = colored(hl().args(opts).args(&files));
        assert!(
            has_escape(&colored),
            "hl --color always {opts:?} should be highlighted"
        );
        assert_eq!(strip_ansi(&colored), expected, "hl --color always {opts:?}");
    }
}

#[test]
fn ignores_the_options_cat_ignores() {
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "ignored", true);
    assert_eq!(plain(&["-u"], &files), plain(&[], &files));
    // -b wins over -n in either order.
    assert_eq!(plain(&["-bn"], &files), plain(&["-b"], &files));
    assert_eq!(plain(&["-nb"], &files), plain(&["-b"], &files));
}

/// The GNU coreutils `cat` to compare `hl` with, if there is one. Other
/// implementations differ: BSD `cat` (macOS) has no `-A`, and uutils `cat`
/// (Ubuntu 25.10 and later) squeezes blank lines across files differently.
/// Before 9.0, `-E` did not show a CRLF ending as `^M$`. `gcat` is the name
/// Homebrew's coreutils uses.
fn gnu_cat() -> Option<&'static str> {
    static CAT: OnceLock<Option<&str>> = OnceLock::new();
    *CAT.get_or_init(|| {
        ["gcat", "cat"].into_iter().find(|cat| {
            std::process::Command::new(cat)
                .arg("--version")
                .output()
                .is_ok_and(|out| is_gnu_cat_9(&String::from_utf8_lossy(&out.stdout)))
        })
    })
}

/// Whether `version`, the output of `cat --version`, is GNU coreutils 9.0
/// or later. Its first line is like "cat (GNU coreutils) 9.4".
fn is_gnu_cat_9(version: &str) -> bool {
    let first = version.lines().next().unwrap_or_default();
    first.contains("(GNU coreutils)")
        && first
            .rsplit(' ')
            .next()
            .and_then(|v| v.split('.').next())
            .and_then(|major| major.parse::<u32>().ok())
            .is_some_and(|major| major >= 9)
}

#[test]
fn recognizes_gnu_cat_9_or_later() {
    assert!(is_gnu_cat_9("cat (GNU coreutils) 9.4\nCopyright"));
    assert!(is_gnu_cat_9("cat (GNU coreutils) 10.0\n"));
    assert!(!is_gnu_cat_9("cat (GNU coreutils) 8.32\n"));
    assert!(!is_gnu_cat_9("cat (uutils coreutils) 0.2.2\n"));
    assert!(!is_gnu_cat_9(""));
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

/// Compares `hl` with GNU `cat` itself, for the combinations the tests above
/// don't spell out. Skipped when there is no GNU `cat` 9.0 or later.
#[test]
fn matches_gnu_cat() {
    use std::io::Write;

    let Some(cat) = gnu_cat() else {
        // Written to stderr directly, as `eprintln!` would be captured.
        let _ = writeln!(
            std::io::stderr(),
            "skipping matches_gnu_cat: GNU coreutils cat 9.0 or later not found (as `gcat` or `cat`)"
        );
        return;
    };
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "gnu", true);
    for opts in CAT_OPTIONS {
        let out = std::process::Command::new(cat)
            .args(*opts)
            .args(&files)
            .output()
            .unwrap();
        assert!(out.status.success(), "{cat} {opts:?} failed");
        assert_eq!(
            plain(opts, &files),
            out.stdout,
            "hl {opts:?} differs from {cat}"
        );
    }
}

#[test]
fn numbering_continues_a_line_across_files() {
    let dir = tempdir();
    let files = files(
        dir.path(),
        &[("partial-a.txt", "a"), ("partial-b.txt", "\n\nb\n")],
    );
    hl().arg("-n")
        .args(&files)
        .assert()
        .success()
        .stdout("     1\ta\n     2\t\n     3\tb\n");
}
