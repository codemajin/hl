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
