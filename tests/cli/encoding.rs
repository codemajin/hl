//! Inputs that are not UTF-8: legacy encodings, UTF-16 and the UTF-8 BOM.

use crate::common::{colored, files, has_escape, hl, stdout, strip_ansi, tempdir, utf16_with_bom};

/// "// 日本語" in `Shift_JIS` and EUC-JP, which are not valid UTF-8.
const SJIS_COMMENT: &[u8] = b"// \x93\xfa\x96\x7b\x8c\xea\n";
const EUC_JP_COMMENT: &[u8] = b"// \xc6\xfc\xcb\xdc\xb8\xec\n";

#[test]
fn prints_non_utf8_files_unchanged() {
    for (name, comment) in [("sjis.rs", SJIS_COMMENT), ("eucjp.rs", EUC_JP_COMMENT)] {
        let dir = tempdir();
        let content = [comment, b"fn main() {}\n"].concat();
        let path = files(dir.path(), &[(name, &content)]);
        hl().args(["--color", "always"])
            .args(&path)
            .assert()
            .success()
            .stdout(content);
    }
}

#[test]
fn stops_highlighting_at_a_non_utf8_line() {
    let dir = tempdir();
    // Past the first 8 KB, so only the line-by-line check can catch it.
    let mut content = "fn main() {}\n\n\n".repeat(1000).into_bytes();
    content.extend_from_slice(SJIS_COMMENT);
    content.extend_from_slice(b"\n\nfn after() {}\n");
    let files = files(dir.path(), &[("late-sjis.rs", content)]);
    for opts in [&[][..], &["-n"], &["-sb"]] {
        let out = colored(hl().args(opts).args(&files));
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
            stdout(hl().args(opts).args(&files)),
            "hl {opts:?} should print the same text as without color"
        );
    }
}

#[test]
fn highlights_utf16_as_utf8() {
    let text = "// 日本語 😀\nfn main() {}\n";
    for big_endian in [false, true] {
        let dir = tempdir();
        let utf16 = files(
            dir.path(),
            &[(
                format!("utf16-{big_endian}.rs").as_str(),
                utf16_with_bom(text, big_endian),
            )],
        );
        let utf8 = files(
            dir.path(),
            &[(format!("utf8-{big_endian}.rs").as_str(), text)],
        );
        assert_eq!(colored(hl().args(&utf16)), colored(hl().args(&utf8)));
        let numbered = colored(hl().arg("-n").args(&utf16));
        assert_eq!(strip_ansi(&numbered), stdout(hl().arg("-n").args(&utf8)));
    }
}

#[test]
fn passes_utf16_through_without_color() {
    let dir = tempdir();
    let content = utf16_with_bom("fn main() {}\n", false);
    let path = files(dir.path(), &[("utf16-plain.rs", &content)]);
    hl().args(&path).assert().success().stdout(content);
}

#[test]
fn replaces_broken_utf16_with_replacement_characters() {
    let dir = tempdir();
    // "a", a lone high surrogate, "b", then an odd trailing byte.
    let path = files(
        dir.path(),
        &[("broken-utf16.txt", b"\xff\xfea\x00\x00\xd8b\x00\n\x00x")],
    );
    let out = colored(hl().args(&path));
    assert_eq!(strip_ansi(&out), "a\u{FFFD}b\n\u{FFFD}".as_bytes());
}

#[test]
fn keeps_utf8_bom_and_detects_the_shebang_after_it() {
    let dir = tempdir();
    let script = "#!/bin/sh\necho hi\n";
    let plain = files(dir.path(), &[("script", script)]);
    let with_bom = files(
        dir.path(),
        &[("script-bom", [b"\xEF\xBB\xBF", script.as_bytes()].concat())],
    );
    let expected = colored(hl().args(&plain));
    assert_eq!(
        expected,
        colored(hl().args(["-l", "sh"]).args(&plain)),
        "the shebang should select sh"
    );
    let out = colored(hl().args(&with_bom));
    assert_eq!(out, [b"\xEF\xBB\xBF", expected.as_slice()].concat());
}

/// How many leading bytes `hl` looks at to tell binary and invalid UTF-8.
const PEEK_LEN: usize = 8 * 1024;

