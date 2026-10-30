use std::{fs, path::Path, sync::OnceLock};

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
        .env("TERM", "xterm-256color")
        // Keeps the user's own config file, syntaxes, themes and cache out of
        // the tests.
        .env_remove("HL_CONFIG")
        .env("XDG_CONFIG_HOME", "/nonexistent/hl-test-config")
        .env("XDG_CACHE_HOME", "/nonexistent/hl-test-cache");
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

#[test]
fn prints_completions() {
    for shell in ["bash", "zsh", "fish"] {
        hl().args(["--completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains("line-range"));
    }
}

#[test]
fn rejects_unknown_shell() {
    hl().args(["--completions", "nosuchshell"])
        .assert()
        .failure()
        .stdout("");
}

/// The man page shows the built-in defaults, not those of the config file.
#[test]
fn prints_man_page_without_config() {
    let dir = tempdir();
    let config = dir.path().join("config.toml");
    fs::write(&config, "theme = \"Nord\"\n").unwrap();
    hl().env("HL_CONFIG", &config)
        .arg("--man")
        .assert()
        .success()
        .stdout(predicate::str::contains(".TH hl 1"))
        .stdout(predicate::str::contains(r"\-\-line\-range"))
        .stdout(predicate::str::contains(r"[default: base16\-ocean.dark]"));
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

/// The GNU coreutils `cat` that `hl` is compared with. Other implementations
/// differ: BSD `cat` (macOS) has no `-A`, and uutils `cat` (Ubuntu 25.10 and
/// later) squeezes blank lines across files differently. `gcat` is the name
/// Homebrew's coreutils uses.
fn gnu_cat() -> &'static str {
    static CAT: OnceLock<&str> = OnceLock::new();
    CAT.get_or_init(|| {
        ["gcat", "cat"]
            .into_iter()
            .find(|cat| {
                std::process::Command::new(cat)
                    .arg("--version")
                    .output()
                    .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("GNU coreutils"))
            })
            .expect("GNU coreutils cat is required (as `gcat` or `cat`)")
    })
}

fn cat_output(opts: &[&str], files: &[std::path::PathBuf]) -> Vec<u8> {
    let out = std::process::Command::new(gnu_cat())
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

/// "// 日本語" in `Shift_JIS` and EUC-JP, which are not valid UTF-8.
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

/// Writes `content` to a config file in `dir` and returns its path.
fn write_config(dir: &Path, content: &str) -> std::path::PathBuf {
    let path = dir.join("config.toml");
    fs::write(&path, content).unwrap();
    path
}

/// `hl` with the config file at `path` given through `HL_CONFIG`.
fn hl_with_config(path: &Path) -> Command {
    let mut cmd = hl();
    cmd.env("HL_CONFIG", path);
    cmd
}

#[test]
fn reads_the_config_from_xdg_config_home() {
    let dir = tempdir();
    fs::create_dir(dir.path().join("hl")).unwrap();
    write_config(&dir.path().join("hl"), "color = \"always\"\n");
    hl().env("XDG_CONFIG_HOME", dir.path())
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::contains(ESC));
}

#[test]
fn hl_config_overrides_the_config_location() {
    let xdg = tempdir();
    fs::create_dir(xdg.path().join("hl")).unwrap();
    write_config(&xdg.path().join("hl"), "color = \"always\"\n");
    let dir = tempdir();
    let config = write_config(dir.path(), "color = \"never\"\n");
    hl_with_config(&config)
        .env("XDG_CONFIG_HOME", xdg.path())
        .args(["--color-depth", "256", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains(ESC).not());
}

#[test]
fn prints_nothing_extra_without_a_config_file() {
    let dir = tempdir();
    let content = fs::read("Cargo.toml").unwrap();
    hl().env("XDG_CONFIG_HOME", dir.path())
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr("")
        .stdout(content);
}

#[test]
fn warns_when_the_hl_config_file_is_missing() {
    let dir = tempdir();
    let content = fs::read("Cargo.toml").unwrap();
    hl_with_config(&dir.path().join("missing.toml"))
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "missing.toml: No such file or directory",
        ))
        .stdout(content);
}

#[test]
fn command_line_beats_env_beats_config() {
    let dir = tempdir();
    let config = write_config(dir.path(), "color-depth = \"truecolor\"\n");
    assert_depth(&mut hl_with_config(&config), TRUECOLOR);
    assert_depth(
        hl_with_config(&config).env("HL_COLOR_DEPTH", "256"),
        ANSI256,
    );
    assert_depth(
        hl_with_config(&config)
            .env("HL_COLOR_DEPTH", "256")
            .args(["--color-depth", "truecolor"]),
        TRUECOLOR,
    );
}

