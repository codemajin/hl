use std::{fs, path::Path};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

const ESC: &str = "\x1b[";

fn hl() -> Command {
    let mut cmd = Command::cargo_bin("hl").unwrap();
    cmd.env_remove("HL_THEME")
        .env_remove("HL_MAX_SIZE")
        .env_remove("HL_MAX_LINE_LENGTH")
        .env_remove("HL_COLOR_DEPTH")
        .env_remove("COLORTERM")
        .env_remove("WT_SESSION")
        .env("TERM", "xterm-256color");
    cmd
}

/// A directory for test files, removed when the returned value is dropped.
fn tempdir() -> TempDir {
    tempfile::Builder::new()
        .prefix("hl-test-")
        .tempdir()
        .unwrap()
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
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
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
    let ok = dir.path().join("ok.txt");
    fs::write(&ok, "hello\n").unwrap();
    hl().arg(dir.path().join("missing.txt"))
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
    assert_ne!(
        plain, rust,
        "rust highlighting should differ from plain text"
    );
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
    let bin = dir.path().join("data.bin");
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
fn cat_fixtures(dir: &Path, prefix: &str, binary: bool) -> Vec<std::path::PathBuf> {
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
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "plain", true);
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
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "visual", true);
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
    let dir = tempdir();
    let files = cat_fixtures(dir.path(), "color", false);
    for opts in [&["-s"][..], &["-b"], &["-sn"], &["-sb"], &["-n"]] {
        let out = hl_output("always", opts, &files);
        assert!(
            has_escape(&out),
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
    let a = dir.path().join("partial-a.txt");
    let b = dir.path().join("partial-b.txt");
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
        .stderr(predicate::str::contains(
            "hl: Nord: No such file or directory",
        ));
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
    let expected =
        colored_stdout(hl().args(["--color", "always", "--theme", "Nord", "Cargo.toml"]));
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
        .stderr("")
        .stdout(content);
}

#[test]
fn ignores_unknown_theme_with_language_but_without_color() {
    let content = fs::read("src/main.rs").unwrap();
    hl().args([
        "--theme",
        "NoSuchTheme",
        "-l",
        "rust",
        "--color",
        "never",
        "src/main.rs",
    ])
    .assert()
    .success()
    .stderr("")
    .stdout(content);
}

#[test]
fn lists_languages_without_loading_the_theme() {
    hl().env("HL_THEME", "NoSuchTheme")
        .arg("--list-languages")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::contains("Rust: rs"));
}

fn has_escape(bytes: &[u8]) -> bool {
    bytes.windows(ESC.len()).any(|w| w == ESC.as_bytes())
}

/// A JS file with a short line, a `long`-byte line and another short line.
/// Returns the path, the head that should stay highlighted and the tail
/// (from the long line on) that should be printed as is.
fn long_line_fixture(
    dir: &Path,
    name: &str,
    long: usize,
) -> (std::path::PathBuf, Vec<u8>, Vec<u8>) {
    let head = b"let a = 1;\n".to_vec();
    let mut tail = "x".repeat(long).into_bytes();
    tail.extend_from_slice(b"\nlet b = 2;\n");
    let path = dir.join(name);
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
    let dir = tempdir();
    let path = dir.path().join("large.rs");
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
    assert_eq!(
        strip_ansi(colored),
        line.repeat(highlighted_lines).as_bytes()
    );
}

#[test]
fn stops_highlighting_at_a_long_line() {
    let dir = tempdir();
    let (path, head, tail) = long_line_fixture(dir.path(), "long.js", 100);
    let out = colored_stdout(
        hl().args(["--color", "always", "--max-line-length", "64"])
            .arg(&path),
    );
    assert_plain_from_long_line(&out, &head, &tail);
}