/// A Rust file whose first `offset` bytes are lines of `x`, followed by
/// `tail` and another line.
fn padded(offset: usize, tail: &[u8]) -> Vec<u8> {
    let mut bytes: Vec<u8> = (0..offset)
        .map(|i| if i % 64 == 63 { b'\n' } else { b'x' })
        .collect();
    bytes.extend_from_slice(tail);
    bytes.extend_from_slice(b"\nfn main() {}\n");
    bytes
}

/// The colored output of `content` read from a file, not a pipe, so that
/// the first read gets the whole 8 KB.
fn colored_file(content: &[u8]) -> Vec<u8> {
    let dir = tempdir();
    let path = files(dir.path(), &[("peek.rs", content)]);
    colored(hl().args(&path))
}

#[test]
fn treats_a_nul_in_the_first_8_kb_as_binary() {
    // The NUL is the last byte of the first 8 KB.
    let content = padded(PEEK_LEN - 1, b"\0");
    assert_eq!(colored_file(&content), content);
}

#[test]
fn highlights_a_nul_past_the_first_8_kb() {
    // The NUL is the first byte after the first 8 KB.
    let content = padded(PEEK_LEN, b"\0");
    let out = colored_file(&content);
    assert!(has_escape(&out));
    assert_eq!(strip_ansi(&out), content);
}

#[test]
fn highlights_a_character_cut_off_by_the_first_8_kb() {
    // "日" takes 3 bytes, the first 2 of them in the first 8 KB.
    let content = padded(PEEK_LEN - 2, "日".as_bytes());
    let out = colored_file(&content);
    assert!(has_escape(&out));
    assert_eq!(strip_ansi(&out), content);
}

#[test]
fn drops_the_utf8_bom_with_its_line() {
    let dir = tempdir();
    let path = files(dir.path(), &[("bom.rs", "\u{FEFF}// one\nfn main() {}\n")]);
    hl().args(["-r", "2:"])
        .args(&path)
        .assert()
        .success()
        .stdout("fn main() {}\n");
    let out = colored(hl().args(["-r", "2:"]).args(&path));
    assert!(has_escape(&out));
    assert_eq!(strip_ansi(&out), b"fn main() {}\n");
}

#[test]
fn applies_line_options_to_utf16_after_converting_it() {
    let text = "// one\nfn two() {}\nfn three() {}\nfn four() {}\n";
    for big_endian in [false, true] {
        let dir = tempdir();
        let utf16 = files(
            dir.path(),
            &[("utf16.rs", utf16_with_bom(text, big_endian))],
        );
        let utf8 = files(dir.path(), &[("utf8.rs", text)]);
        for opts in [
            &["-r", "2:3"][..],
            &["-H", "2"],
            &["-n", "-r", "2:3", "-H", "3"],
        ] {
            assert_eq!(
                colored(hl().args(opts).args(&utf16)),
                colored(hl().args(opts).args(&utf8)),
                "hl {opts:?}, big endian: {big_endian}"
            );
        }
        let out = colored(hl().args(["-r", "2:3"]).args(&utf16));
        assert_eq!(strip_ansi(&out), b"fn two() {}\nfn three() {}\n");
        let out = colored(hl().args(["-H", "2"]).args(&utf16));
        let lines: Vec<&[u8]> = out.split_inclusive(|&b| b == b'\n').collect();
        assert!(lines[1].starts_with(b"\x1b[48;"), "{out:?}");
        assert!(!lines[0].starts_with(b"\x1b[48;"), "{out:?}");
    }
}

#[test]
fn prints_nothing_for_a_utf16_bom_alone() {
    let dir = tempdir();
    let path = files(dir.path(), &[("bom-only.txt", b"\xFF\xFE")]);
    hl().args(["--color", "always"])
        .args(&path)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn prints_utf32_unchanged_as_binary() {
    let dir = tempdir();
    // "a\n" in UTF-32LE with its BOM, which starts like the UTF-16LE BOM.
    let content = b"\xFF\xFE\x00\x00a\x00\x00\x00\n\x00\x00\x00";
    let path = files(dir.path(), &[("utf32.txt", content)]);
    for opts in [&["--color", "always"][..], &["--strip-ansi", "always"]] {
        hl().args(opts)
            .args(&path)
            .assert()
            .success()
            .stdout(&content[..]);
    }
}