#[test]
fn command_line_overrides_the_config_color() {
    let dir = tempdir();
    let config = write_config(dir.path(), "color = \"always\"\n");
    let content = fs::read("Cargo.toml").unwrap();
    hl_with_config(&config)
        .args(["--color", "never", "Cargo.toml"])
        .assert()
        .success()
        .stdout(content);
}

#[test]
fn reads_the_theme_from_the_config() {
    let dir = tempdir();
    let config = write_config(dir.path(), "theme = \"Nord\"\n");
    let expected =
        colored_stdout(hl().args(["--color", "always", "--theme", "Nord", "Cargo.toml"]));
    let default = colored_stdout(hl().args(["--color", "always", "Cargo.toml"]));
    assert_ne!(expected, default);
    hl_with_config(&config)
        .args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stderr("")
        .stdout(expected);
}

#[test]
fn reads_sizes_from_the_config_as_integers_or_strings() {
    let dir = tempdir();
    let path = dir.path().join("large.rs");
    let content = "fn main() {}\n".repeat(100).into_bytes();
    fs::write(&path, &content).unwrap();
    for size in ["64", "\"64\"", "\"1K\""] {
        let config = write_config(dir.path(), &format!("max-size = {size}\n"));
        hl_with_config(&config)
            .args(["--color", "always"])
            .arg(&path)
            .assert()
            .success()
            .stderr("")
            .stdout(content.clone());
    }
}

#[test]
fn ignores_a_broken_config_file() {
    let dir = tempdir();
    let config = write_config(dir.path(), "color = \"always\"\ntheme =\n");
    let content = fs::read("Cargo.toml").unwrap();
    hl_with_config(&config)
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr(predicate::str::contains("invalid TOML, ignoring the file"))
        .stdout(content);
}

#[test]
fn ignores_bad_config_entries_and_keeps_the_rest() {
    let dir = tempdir();
    let config = write_config(
        dir.path(),
        "color = \"always\"\ncolor-depth = \"16\"\nnumber = 1\npaging = true\n",
    );
    hl_with_config(&config)
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr(
            predicate::str::contains("invalid value '16' for 'color-depth', ignoring it")
                .and(predicate::str::contains(
                    "unknown key 'number', ignoring it",
                ))
                .and(predicate::str::contains(
                    "'paging' must be a string or an integer, ignoring it",
                )),
        )
        .stdout(predicate::str::contains(ANSI256));
}

#[test]
fn reports_an_invalid_env_value_as_before_with_a_config() {
    let dir = tempdir();
    let config = write_config(dir.path(), "color-depth = \"256\"\n");
    hl_with_config(&config)
        .env("HL_COLOR_DEPTH", "16")
        .arg("Cargo.toml")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("invalid value '16' for '--color-depth")
                .and(predicate::str::contains("config.toml").not()),
        );
}

const INI_TEXT: &str = "[section]\nkey = value\n";

/// Colored output of `hl` with `args`, which must succeed without warnings.
fn colored_with(cmd: &mut Command, args: &[&str]) -> Vec<u8> {
    cmd.args(["--color", "always"])
        .args(args)
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone()
}