#[test]
fn highlights_a_line_exactly_at_the_limit() {
    let dir = tempdir();
    let (path, _, _) = long_line_fixture(dir.path(), "exact.js", 64);
    let out = colored_stdout(
        hl().args(["--color", "always", "--max-line-length", "64"])
            .arg(&path),
    );
    assert!(
        out.ends_with(b"\x1b[0m\n"),
        "the last line should be highlighted"
    );
}

#[test]
fn limits_line_length_by_default() {
    let dir = tempdir();
    let (path, head, tail) = long_line_fixture(dir.path(), "default.js", 20 * 1024);
    let out = colored_stdout(hl().args(["--color", "always"]).arg(&path));
    assert_plain_from_long_line(&out, &head, &tail);
}

#[test]
fn zero_disables_the_limits() {
    let dir = tempdir();
    let (path, _, _) = long_line_fixture(dir.path(), "unlimited.js", 100);
    let out = colored_stdout(
        hl().args([
            "--color",
            "always",
            "--max-size",
            "0",
            "--max-line-length",
            "0",
        ])
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
    let dir = tempdir();
    let path = dir.path().join("switch.js");
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
        assert!(
            has_escape(&out),
            "hl {opts:?} should highlight the first line"
        );
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
        hl().args(["--max-size", size, "Cargo.toml"])
            .assert()
            .success();
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
        let dir = tempdir();
        let path = dir.path().join(name);
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
    let dir = tempdir();
    let path = dir.path().join("late-sjis.rs");
    // Past the first 8 KB, so only the line-by-line check can catch it.
    let mut content = "fn main() {}\n\n\n".repeat(1000).into_bytes();
    content.extend_from_slice(SJIS_COMMENT);
    content.extend_from_slice(b"\n\nfn after() {}\n");
    fs::write(&path, &content).unwrap();
    let files = [path];
    for opts in [&[][..], &["-n"], &["-sb"]] {
        let out = hl_output("always", opts, &files);
        assert!(
            has_escape(&out),
            "hl {opts:?} should highlight the first lines"
        );
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
        let utf16 = [dir.path().join(format!("utf16-{big_endian}.rs"))];
        fs::write(&utf16[0], utf16_with_bom(text, big_endian)).unwrap();
        let utf8 = [dir.path().join(format!("utf8-{big_endian}.rs"))];
        fs::write(&utf8[0], text).unwrap();
        assert_eq!(
            hl_output("always", &[], &utf16),
            hl_output("always", &[], &utf8)
        );
        let numbered = hl_output("always", &["-n"], &utf16);
        assert_eq!(strip_ansi(&numbered), cat_output(&["-n"], &utf8));
    }
}

#[test]
fn passes_utf16_through_without_color() {
    let dir = tempdir();
    let path = dir.path().join("utf16-plain.rs");
    let content = utf16_with_bom("fn main() {}\n", false);
    fs::write(&path, &content).unwrap();
    hl().arg(&path).assert().success().stdout(content);
}

#[test]
fn replaces_broken_utf16_with_replacement_characters() {
    let dir = tempdir();
    let path = [dir.path().join("broken-utf16.txt")];
    // "a", a lone high surrogate, "b", then an odd trailing byte.
    fs::write(&path[0], b"\xff\xfea\x00\x00\xd8b\x00\n\x00x").unwrap();
    let out = hl_output("always", &[], &path);
    assert_eq!(strip_ansi(&out), "a\u{FFFD}b\n\u{FFFD}".as_bytes());
}

#[test]
fn keeps_utf8_bom_and_detects_the_shebang_after_it() {
    let dir = tempdir();
    let script = "#!/bin/sh\necho hi\n";
    let plain = [dir.path().join("script")];
    fs::write(&plain[0], script).unwrap();
    let with_bom = [dir.path().join("script-bom")];
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

const TRUECOLOR: &str = "\x1b[38;2;";
const ANSI256: &str = "\x1b[38;5;";

/// Asserts that `cmd` colors Cargo.toml with `expected` escapes only.
fn assert_depth(cmd: &mut Command, expected: &str) {
    let other = if expected == TRUECOLOR {
        ANSI256
    } else {
        TRUECOLOR
    };
    cmd.args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains(expected).and(predicate::str::contains(other).not()));
}

#[test]
fn uses_truecolor_when_colorterm_advertises_it() {
    assert_depth(hl().env("COLORTERM", "truecolor"), TRUECOLOR);
    assert_depth(hl().env("COLORTERM", "24bit"), TRUECOLOR);
}

#[test]
fn uses_truecolor_for_direct_term_and_windows_terminal() {
    assert_depth(hl().env("TERM", "xterm-direct"), TRUECOLOR);
    assert_depth(hl().env("WT_SESSION", "1"), TRUECOLOR);
}

#[test]
fn falls_back_to_256_colors() {
    assert_depth(&mut hl(), ANSI256);
    assert_depth(hl().env_remove("TERM"), ANSI256);
    assert_depth(hl().env("WT_SESSION", ""), ANSI256);
}

#[test]
fn color_depth_option_overrides_detection() {
    assert_depth(
        hl().env("COLORTERM", "truecolor")
            .args(["--color-depth", "256"]),
        ANSI256,
    );
    assert_depth(hl().args(["--color-depth", "truecolor"]), TRUECOLOR);
    assert_depth(hl().args(["--color-depth", "24bit"]), TRUECOLOR);
    assert_depth(
        hl().env("COLORTERM", "truecolor")
            .args(["--color-depth", "auto"]),
        TRUECOLOR,
    );
}

#[test]
fn color_depth_can_be_set_from_env() {
    assert_depth(
        hl().env("COLORTERM", "truecolor")
            .env("HL_COLOR_DEPTH", "256"),
        ANSI256,
    );
    assert_depth(
        hl().env("HL_COLOR_DEPTH", "256")
            .args(["--color-depth", "truecolor"]),
        TRUECOLOR,
    );
}

#[test]
fn rejects_unknown_color_depth() {
    hl().args(["--color-depth", "16", "Cargo.toml"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn keeps_the_text_with_256_colors() {
    let content = fs::read("src/main.rs").unwrap();
    let out =
        colored_stdout(hl().args(["--color", "always", "--color-depth", "256", "src/main.rs"]));
    assert!(has_escape(&out));
    assert_eq!(strip_ansi(&out), content);
}

#[cfg(target_os = "linux")]
#[test]
fn reports_write_errors_without_blaming_the_input() {
    let full = fs::File::create("/dev/full").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hl"))
        .args(["src/main.rs", "Cargo.toml"])
        .stdout(full)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "hl: write error: No space left on device\n"
    );
}

#[test]
fn stops_quietly_when_the_listing_pipe_closes() {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_hl"))
        .arg("--list-languages")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn never_starts_a_pager_without_a_terminal() {
    let dir = tempdir();
    let marker = dir.path().join("paged");
    let pager = format!("touch {}", marker.display());
    let content = fs::read("Cargo.toml").unwrap();
    for opts in [&["-p"][..], &["--paging=always"], &["--paging=auto"]] {
        hl().args(opts)
            .arg("Cargo.toml")
            .env("PAGER", &pager)
            .assert()
            .success()
            .stdout(content.clone());
        assert!(!marker.exists(), "pager started with {opts:?}");
    }
}

#[test]
fn accepts_p_together_with_paging() {
    let content = fs::read("Cargo.toml").unwrap();
    for opts in [["-p", "--paging=never"], ["--paging=never", "-p"]] {
        hl().args(opts)
            .arg("Cargo.toml")
            .assert()
            .success()
            .stdout(content.clone());
    }
}

#[test]
fn rejects_unknown_paging_mode() {
    hl().args(["--paging=sometimes", "Cargo.toml"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value 'sometimes'"));
}

/// Writes `files` (name, content) into `dir` and returns their paths.
fn write_files(dir: &Path, files: &[(&str, &str)]) -> Vec<std::path::PathBuf> {
    files
        .iter()
        .map(|(name, content)| {
            let path = dir.join(name);
            fs::write(&path, content).unwrap();
            path
        })
        .collect()
}

#[test]
fn prints_a_header_before_each_file() {
    let dir = tempdir();
    let files = write_files(dir.path(), &[("a.txt", "one\ntwo\n"), ("b.txt", "three\n")]);
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
    let files = write_files(dir.path(), &[("a.txt", "one\n")]);
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
    let files = write_files(dir.path(), &[("a.txt", "one\n\n"), ("b.txt", "two\n")]);
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
    let files = write_files(dir.path(), &[("a.txt", "one"), ("b.txt", "two\n")]);
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

#[test]
fn does_not_squeeze_across_a_header() {
    let dir = tempdir();
    let files = write_files(dir.path(), &[("a.txt", "one\n\n\n"), ("b.txt", "\ntwo\n")]);
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
    let files = write_files(dir.path(), &[("ok.txt", "hello\n")]);
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

fn lines_fixture(dir: &Path) -> std::path::PathBuf {
    write_files(dir, &[("lines.txt", "1\n2\n3\n4\n5\n")]).remove(0)
}

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

#[test]
fn rejects_invalid_line_ranges() {
    for (range, message) in [
        ("0", "line numbers start at 1"),
        ("2:0", "line numbers start at 1"),
        ("5:3", "'5:3' ends before it starts"),
        ("a:b", "'a:b' is not a line range"),
        (":", "':' is not a line range"),
        ("1-3", "'1-3' is not a line range"),
    ] {
        hl().args(["--line-range", range, "Cargo.toml"])
            .assert()
            .failure()
            .stderr(predicate::str::contains(message));
    }
}

#[test]
fn numbers_a_line_range_with_the_input_line_numbers() {
    let dir = tempdir();
    let path = write_files(dir.path(), &[("blank.txt", "1\n\n3\n\n\n6\n")]).remove(0);
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
    let files = write_files(
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
    let files = write_files(dir.path(), &[("a.txt", "a1\na2"), ("b.txt", "b1\nb2\n")]);
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
    let path = write_files(
        dir.path(),
        &[("comment.rs", "/*\nfn not_code() {}\n*/\nfn main() {}\n")],
    )
    .remove(0);
    let whole = colored_stdout(hl().args(["--color", "always"]).arg(&path));
    let whole_lines: Vec<&[u8]> = whole.split_inclusive(|&b| b == b'\n').collect();
    let range = colored_stdout(hl().args(["--color", "always", "-r", "2:3"]).arg(&path));
    assert_eq!(range, whole_lines[1..3].concat());
    // On its own, the same text would be highlighted as code.
    let alone = colored_stdout(
        hl().args(["--color", "always", "-l", "rust"])
            .write_stdin("fn not_code() {}\n*/\n"),
    );
    assert_ne!(range, alone);
}

#[test]
fn applies_a_line_range_without_highlighting() {
    let dir = tempdir();
    let files = write_files(dir.path(), &[("tabs.rs", "fn a() {}\n\tb\nfn c() {}\n")]);
    hl().args(["--color", "always", "-T", "-r", "2"])
        .args(&files)
        .assert()
        .success()
        .stdout("^Ib\n");
    hl().args(["--color", "always", "--max-size", "1", "-r", "2:"])
        .args(&files)
        .assert()
        .success()
        .stdout("\tb\nfn c() {}\n");
    let sjis = dir.path().join("sjis.txt");
    fs::write(&sjis, b"a\n\x82\xa0\nc\n").unwrap();
    hl().args(["--color", "always", "-r", "2"])
        .arg(&sjis)
        .assert()
        .success()
        .stdout(&b"\x82\xa0\n"[..]);
}
