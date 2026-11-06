//! Escape sequences and control characters: removed from the input, and
//! shown visibly in file names and messages.

use predicates::prelude::*;

use crate::common::{
    colored, files, has_escape, hl, hl_with_config, strip_ansi, tempdir, utf16_with_bom,
};

/// The issue's example, with an OSC title, a hyperlink and colors.
const ESCAPED_INPUT: &[u8] =
    b"a\x1b[31mred\x1b[0m\n\x1b]0;title\x07\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\\n";

fn assert_no_input_escapes(out: &[u8]) {
    let text = String::from_utf8_lossy(out);
    for seq in ["\x1b[31m", "\x1b]", "\x07", "\x1b\\"] {
        assert!(!text.contains(seq), "{seq:?} left in {text:?}");
    }
}

#[test]
fn strips_escape_sequences_from_colored_input() {
    let out = colored(hl().args(["-l", "txt"]).write_stdin(ESCAPED_INPUT));
    assert_no_input_escapes(&out);
    assert!(has_escape(&out), "the output should still be colored");
    assert_eq!(strip_ansi(&out), b"ared\nlink\n");
}

#[test]
fn keeps_escape_sequences_without_color() {
    hl().arg("-l")
        .arg("txt")
        .write_stdin(ESCAPED_INPUT)
        .assert()
        .success()
        .stdout(ESCAPED_INPUT);
}

#[test]
fn keeps_escape_sequences_with_strip_ansi_never() {
    let out = colored(
        hl().args(["--strip-ansi", "never", "-l", "txt"])
            .write_stdin(ESCAPED_INPUT),
    );
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("\x1b[31mred"), "{text:?}");
    assert!(text.contains("\x1b]0;title\x07"), "{text:?}");
}

#[test]
fn strips_escape_sequences_without_color_when_always() {
    hl().args(["--strip-ansi", "always"])
        .write_stdin(ESCAPED_INPUT)
        .assert()
        .success()
        .stdout("ared\nlink\n");
}

#[test]
fn strips_escape_sequences_after_highlighting_stops() {
    let out = colored(
        hl().args(["--max-line-length", "4", "-l", "txt"])
            .write_stdin("ab\nabc\x1b[31mdef\x1b]0;t\x07\nx\x1b[0m\n"),
    );
    assert!(out.ends_with(b"abcdef\nx\n"), "{out:?}");
    assert!(has_escape(&out[..out.len() - 9]), "{out:?}");
}

#[test]
fn strips_escape_sequences_from_colored_binary_and_non_utf8_input() {
    for input in [
        &b"\x00a\x1b[31mb\x1b]0;t\x07\n"[..],
        b"\xffa\x1b[31mb\x1b]0;t\x07\n",
    ] {
        let mut expected = input[..1].to_vec();
        expected.extend_from_slice(b"ab\n");
        hl().args(["--color", "always"])
            .write_stdin(input)
            .assert()
            .success()
            .stdout(expected);
    }
}

#[test]
fn strips_escape_sequences_from_utf16_after_decoding() {
    let out = colored(
        hl().args(["-l", "txt"])
            .write_stdin(utf16_with_bom("a\x1b[31mb\x1b]0;t\x07\n", false)),
    );
    assert_no_input_escapes(&out);
    assert_eq!(strip_ansi(&out), b"ab\n");
}

#[test]
fn strips_escape_sequences_from_utf16_over_the_size_limit() {
    let dir = tempdir();
    let path = files(
        dir.path(),
        &[("large-utf16.txt", utf16_with_bom("a\x1b]0;P\x07b\n", false))],
    );
    for args in [
        &["--color", "always"][..],
        &["--color", "never", "--strip-ansi", "always"],
    ] {
        hl().args(args)
            .args(["--max-size", "1"])
            .args(&path)
            .assert()
            .success()
            .stdout("ab\n");
    }
}

#[test]
fn strips_c1_controls_encoded_in_utf8() {
    let input = b"a\xc2\x9b31mred\xc2\x9d0;t\xc2\x9c\xc2\x85\n";
    hl().args(["--strip-ansi", "always"])
        .write_stdin(&input[..])
        .assert()
        .success()
        .stdout("ared\n");
    let out = colored(hl().args(["-l", "txt"]).write_stdin(&input[..]));
    assert_eq!(strip_ansi(&out), b"ared\n");
    hl().write_stdin(&input[..])
        .assert()
        .success()
        .stdout(&input[..]);
}