/// A file named `name` in `dir` holding `content`, returned as a string path.
fn fixture(dir: &Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn maps_a_glob_to_a_language() {
    let dir = tempdir();
    let file = fixture(dir.path(), "app.myconf", INI_TEXT);
    let ini = colored_with(&mut hl(), &["-l", "ini", &file]);
    assert_ne!(colored_with(&mut hl(), &[&file]), ini);
    assert_eq!(
        colored_with(&mut hl(), &["--map-syntax", "*.myconf:INI", &file]),
        ini
    );
}

#[test]
fn maps_an_exact_file_name() {
    let dir = tempdir();
    let file = fixture(dir.path(), "Settings", INI_TEXT);
    assert_eq!(
        colored_with(&mut hl(), &["--map-syntax", "Settings:ini", &file]),
        colored_with(&mut hl(), &["-l", "ini", &file])
    );
}

#[test]
fn mapping_beats_extension_detection() {
    let dir = tempdir();
    let file = fixture(dir.path(), "main.rs", "def f(): pass\n");
    assert_eq!(
        colored_with(&mut hl(), &["--map-syntax", "*.rs:Python", &file]),
        colored_with(&mut hl(), &["-l", "python", &file])
    );
}

#[test]
fn language_option_beats_mapping() {
    let dir = tempdir();
    let file = fixture(dir.path(), "main.rs", "fn main() {}\n");
    assert_eq!(
        colored_with(
            &mut hl(),
            &["--map-syntax", "*.rs:Python", "-l", "rust", &file]
        ),
        colored_with(&mut hl(), &["-l", "rust", &file])
    );
}

#[test]
fn matches_a_pattern_with_a_slash_against_the_full_path() {
    let dir = tempdir();
    let inside = fixture(dir.path(), "sub/app.myconf", INI_TEXT);
    let outside = fixture(dir.path(), "app.myconf", INI_TEXT);
    let map = ["--map-syntax", "**/sub/*.myconf:INI"];
    let ini = colored_with(&mut hl(), &["-l", "ini", &inside]);
    assert_eq!(
        colored_with(&mut hl(), &[&map[..], &[&inside]].concat()),
        ini
    );
    assert_ne!(
        colored_with(&mut hl(), &[&map[..], &[&outside]].concat()),
        ini
    );
    // A relative path is made absolute before matching.
    let relative = colored_with(
        hl().current_dir(dir.path()),
        &[&map[..], &["sub/app.myconf"]].concat(),
    );
    assert_eq!(relative, ini);
}

#[test]
fn later_mapping_wins() {
    let dir = tempdir();
    let file = fixture(dir.path(), "app.myconf", INI_TEXT);
    assert_eq!(
        colored_with(
            &mut hl(),
            &[
                "--map-syntax",
                "*:Python",
                "--map-syntax",
                "*.myconf:INI",
                &file
            ]
        ),
        colored_with(&mut hl(), &["-l", "ini", &file])
    );
}

#[test]
fn reads_mappings_from_the_config() {
    let dir = tempdir();
    let file = fixture(dir.path(), "app.myconf", INI_TEXT);
    let config = write_config(dir.path(), "map-syntax = [\"*.myconf:INI\"]\n");
    assert_eq!(
        colored_with(&mut hl_with_config(&config), &[&file]),
        colored_with(&mut hl(), &["-l", "ini", &file])
    );
}

#[test]
fn command_line_mapping_beats_config_mapping() {
    let dir = tempdir();
    let file = fixture(dir.path(), "app.myconf", INI_TEXT);
    let config = write_config(
        dir.path(),
        "map-syntax = [\"*.myconf:Python\", \"*.other:Python\"]\n",
    );
    assert_eq!(
        colored_with(
            &mut hl_with_config(&config),
            &["--map-syntax", "*.myconf:INI", &file]
        ),
        colored_with(&mut hl(), &["-l", "ini", &file])
    );
    // The command line adds to the config mappings rather than replacing them.
    let other = fixture(dir.path(), "x.other", INI_TEXT);
    assert_eq!(
        colored_with(
            &mut hl_with_config(&config),
            &["--map-syntax", "*.myconf:INI", &other]
        ),
        colored_with(&mut hl(), &["-l", "python", &other])
    );
}

#[test]
fn does_not_map_stdin() {
    assert_eq!(
        colored_with(hl().write_stdin(INI_TEXT), &["--map-syntax", "*:INI"]),
        colored_with(hl().write_stdin(INI_TEXT), &[])
    );
}

#[test]
fn rejects_a_malformed_mapping() {
    for spec in ["nocolon", ":INI", "*.conf:", "[:INI"] {
        hl().args(["--map-syntax", spec, "Cargo.toml"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid value"))
            .stdout("");
    }
}

#[test]
fn rejects_an_unknown_language_in_a_mapping() {
    for color in ["always", "never"] {
        hl().args(["--color", color, "--map-syntax", "*.conf:no-such-lang"])
            .write_stdin("x\n")
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "unknown language 'no-such-lang' in map-syntax '*.conf:no-such-lang'",
            ))
            .stdout("");
    }
}

#[test]
fn warns_about_bad_config_mappings_and_keeps_the_rest() {
    let dir = tempdir();
    let file = fixture(dir.path(), "app.myconf", INI_TEXT);
    let config = write_config(
        dir.path(),
        "map-syntax = [\"*.myconf:no-such-lang\", \"bad\", \"*.myconf:INI\"]\n",
    );
    let out = hl_with_config(&config)
        .args(["--color", "always", &file])
        .assert()
        .success()
        .stderr(
            predicate::str::contains(
                "unknown language 'no-such-lang' in map-syntax '*.myconf:no-such-lang', ignoring it",
            )
            .and(predicate::str::contains(
                "invalid map-syntax entry 'bad'",
            )),
        )
        .get_output()
        .stdout
        .clone();
    assert_eq!(out, colored_with(&mut hl(), &["-l", "ini", &file]));
}

#[test]
fn warns_about_config_mappings_of_the_wrong_type() {
    let dir = tempdir();
    for (content, warning) in [
        (
            "map-syntax = \"*.conf:INI\"\n",
            "'map-syntax' must be an array of strings",
        ),
        (
            "map-syntax = [1]\n",
            "'map-syntax' must be an array of strings",
        ),
        (
            "theme = [\"Nord\"]\n",
            "'theme' must be a string or an integer",
        ),
        ("number = [\"1\"]\n", "unknown key 'number'"),
    ] {
        let config = write_config(dir.path(), content);
        hl_with_config(&config)
            .arg("Cargo.toml")
            .assert()
            .success()
            .stderr(predicate::str::contains(warning));
    }
}

/// A config directory for custom syntaxes and themes, with its own cache.
struct Assets {
    dir: TempDir,
}

const HLTEST_SYNTAX: &str = "%YAML 1.2
---
name: HlTest
file_extensions: [hltest]
scope: source.hltest
contexts:
  main:
    - match: \\bkw\\b
      scope: keyword.control.hltest
";

const RED_THEME: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>name</key><string>Red</string>
<key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#FF0000</string></dict></dict>
</array></dict></plist>
"#;

const NOT_CACHED: &str = "custom syntaxes are not cached";

impl Assets {
    fn new() -> Self {
        Self { dir: tempdir() }
    }

    fn path(&self, sub: &str) -> std::path::PathBuf {
        self.dir.path().join(sub)
    }

    fn syntax(&self, name: &str, content: &str) -> &Self {
        let dir = self.path("config/hl/syntaxes");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.sublime-syntax")), content).unwrap();
        self
    }

    fn theme(&self, name: &str, content: &str) -> &Self {
        let dir = self.path("config/hl/themes");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.tmTheme")), content).unwrap();
        self
    }

    fn file(&self, name: &str, content: &str) -> String {
        let path = self.path(name);
        fs::write(&path, content).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn cache(&self) -> std::path::PathBuf {
        self.path("cache/hl/syntaxes.bin")
    }

    fn hl(&self) -> Command {
        let mut cmd = hl();
        cmd.env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_CACHE_HOME", self.path("cache"));
        cmd
    }

    fn colored(&self, args: &[&str]) -> Vec<u8> {
        colored_stdout(
            self.hl()
                .args(["--color", "always", "--color-depth", "truecolor"])
                .args(args),
        )
    }
}

#[test]
fn highlights_with_a_custom_syntax() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    let file = assets.file("a.hltest", "kw other\n");
    let plain = assets.file("a.txt", "kw other\n");
    let detected = assets.colored(&[&file]);
    assert_ne!(detected, assets.colored(&[&plain]));
    assert_eq!(detected, assets.colored(&["-l", "hltest", &plain]));
    assert_eq!(detected, assets.colored(&["-l", "HlTest", &plain]));
    assets
        .hl()
        .arg("--list-languages")
        .assert()
        .success()
        .stdout(predicate::str::contains("HlTest: hltest\n"));
}

#[test]
fn custom_syntax_replaces_a_bundled_one() {
    let assets = Assets::new();
    assets.syntax(
        "MyRust",
        &HLTEST_SYNTAX
            .replace("[hltest]", "[rs]")
            .replace("HlTest", "MyRust"),
    );
    let file = assets.file("a.rs", "kw fn main() {}\n");
    let detected = assets.colored(&[&file]);
    assert_eq!(detected, assets.colored(&["-l", "MyRust", &file]));
    assert_ne!(detected, assets.colored(&["-l", "Rust", &file]));
}

#[test]
fn skips_custom_syntaxes_that_cannot_be_loaded() {
    let assets = Assets::new();
    assets
        .syntax("HlTest", HLTEST_SYNTAX)
        .syntax("BadYaml", "name: x\n  bad: [\n")
        .syntax(
            "BadRegex",
            &HLTEST_SYNTAX.replace("\\bkw\\b", "'(unclosed'"),
        );
    let file = assets.file("a.hltest", "kw other\n");
    let expected = assets.colored(&["-l", "hltest", &file]);
    assets
        .hl()
        .args(["--color", "always", "--color-depth", "truecolor", &file])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("BadYaml.sublime-syntax: cannot load syntax, skipping it")
                .and(predicate::str::contains(
                    "BadRegex.sublime-syntax: cannot load syntax, skipping it",
                )),
        )
        .stdout(expected);
}

