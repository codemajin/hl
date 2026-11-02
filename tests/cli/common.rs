//! Helpers shared by the integration tests.

use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

pub const ESC: &str = "\x1b[";
pub const TRUECOLOR: &str = "\x1b[38;2;";
pub const ANSI256: &str = "\x1b[38;5;";

/// `hl` with `args`, run without `assert_cmd` to control its standard
/// streams. Like `hl()`, it does not see the developer's environment.
pub fn raw_hl(args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_hl"));
    cmd.args(args)
        .env_remove("HL_THEME")
        .env_remove("HL_MAX_SIZE")
        .env_remove("HL_MAX_LINE_LENGTH")
        .env_remove("HL_COLOR_DEPTH")
        .env_remove("COLORTERM")
        .env_remove("WT_SESSION")
        .env("TERM", "xterm-256color")
        // Keeps the user's own config file, syntaxes, themes and cache out of
        // the tests. HOME is set as well, so that a test that unsets an XDG
        // variable never falls back to the user's `~/.config` or `~/.cache`;
        // a test that needs it points it to a temporary directory.
        .env_remove("HL_CONFIG")
        .env("XDG_CONFIG_HOME", "/nonexistent/hl-test-config")
        .env("XDG_CACHE_HOME", "/nonexistent/hl-test-cache")
        .env("HOME", "/nonexistent/hl-test-home");
    cmd
}

/// `hl` with none of the variables it reads set from the developer's
/// environment, except `TERM` set to a fixed value.
pub fn hl() -> Command {
    Command::from_std(raw_hl(&[]))
}

/// `hl` with the config file at `path` given through `HL_CONFIG`.
pub fn hl_with_config(path: &Path) -> Command {
    let mut cmd = hl();
    cmd.env("HL_CONFIG", path);
    cmd
}

/// A directory for test files, removed when the returned value is dropped.
pub fn tempdir() -> TempDir {
    tempfile::Builder::new()
        .prefix("hl-test-")
        .tempdir()
        .unwrap()
}

/// Writes `files` (name, content) into `dir`, creating the directories in
/// the names, and returns their paths.
pub fn files<C: AsRef<[u8]>>(dir: &Path, files: &[(&str, C)]) -> Vec<PathBuf> {
    files
        .iter()
        .map(|(name, content)| {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            path
        })
        .collect()
}

/// The five lines "1" to "5", in a file in `dir`.
pub fn lines_fixture(dir: &Path) -> PathBuf {
    files(dir, &[("lines.txt", "1\n2\n3\n4\n5\n")]).remove(0)
}

/// The stdout of `cmd`, which must succeed.
pub fn stdout(cmd: &mut Command) -> Vec<u8> {
    cmd.assert().success().get_output().stdout.clone()
}

/// The stdout of `cmd` with `--color always`, which must succeed.
pub fn colored(cmd: &mut Command) -> Vec<u8> {
    stdout(cmd.args(["--color", "always"]))
}

/// Removes SGR escape sequences (`ESC [ ... m`).
pub fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\x1b[") {
            match bytes[i..].iter().position(|&b| b == b'm') {
                Some(end) => i += end + 1,
                None => break,
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

pub fn has_escape(bytes: &[u8]) -> bool {
    bytes.windows(ESC.len()).any(|w| w == ESC.as_bytes())
}

pub fn utf16_with_bom(text: &str, big_endian: bool) -> Vec<u8> {
    std::iter::once(0xFEFF)
        .chain(text.encode_utf16())
        .flat_map(|u| {
            if big_endian {
                u.to_be_bytes()
            } else {
                u.to_le_bytes()
            }
        })
        .collect()
}

/// Asserts that `cmd` colors Cargo.toml with `expected` escapes only.
pub fn assert_depth(cmd: &mut Command, expected: &str) {
    let other = if expected == TRUECOLOR {
        ANSI256
    } else {
        TRUECOLOR
    };
    cmd.args(["--color", "always", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains(expected).and(predicate::str::contains(other).not()));
}
