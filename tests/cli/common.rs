//! Helpers shared by the integration tests.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

pub const ESC: &str = "\x1b[";
pub const TRUECOLOR: &str = "\x1b[38;2;";
pub const ANSI256: &str = "\x1b[38;5;";

/// The message of the OS for a file that does not exist.
pub const NOT_FOUND: &str = if cfg!(windows) {
    "The system cannot find the file specified."
} else {
    "No such file or directory"
};

/// A directory that does not exist, as an absolute path on every platform
/// (`/nonexistent` has no drive, so it is not absolute on Windows).
fn nonexistent(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("nonexistent")
        .join(name)
}

/// `hl` with `args`, run without `assert_cmd` to control its standard
/// streams. Like `hl()`, it does not see the developer's environment.
pub fn raw_hl(args: &[&str]) -> std::process::Command {
    let mut cmd = isolated(env!("CARGO_BIN_EXE_hl"));
    cmd.args(args);
    cmd
}

/// `program` without the variables that `hl` reads from the developer's
/// environment, for a program that starts `hl` itself, such as `script`.
pub fn isolated(program: impl AsRef<OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    cmd.env_remove("HL_THEME")
        .env_remove("HL_MAX_SIZE")
        .env_remove("HL_MAX_LINE_LENGTH")
        .env_remove("HL_COLOR_DEPTH")
        .env_remove("COLORTERM")
        .env_remove("WT_SESSION")
        .env("TERM", "xterm-256color")
        // Keeps the user's own config file, syntaxes, themes and cache out of
        // the tests. The Windows variables and HOME are set as well, so that
        // a test that unsets an XDG variable never falls back to the user's
        // `%APPDATA%`, `~/.config` and the like; a test that needs one points
        // it to a temporary directory.
        .env_remove("HL_CONFIG")
        .env("XDG_CONFIG_HOME", nonexistent("hl-test-config"))
        .env("XDG_CACHE_HOME", nonexistent("hl-test-cache"))
        .env("APPDATA", nonexistent("hl-test-appdata"))
        .env("LOCALAPPDATA", nonexistent("hl-test-localappdata"))
        .env("HOME", nonexistent("hl-test-home"));
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

/// The directory of the fixtures that the tests read, as an absolute path,
/// so that a test that changes the working directory finds it too.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli/fixtures")
}

/// The fixture `name` in `fixtures_dir()`: `sample.toml` or `sample.rs`.
/// The tests read these rather than the project's own files, whose
/// content changes.
pub fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
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

/// Removes SGR escape sequences (`ESC [ ... m`). Panics on one without
/// its `m`, which `hl` never writes, rather than dropping the rest.
pub fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\x1b[") {
            let end = bytes[i..]
                .iter()
                .position(|&b| b == b'm')
                .unwrap_or_else(|| panic!("unterminated CSI at byte {i}"));
            i += end + 1;
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

/// Asserts that `cmd` colors `sample.toml` with `expected` escapes only.
pub fn assert_depth(cmd: &mut Command, expected: &str) {
    let other = if expected == TRUECOLOR {
        ANSI256
    } else {
        TRUECOLOR
    };
    cmd.args(["--color", "always"])
        .arg(fixture("sample.toml"))
        .assert()
        .success()
        .stdout(predicate::str::contains(expected).and(predicate::str::contains(other).not()));
}