#[test]
fn caches_custom_syntaxes() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    let file = assets.file("a.hltest", "kw other\n");
    let uncached = assets
        .hl()
        .args(["--color", "always", &file])
        .assert()
        .success()
        .stderr(predicate::str::contains(NOT_CACHED))
        .get_output()
        .stdout
        .clone();

    assets
        .hl()
        .arg("--build-cache")
        .assert()
        .success()
        .stdout(predicate::str::contains("cached 1 of 1 custom syntaxes"));
    assert!(assets.cache().is_file());
    assets
        .hl()
        .args(["--color", "always", &file])
        .assert()
        .success()
        .stderr("")
        .stdout(uncached);

    // A changed syntax makes the cache stale until it is built again.
    assets.syntax(
        "Other",
        &HLTEST_SYNTAX
            .replace("hltest", "other")
            .replace("HlTest", "Other"),
    );
    assets
        .hl()
        .arg("--list-languages")
        .assert()
        .success()
        .stderr(predicate::str::contains(NOT_CACHED))
        .stdout(predicate::str::contains("Other: other\n"));
    assets.hl().arg("--build-cache").assert().success();
    assets
        .hl()
        .arg("--list-languages")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::contains("Other: other\n"));

    assets
        .hl()
        .arg("--clear-cache")
        .assert()
        .success()
        .stderr("");
    assert!(!assets.cache().exists());
    assets
        .hl()
        .arg("--clear-cache")
        .assert()
        .success()
        .stderr("");
}