#[test]
fn strips_c0_controls_but_keeps_raw_bytes() {
    // An 8-bit CSI and OSC as raw bytes, BEL, SO, BS and VT. The raw bytes
    // are kept, as they are not UTF-8 and may be Shift_JIS.
    let input = b"a\x9b31mRED\x9d0;PWN\x07b\x0e\x08\x0bc\x00\t\r\n";
    let expected = b"a\x9b31mRED\x9d0;PWNbc\x00\t\r\n";
    hl().args(["--strip-ansi", "always"])
        .write_stdin(&input[..])
        .assert()
        .success()
        .stdout(&expected[..]);
    hl().args(["--color", "always"])
        .write_stdin(&input[..])
        .assert()
        .success()
        .stdout(&expected[..]);
    hl().write_stdin(&input[..])
        .assert()
        .success()
        .stdout(&input[..]);
}

#[test]
fn strips_c0_controls_from_highlighted_input() {
    let out = colored(hl().args(["-l", "txt"]).write_stdin("a\x0eb\x07c\x08\n"));
    assert!(has_escape(&out));
    assert_eq!(strip_ansi(&out), b"abc\n");
}

/// A file name that sets the window title.
const ESCAPED_NAME: &str = "x\x1b]0;PWN\x07y";
const VISIBLE_NAME: &str = r"x\x1b]0;PWN\x07y";

#[cfg(unix)]
#[test]
fn shows_control_characters_of_file_names_in_headers() {
    let dir = tempdir();
    let files = files(dir.path(), &[(ESCAPED_NAME, "one\n")]);
    let out = hl().arg("--header").args(&files).assert().success();
    let stdout = &out.get_output().stdout;
    let text = String::from_utf8_lossy(stdout);
    assert!(!text.contains(['\x1b', '\x07']), "{text:?}");
    assert!(
        text.starts_with("==> ") && text.ends_with(&format!("{VISIBLE_NAME} <==\none\n")),
        "{text:?}"
    );
}

#[test]
fn shows_control_characters_of_file_names_in_errors() {
    let dir = tempdir();
    let missing = dir.path().join(ESCAPED_NAME);
    let out = hl().arg(&missing).assert().code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(!stderr.contains(['\x1b', '\x07']), "{stderr:?}");
    assert!(stderr.contains(&format!("{VISIBLE_NAME}: ")), "{stderr:?}");
}

#[test]
fn shows_control_characters_of_theme_names() {
    hl().env("HL_THEME", "t\x1b[2J")
        .args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stderr(predicate::str::contains(r"unknown theme 't\x1b[2J'"))
        .stderr(predicate::str::contains("\x1b").not());
}

#[test]
fn shows_control_characters_of_fatal_errors() {
    hl().args(["-l", "no\x1b]0;PWN\x07such"])
        .write_stdin("a\n")
        .assert()
        .code(1)
        .stderr("hl: unknown language 'no\\x1b]0;PWN\\x07such' (see --list-languages)\n");
}

#[test]
fn shows_control_characters_of_config_entries() {
    let dir = tempdir();
    let config = files(
        dir.path(),
        &[(
            "config.toml",
            r#""k\u001b[31mX" = 1
color = "n\u001b[33mZ"
number = "v\u001b[35mV"
map-syntax = ["bad\u001b[32mY", "*.q:no\u001b[34mW"]
"#,
        )],
    );
    let out = hl_with_config(&config[0])
        .args(["--color", "always", "-l", "txt"])
        .write_stdin("a\n")
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(!stderr.contains('\x1b'), "{stderr:?}");
    for expected in [
        r"unknown key 'k\x1b[31mX'",
        r"invalid value 'n\x1b[33mZ' for 'color'",
        r"invalid map-syntax entry 'bad\x1b[32mY': 'bad\x1b[32mY' is not PATTERN:LANG",
        r"unknown language 'no\x1b[34mW' in map-syntax '*.q:no\x1b[34mW'",
    ] {
        assert!(stderr.contains(expected), "{expected:?} not in {stderr:?}");
    }
}

#[test]
fn shows_control_characters_of_broken_config_files() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "color = \"\x1b[2J\"\n")]);
    hl_with_config(&config[0])
        .write_stdin("a\n")
        .assert()
        .success()
        .stdout("a\n")
        .stderr(predicate::str::contains("invalid TOML"))
        .stderr(predicate::str::contains("\x1b").not());
}

#[test]
fn reads_strip_ansi_from_the_config() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "strip-ansi = \"never\"\n")]);
    let out = colored(
        hl_with_config(&config[0])
            .args(["-l", "txt"])
            .write_stdin(ESCAPED_INPUT),
    );
    assert!(String::from_utf8_lossy(&out).contains("\x1b[31mred"));
}
