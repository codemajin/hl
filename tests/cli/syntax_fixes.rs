//! The fixes for the bundled syntaxes that used to hang (see `build.rs`):
//! JavaScript on a comment after an expression, and Scala on an operator
//! after `@` in a type.

use std::time::Duration;

use crate::common::{files, has_escape, hl, strip_ansi, tempdir};

/// Runs `hl` on `input` with `args`, failing the test if it takes long or
/// warns (as it does when a line takes longer than 3 seconds).
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

#[test]
fn highlights_an_operator_after_at_in_a_scala_type() {
    colored_in_time(&["-l", "scala"], ":a@<");
    colored_in_time(&["-l", "scala"], "type L = A @> B\n");
    colored_in_time(&["-l", "scala"], "x match { case y: A @> B => 1 }\n");
}
