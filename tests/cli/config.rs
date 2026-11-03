//! The config file: where it is read from, what it can set, and how bad
//! entries are reported.

use std::fs;

use predicates::prelude::*;

use crate::common::{
    ANSI256, ESC, TRUECOLOR, assert_depth, colored, files, hl, hl_with_config, tempdir,
};

#[test]
fn reads_the_config_from_xdg_config_home() {
    let dir = tempdir();
    files(dir.path(), &[("hl/config.toml", "color = \"always\"\n")]);
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
    files(xdg.path(), &[("hl/config.toml", "color = \"always\"\n")]);
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "color = \"never\"\n")]);
    hl_with_config(&config[0])
        .env("XDG_CONFIG_HOME", xdg.path())
        .args(["--color-depth", "256", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains(ESC).not());
}

/// The XDG spec says a relative path is invalid and must be ignored.
#[test]
fn ignores_a_relative_xdg_config_home() {
    let home = tempdir();
    files(
        home.path(),
        &[(".config/hl/config.toml", "color = \"always\"\n")],
    );
    hl().env("HOME", home.path())
        .env("XDG_CONFIG_HOME", "relative")
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::contains(ESC));
}

#[test]
fn empty_hl_config_counts_as_unset() {
    let dir = tempdir();
    files(dir.path(), &[("hl/config.toml", "color = \"always\"\n")]);
    hl().env("XDG_CONFIG_HOME", dir.path())
        .env("HL_CONFIG", "")
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::contains(ESC));
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
    let config = files(
        dir.path(),
        &[("config.toml", "color-depth = \"truecolor\"\n")],
    );
    assert_depth(&mut hl_with_config(&config[0]), TRUECOLOR);
    assert_depth(
        hl_with_config(&config[0]).env("HL_COLOR_DEPTH", "256"),
        ANSI256,
    );
    assert_depth(
        hl_with_config(&config[0])
            .env("HL_COLOR_DEPTH", "256")
            .args(["--color-depth", "truecolor"]),
        TRUECOLOR,
    );
}

#[test]
fn command_line_overrides_the_config_color() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "color = \"always\"\n")]);
    let content = fs::read("Cargo.toml").unwrap();
    hl_with_config(&config[0])
        .args(["--color", "never", "Cargo.toml"])
        .assert()
        .success()
        .stdout(content);
}

#[test]
fn reads_the_theme_from_the_config() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "theme = \"Nord\"\n")]);
    let expected = colored(hl().args(["--theme", "Nord", "Cargo.toml"]));
    let default = colored(hl().arg("Cargo.toml"));
    assert_ne!(expected, default);
    hl_with_config(&config[0])
        .args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stderr("")
        .stdout(expected);
}

#[test]
fn env_theme_beats_the_config_theme() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "theme = \"Nord\"\n")]);
    let expected = colored(hl().args(["--theme", "Monokai Extended", "Cargo.toml"]));
    let nord = colored(hl().args(["--theme", "Nord", "Cargo.toml"]));
    assert_ne!(expected, nord);
    let out = colored(
        hl_with_config(&config[0])
            .env("HL_THEME", "Monokai Extended")
            .arg("Cargo.toml"),
    );
    assert_eq!(out, expected);
}

#[test]
fn reads_sizes_from_the_config_as_integers_or_strings() {
    let dir = tempdir();
    let content = "fn main() {}\n".repeat(100);
    let path = files(dir.path(), &[("large.rs", &content)]);
    for size in ["64", "\"64\"", "\"1K\""] {
        let config = files(
            dir.path(),
            &[("config.toml", format!("max-size = {size}\n"))],
        );
        hl_with_config(&config[0])
            .args(["--color", "always"])
            .args(&path)
            .assert()
            .success()
            .stderr("")
            .stdout(content.clone());
    }
}

#[test]
fn ignores_a_broken_config_file() {
    let dir = tempdir();
    let config = files(
        dir.path(),
        &[("config.toml", "color = \"always\"\ntheme =\n")],
    );
    let content = fs::read("Cargo.toml").unwrap();
    hl_with_config(&config[0])
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr(predicate::str::contains("invalid TOML, ignoring the file"))
        .stdout(content);
}

#[test]
fn ignores_bad_config_entries_and_keeps_the_rest() {
    let dir = tempdir();
    let config = files(
        dir.path(),
        &[(
            "config.toml",
            "color = \"always\"\ncolor-depth = \"16\"\nnumber = 1\npaging = true\nhighlight = 1\n",
        )],
    );
    hl_with_config(&config[0])
        .arg("Cargo.toml")
        .assert()
        .success()
        .stderr(
            predicate::str::contains("invalid value '16' for 'color-depth', ignoring it")
                .and(predicate::str::contains(
                    "'number' must be true or false, ignoring it",
                ))
                .and(predicate::str::contains(
                    "unknown key 'highlight', ignoring it",
                ))
                .and(predicate::str::contains(
                    "'paging' must be a string or an integer, ignoring it",
                )),
        )
        .stdout(predicate::str::contains(ANSI256));
}

#[test]
fn reads_flags_from_the_config() {
    let dir = tempdir();
    let created = files(
        dir.path(),
        &[
            ("config.toml", "number = true\nheader = true\n"),
            ("a.txt", "one\n\n"),
        ],
    );
    hl_with_config(&created[0])
        .arg(&created[1])
        .assert()
        .success()
        .stderr("")
        .stdout(format!(
            "==> {} <==\n     1\tone\n     2\t\n",
            created[1].display()
        ));
}

#[test]
fn command_line_cancels_config_flags() {
    let dir = tempdir();
    let config = files(
        dir.path(),
        &[("config.toml", "number = true\nheader = true\n")],
    );
    let content = fs::read("Cargo.toml").unwrap();
    hl_with_config(&config[0])
        .args(["--no-number", "--no-header", "Cargo.toml"])
        .assert()
        .success()
        .stderr("")
        .stdout(content);
}

#[test]
fn reports_an_invalid_env_value_by_its_name_with_a_config() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "color-depth = \"256\"\n")]);
    hl_with_config(&config[0])
        .env("HL_COLOR_DEPTH", "16")
        .arg("Cargo.toml")
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("invalid value '16' for HL_COLOR_DEPTH")
                .and(predicate::str::contains("config.toml").not()),
        );
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
        ("number = [\"1\"]\n", "'number' must be true or false"),
        ("pager = [\"less\"]\n", "unknown key 'pager'"),
    ] {
        let config = files(dir.path(), &[("config.toml", content)]);
        hl_with_config(&config[0])
            .arg("Cargo.toml")
            .assert()
            .success()
            .stderr(predicate::str::contains(warning));
    }
}
