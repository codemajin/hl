//! Commands that print information instead of the inputs, and the help.

use predicates::prelude::*;

use crate::common::{files, hl, hl_with_config, tempdir};

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
        .code(2)
        .stdout("");
}

/// Completions do not depend on the config file, like the man page.
#[test]
fn prints_completions_without_config() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "theme = \"Nord\"\n")]);
    let expected = hl()
        .args(["--completions", "bash"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    hl_with_config(&config[0])
        .args(["--completions", "bash"])
        .assert()
        .success()
        .stderr("")
        .stdout(expected);
}

/// The man page shows the built-in defaults, not those of the config file.
#[test]
fn prints_man_page_without_config() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "theme = \"Nord\"\n")]);
    hl_with_config(&config[0])
        .arg("--man")
        .assert()
        .success()
        .stdout(predicate::str::contains(".TH hl 1"))
        .stdout(predicate::str::contains(r"\-\-line\-range"))
        .stdout(predicate::str::contains(r"[default: base16\-ocean.dark]"));
}

/// Unlike the man page, the help shows the defaults of the config file.
#[test]
fn prints_help_with_config_defaults() {
    let dir = tempdir();
    let config = files(dir.path(), &[("config.toml", "theme = \"Nord\"\n")]);
    hl_with_config(&config[0])
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("[default: Nord]"));
}

/// The `--no-*` options are left out of `-h` to keep it short.
#[test]
fn shows_no_options_only_in_long_help() {
    hl().arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-number").not());
    hl().arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-number"));
}
