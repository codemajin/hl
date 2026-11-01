//! The fix for the JavaScript syntax, which used to hang on a comment after
//! an expression (see `build.rs`).

use std::time::Duration;

use crate::common::{files, has_escape, hl, strip_ansi, tempdir};

/// Runs `hl` on `input` as `language`, failing the test if it takes long.
fn colored_in_time(args: &[&str], input: &str) -> Vec<u8> {
    let out = hl()
        .args(["--color", "always"])
        .args(args)
        .write_stdin(input)
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    assert!(has_escape(&out), "{out:?}");
    assert_eq!(strip_ansi(&out), input.as_bytes());
    out
}

#[test]
fn highlights_a_comment_after_an_expression_in_javascript() {
    colored_in_time(&["-l", "js"], "x /*");
    colored_in_time(&["-l", "js"], "foo /* comment */\n");
    colored_in_time(&["-l", "JavaScript (Rails)"], "a /* c */\n");
    colored_in_time(&["-l", "html"], "<script>\nx /*\n</script>\n");
    colored_in_time(&["-l", "md"], "```js\nx /*\n```\n");
    // Detected from the first line rather than given.
    colored_in_time(&[], "<html {t\r/*");
}

#[test]
fn highlights_a_comment_after_an_expression_in_a_javascript_file() {
    let dir = tempdir();
    let file = files(dir.path(), &[("t.js", "x /*")]);
    let out = hl()
        .args(["--color", "always"])
        .args(&file)
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(has_escape(&out), "{out:?}");
    assert_eq!(strip_ansi(&out), b"x /*");
}
