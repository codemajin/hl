//! Choosing the theme, and falling back on an unknown one.

use std::fs;

use predicates::prelude::*;

use crate::common::{colored, hl, strip_ansi};

#[test]
fn theme_long_option_selects_the_theme() {
    let default = colored(hl().arg("Cargo.toml"));
    let nord = colored(hl().args(["--theme", "Nord", "Cargo.toml"]));
    assert_ne!(nord, default, "--theme Nord should change the colors");
    assert_eq!(strip_ansi(&nord), fs::read("Cargo.toml").unwrap());
}

#[test]
fn short_t_is_show_tabs_not_theme() {
    // `-t` is cat's -vT, so "Nord" is read as a file name.
    hl().args(["-t", "Nord"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "hl: Nord: No such file or directory",
        ));
}

#[test]
fn falls_back_on_unknown_theme() {
    let expected = colored(hl().arg("Cargo.toml"));
    hl().args(["--color", "always", "--theme", "NoSuchTheme", "Cargo.toml"])
        .assert()
        .success()
        .stderr("hl: unknown theme 'NoSuchTheme', falling back to base16-ocean.dark (see --list-themes)\n")
        .stdout(expected);
}

#[test]
fn falls_back_on_unknown_theme_from_env() {
    let expected = colored(hl().arg("Cargo.toml"));
    hl().env("HL_THEME", "NoSuchTheme")
        .args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stderr(predicate::str::contains("unknown theme 'NoSuchTheme'"))
        .stdout(expected);
}

#[test]
fn matches_theme_case_insensitively() {
    let expected = colored(hl().args(["--theme", "Nord", "Cargo.toml"]));
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
