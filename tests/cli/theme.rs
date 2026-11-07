//! Choosing the theme, and falling back on an unknown one.

use std::fs;

use predicates::prelude::*;

use crate::common::{NOT_FOUND, colored, fixture, hl, strip_ansi};

#[test]
fn theme_long_option_selects_the_theme() {
    let default = colored(hl().arg(fixture("sample.toml")));
    let nord = colored(hl().args(["--theme", "Nord"]).arg(fixture("sample.toml")));
    assert_ne!(nord, default, "--theme Nord should change the colors");
    assert_eq!(strip_ansi(&nord), fs::read(fixture("sample.toml")).unwrap());
}

#[test]
fn short_t_is_show_tabs_not_theme() {
    // `-t` is cat's -vT, so "Nord" is read as a file name.
    hl().args(["-t", "Nord"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!("hl: Nord: {NOT_FOUND}")));
}

#[test]
fn falls_back_on_unknown_theme() {
    let expected = colored(hl().arg(fixture("sample.toml")));
    hl().args(["--color", "always", "--theme", "NoSuchTheme"]).arg(fixture("sample.toml"))
        .assert()
        .success()
        .stderr("hl: unknown theme 'NoSuchTheme', falling back to base16-ocean.dark (see --list-themes)\n")
        .stdout(expected);
}

#[test]
fn falls_back_on_unknown_theme_from_env() {
    let expected = colored(hl().arg(fixture("sample.toml")));
    hl().env("HL_THEME", "NoSuchTheme")
        .args(["--color", "always"])
        .arg(fixture("sample.toml"))
        .assert()
        .success()
        .stderr(predicate::str::contains("unknown theme 'NoSuchTheme'"))
        .stdout(expected);
}

#[test]
fn matches_theme_case_insensitively() {
    let expected = colored(hl().args(["--theme", "Nord"]).arg(fixture("sample.toml")));
    hl().args(["--color", "always", "--theme", "nord"])
        .arg(fixture("sample.toml"))
        .assert()
        .success()
        .stderr("")
        .stdout(expected);
}

#[test]
fn ignores_unknown_theme_without_color() {
    let content = fs::read(fixture("sample.rs")).unwrap();
    hl().args(["--theme", "NoSuchTheme"])
        .arg(fixture("sample.rs"))
        .assert()
        .success()
        .stderr("")
        .stdout(content);
}

#[test]
fn ignores_unknown_theme_with_language_but_without_color() {
    let content = fs::read(fixture("sample.rs")).unwrap();
    hl().args(["--theme", "NoSuchTheme", "-l", "rust", "--color", "never"])
        .arg(fixture("sample.rs"))
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
