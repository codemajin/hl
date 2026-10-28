use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

const ESC: &str = "\x1b[";

fn hl() -> Command {
    let mut cmd = Command::cargo_bin("hl").unwrap();
    cmd.env_remove("HL_THEME")
        .env_remove("HL_MAX_SIZE")
        .env_remove("HL_MAX_LINE_LENGTH");
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
    hl().arg("-")
        .write_stdin("abc\n")
        .assert()
        .success()
        .stdout("abc\n");
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

fn colored_stdout(cmd: &mut Command) -> Vec<u8> {
    cmd.assert().success().get_output().stdout.clone()
}

#[test]
fn falls_back_on_unknown_theme() {
    let expected = colored_stdout(hl().args(["--color", "always", "Cargo.toml"]));
    hl().args(["--color", "always", "--theme", "NoSuchTheme", "Cargo.toml"])
        .assert()
        .success()
        .stderr("hl: unknown theme 'NoSuchTheme', falling back to base16-ocean.dark (see --list-themes)\n")
        .stdout(expected);
}

#[test]
fn falls_back_on_unknown_theme_from_env() {
    let expected = colored_stdout(hl().args(["--color", "always", "Cargo.toml"]));
    hl().env("HL_THEME", "NoSuchTheme")
        .args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stderr(predicate::str::contains("unknown theme 'NoSuchTheme'"))
        .stdout(expected);
}

#[test]
fn matches_theme_case_insensitively() {
    let expected = colored_stdout(hl().args(["--color", "always", "--theme", "Nord", "Cargo.toml"]));
    hl().args(["--color", "always", "--theme", "nord", "Cargo.toml"])
        .assert()
        .success()
        .stderr("")
        .stdout(expected);
}

#[test]
fn ignores_unknown_theme_without_color() {
    let content = fs::read("src/main.rs").unwrap();
    hl().args(["--theme", "NoSuchTheme", "src/main.rs"])
        .assert()
        .success()
        .stdout(content);
}

fn has_escape(bytes: &[u8]) -> bool {
    bytes.windows(ESC.len()).any(|w| w == ESC.as_bytes())
}

/// A JS file with a short line, a `long`-byte line and another short line.
/// Returns the path, the head that should stay highlighted and the tail
/// (from the long line on) that should be printed as is.
fn long_line_fixture(name: &str, long: usize) -> (std::path::PathBuf, Vec<u8>, Vec<u8>) {
    let head = b"let a = 1;\n".to_vec();
    let mut tail = "x".repeat(long).into_bytes();
    tail.extend_from_slice(b"\nlet b = 2;\n");
    let path = tempdir().join(name);
    fs::write(&path, [head.as_slice(), &tail].concat()).unwrap();
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
    let path = tempdir().join("large.rs");
    let content = "fn main() {}\n".repeat(100).into_bytes();
    fs::write(&path, &content).unwrap();
    hl().args(["--color", "always", "--max-size", "1K"])
        .arg(&path)
        .assert()
        .success()
        .stdout(content);
}

#[test]
fn stops_highlighting_large_stdin_at_the_limit() {
    let line = "fn main() {}\n";
    let input = line.repeat(100);
    let out = colored_stdout(
        hl().args(["--color", "always", "-l", "rust", "--max-size", "1K"])
            .write_stdin(input.clone()),
    );
    // The line that crosses 1024 bytes and everything after it are plain.
    let highlighted_lines = 1024 / line.len();
    let tail = &input.as_bytes()[highlighted_lines * line.len()..];
    assert!(out.ends_with(tail));
    let colored = &out[..out.len() - tail.len()];
    assert!(has_escape(colored));
    assert_eq!(strip_ansi(colored), line.repeat(highlighted_lines).as_bytes());
}

#[test]
fn stops_highlighting_at_a_long_line() {
    let (path, head, tail) = long_line_fixture("long.js", 100);
    let out = colored_stdout(
        hl().args(["--color", "always", "--max-line-length", "64"])
            .arg(&path),
    );
    assert_plain_from_long_line(&out, &head, &tail);
}

#[test]
fn highlights_a_line_exactly_at_the_limit() {
    let (path, _, _) = long_line_fixture("exact.js", 64);
    let out = colored_stdout(
        hl().args(["--color", "always", "--max-line-length", "64"])
            .arg(&path),
    );
    assert!(out.ends_with(b"\x1b[0m\n"), "the last line should be highlighted");
}

#[test]
fn limits_line_length_by_default() {
    let (path, head, tail) = long_line_fixture("default.js", 20 * 1024);
    let out = colored_stdout(hl().args(["--color", "always"]).arg(&path));
    assert_plain_from_long_line(&out, &head, &tail);
}

#[test]
fn zero_disables_the_limits() {
    let (path, _, _) = long_line_fixture("unlimited.js", 100);
    let out = colored_stdout(
        hl().args(["--color", "always", "--max-size", "0", "--max-line-length", "0"])
            .arg(&path),
    );
    assert!(out.ends_with(b"\x1b[0m\n"), "the last line should be highlighted");
}

#[test]
fn reads_limits_from_env() {
    let (path, head, tail) = long_line_fixture("env.js", 100);
    let out = colored_stdout(
        hl().env("HL_MAX_LINE_LENGTH", "64")
            .args(["--color", "always"])
            .arg(&path),
    );
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
    let path = tempdir().join("switch.js");
    let mut content = b"let a = 1;\n\n\n".to_vec();
    content.extend_from_slice("x".repeat(100).as_bytes());
    content.extend_from_slice(b"\n\n\nlet b = 2;\nno newline");
    fs::write(&path, content).unwrap();
    let files = [path];
    for opts in [&["-n"][..], &["-b"], &["-s"], &["-sn"]] {
        let out = hl_output(
            "always",
            &[&["--max-line-length", "64"][..], opts].concat(),
            &files,
        );
        assert!(has_escape(&out), "hl {opts:?} should highlight the first line");
        assert_eq!(
            strip_ansi(&out),
            cat_output(opts, &files),
            "hl {opts:?} differs from cat"
        );
    }
}

#[test]
fn parses_size_units() {
    for size in ["1024", "1K", "2m", "1G"] {
        hl().args(["--max-size", size, "Cargo.toml"]).assert().success();
    }
    for size in ["abc", "1T", "-1", "", "99999999999G"] {
        hl().arg(format!("--max-size={size}"))
            .arg("Cargo.toml")
            .assert()
            .code(2)
            .stderr(predicate::str::contains("--max-size"));
    }
}

/// "// 日本語" in Shift_JIS and EUC-JP, which are not valid UTF-8.
const SJIS_COMMENT: &[u8] = b"// \x93\xfa\x96\x7b\x8c\xea\n";
const EUC_JP_COMMENT: &[u8] = b"// \xc6\xfc\xcb\xdc\xb8\xec\n";

#[test]
fn prints_non_utf8_files_unchanged() {
    for (name, comment) in [("sjis.rs", SJIS_COMMENT), ("eucjp.rs", EUC_JP_COMMENT)] {
        let path = tempdir().join(name);
        let content = [comment, b"fn main() {}\n"].concat();
        fs::write(&path, &content).unwrap();
        hl().args(["--color", "always"])
            .arg(&path)
            .assert()
            .success()
            .stdout(content);
    }
}

#[test]
fn stops_highlighting_at_a_non_utf8_line() {
    let path = tempdir().join("late-sjis.rs");
    // Past the first 8 KB, so only the line-by-line check can catch it.
    let mut content = "fn main() {}\n\n\n".repeat(1000).into_bytes();
    content.extend_from_slice(SJIS_COMMENT);
    content.extend_from_slice(b"\n\nfn after() {}\n");
    fs::write(&path, &content).unwrap();
    let files = [path];
    for opts in [&[][..], &["-n"], &["-sb"]] {
        let out = hl_output("always", opts, &files);
        assert!(has_escape(&out), "hl {opts:?} should highlight the first lines");
        assert!(
            out.ends_with(b"fn after() {}\n"),
            "hl {opts:?} should not color the rest"
        );
        assert_eq!(
            strip_ansi(&out),
            cat_output(opts, &files),
            "hl {opts:?} differs from cat"
        );
    }
}

fn utf16_with_bom(text: &str, big_endian: bool) -> Vec<u8> {
    std::iter::once(0xFEFF)
        .chain(text.encode_utf16())
        .flat_map(|u| {
            if big_endian {
                u.to_be_bytes()
            } else {
                u.to_le_bytes()
            }
        })
        .collect()
}

#[test]
fn highlights_utf16_as_utf8() {
    let text = "// 日本語 😀\nfn main() {}\n";
    for big_endian in [false, true] {
        let dir = tempdir();
        let utf16 = [dir.join(format!("utf16-{big_endian}.rs"))];
        fs::write(&utf16[0], utf16_with_bom(text, big_endian)).unwrap();
        let utf8 = [dir.join(format!("utf8-{big_endian}.rs"))];
        fs::write(&utf8[0], text).unwrap();
        assert_eq!(hl_output("always", &[], &utf16), hl_output("always", &[], &utf8));
        let numbered = hl_output("always", &["-n"], &utf16);
        assert_eq!(strip_ansi(&numbered), cat_output(&["-n"], &utf8));
    }
}

#[test]
fn passes_utf16_through_without_color() {
    let path = tempdir().join("utf16-plain.rs");
    let content = utf16_with_bom("fn main() {}\n", false);
    fs::write(&path, &content).unwrap();
    hl().arg(&path).assert().success().stdout(content);
}

#[test]
fn replaces_broken_utf16_with_replacement_characters() {
    let path = [tempdir().join("broken-utf16.txt")];
    // "a", a lone high surrogate, "b", then an odd trailing byte.
    fs::write(&path[0], b"\xff\xfea\x00\x00\xd8b\x00\n\x00x").unwrap();
    let out = hl_output("always", &[], &path);
    assert_eq!(strip_ansi(&out), "a\u{FFFD}b\n\u{FFFD}".as_bytes());
}

#[test]
fn keeps_utf8_bom_and_detects_the_shebang_after_it() {
    let dir = tempdir();
    let script = "#!/bin/sh\necho hi\n";
    let plain = [dir.join("script")];
    fs::write(&plain[0], script).unwrap();
    let with_bom = [dir.join("script-bom")];
    fs::write(&with_bom[0], [b"\xEF\xBB\xBF", script.as_bytes()].concat()).unwrap();
    let expected = hl_output("always", &[], &plain);
    assert_eq!(
        expected,
        hl_output("always", &["-l", "sh"], &plain),
        "the shebang should select sh"
    );
    let out = hl_output("always", &[], &with_bom);
    assert_eq!(out, [b"\xEF\xBB\xBF", expected.as_slice()].concat());
}