#[test]
fn build_cache_without_custom_syntaxes_removes_the_cache() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    assets.hl().arg("--build-cache").assert().success();
    fs::remove_dir_all(assets.path("config/hl/syntaxes")).unwrap();
    assets
        .hl()
        .arg("--build-cache")
        .assert()
        .success()
        .stdout(predicate::str::contains("no custom syntaxes"));
    assert!(!assets.cache().exists());
}

#[test]
fn bundled_syntaxes_need_no_cache() {
    let assets = Assets::new();
    assets
        .hl()
        .args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn uses_a_custom_theme() {
    let assets = Assets::new();
    assets.theme("Red", RED_THEME);
    let file = assets.file("a.txt", "text\n");
    let out = assets.colored(&["--theme", "Red", &file]);
    assert!(
        String::from_utf8(out.clone())
            .unwrap()
            .contains("\x1b[38;2;255;0;0m")
    );
    assert_eq!(out, assets.colored(&["--theme", "red", &file]));
    assets
        .hl()
        .arg("--list-themes")
        .assert()
        .success()
        .stdout(predicate::str::contains("\nRed\n"));
}

#[test]
fn custom_theme_replaces_a_bundled_one() {
    let assets = Assets::new();
    assets.theme("Nord", RED_THEME);
    let file = assets.file("a.txt", "text\n");
    let out = assets.colored(&["--theme", "Nord", &file]);
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("\x1b[38;2;255;0;0m")
    );
    let listed = colored_stdout(assets.hl().arg("--list-themes"));
    let listed = String::from_utf8(listed).unwrap();
    assert_eq!(listed.lines().filter(|&n| n == "Nord").count(), 1);
}

#[test]
fn skips_custom_themes_that_cannot_be_loaded() {
    let assets = Assets::new();
    assets.theme("Broken", "not a theme\n");
    let file = assets.file("a.txt", "text\n");
    assets
        .hl()
        .arg("--list-themes")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Broken.tmTheme: cannot load theme, skipping it",
        ))
        .stdout(predicate::str::contains("Broken").not());
    let expected = assets.colored(&[&file]);
    assets
        .hl()
        .args([
            "--color",
            "always",
            "--color-depth",
            "truecolor",
            "--theme",
            "Broken",
            &file,
        ])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("Broken.tmTheme: cannot load theme").and(
                predicate::str::contains("falling back to base16-ocean.dark"),
            ),
        )
        .stdout(expected);
}
