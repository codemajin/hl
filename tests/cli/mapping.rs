//! `--map-syntax` and `map-syntax` in the config: that the mappings reach
//! the language detection, and how bad ones are reported. Which mapping
//! wins and how patterns match are covered by the unit tests in
//! `src/highlight.rs`.

use predicates::prelude::*;

use crate::common::{colored, files, hl, hl_with_config, tempdir};

const INI_TEXT: &str = "[section]\nkey = value\n";

#[test]
fn maps_a_glob_to_a_language() {
    let dir = tempdir();
    let file = files(dir.path(), &[("app.myconf", INI_TEXT)]);
    let ini = colored(hl().args(["-l", "ini"]).args(&file));
    assert_ne!(colored(hl().args(&file)), ini);
    assert_eq!(
        colored(hl().args(["--map-syntax", "*.myconf:INI"]).args(&file)),
        ini
    );
}

#[test]
fn language_option_beats_mapping() {
    let dir = tempdir();
    let file = files(dir.path(), &[("main.rs", "fn main() {}\n")]);
    assert_eq!(
        colored(
            hl().args(["--map-syntax", "*.rs:Python", "-l", "rust"])
                .args(&file)
        ),
        colored(hl().args(["-l", "rust"]).args(&file))
    );
}

#[test]
fn reads_mappings_from_the_config() {
    let dir = tempdir();
    let created = files(
        dir.path(),
        &[
            ("app.myconf", INI_TEXT),
            ("config.toml", "map-syntax = [\"*.myconf:INI\"]\n"),
        ],
    );
    let (file, config) = (&created[0], &created[1]);
    let mapped = hl_with_config(config)
        .args(["--color", "always"])
        .arg(file)
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    assert_eq!(mapped, colored(hl().args(["-l", "ini"]).arg(file)));
}

#[test]
fn does_not_map_stdin() {
    assert_eq!(
        colored(hl().args(["--map-syntax", "*:INI"]).write_stdin(INI_TEXT)),
        colored(hl().write_stdin(INI_TEXT))
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
    let created = files(
        dir.path(),
        &[
            ("app.myconf", INI_TEXT),
            (
                "config.toml",
                "map-syntax = [\"*.myconf:no-such-lang\", \"bad\", \"*.myconf:INI\"]\n",
            ),
        ],
    );
    let (file, config) = (&created[0], &created[1]);
    let out = hl_with_config(config)
        .args(["--color", "always"])
        .arg(file)
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
    assert_eq!(out, colored(hl().args(["-l", "ini"]).arg(file)));
}
