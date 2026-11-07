//! Custom syntaxes and themes from the config directory, and the syntax
//! cache.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{colored, files, fixture, has_escape, hl, stdout, strip_ansi, tempdir};

/// A config directory for custom syntaxes and themes, with its own cache.
struct Assets {
    dir: TempDir,
}

const HLTEST_SYNTAX: &str = "%YAML 1.2
---
name: HlTest
file_extensions: [hltest]
scope: source.hltest
contexts:
  main:
    - match: \\bkw\\b
      scope: keyword.control.hltest
";

const RED_THEME: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>name</key><string>Red</string>
<key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#FF0000</string></dict></dict>
</array></dict></plist>
"#;

const NOT_CACHED: &str = "custom syntaxes are not cached";

impl Assets {
    fn new() -> Self {
        Self { dir: tempdir() }
    }

    /// The directory for the input files.
    fn dir(&self) -> &Path {
        self.dir.path()
    }

    fn path(&self, sub: &str) -> PathBuf {
        self.dir.path().join(sub)
    }

    fn syntax(&self, name: &str, content: &str) -> &Self {
        let name = format!("config/hl/syntaxes/{name}.sublime-syntax");
        files(self.dir(), &[(name.as_str(), content)]);
        self
    }

    fn theme(&self, name: &str, content: &str) -> &Self {
        let name = format!("config/hl/themes/{name}.tmTheme");
        files(self.dir(), &[(name.as_str(), content)]);
        self
    }

    fn cache(&self) -> PathBuf {
        self.path("cache/hl/syntaxes.bin")
    }

    /// The path under the temporary directory made of `parts`, as `hl` shows
    /// it in a message: joined one at a time, so with backslashes on Windows.
    fn shown(&self, parts: &[&str]) -> String {
        let path = parts
            .iter()
            .fold(self.dir().to_path_buf(), |p, part| p.join(part));
        path.display().to_string()
    }

    /// The cache as `hl` shows it.
    fn shown_cache(&self) -> String {
        self.shown(&["cache", "hl", "syntaxes.bin"])
    }

    /// `hl` with these assets, on a terminal that shows truecolor, so that
    /// the theme colors are printed as they are.
    fn hl(&self) -> Command {
        let mut cmd = hl();
        cmd.env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("COLORTERM", "truecolor");
        cmd
    }
}

#[test]
fn highlights_with_a_custom_syntax() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    let inputs = files(
        assets.dir(),
        &[("a.hltest", "kw other\n"), ("a.txt", "kw other\n")],
    );
    let (file, plain) = (&inputs[0], &inputs[1]);
    let detected = colored(assets.hl().arg(file));
    assert_ne!(detected, colored(assets.hl().arg(plain)));
    assert_eq!(
        detected,
        colored(assets.hl().args(["-l", "hltest"]).arg(plain))
    );
    assert_eq!(
        detected,
        colored(assets.hl().args(["-l", "HlTest"]).arg(plain))
    );
    assets
        .hl()
        .arg("--list-languages")
        .assert()
        .success()
        .stdout(predicate::str::contains("HlTest: hltest\n"));
}

#[test]
fn custom_syntax_replaces_a_bundled_one() {
    let assets = Assets::new();
    assets.syntax(
        "MyRust",
        &HLTEST_SYNTAX
            .replace("[hltest]", "[rs]")
            .replace("HlTest", "MyRust"),
    );
    let file = files(assets.dir(), &[("a.rs", "kw fn main() {}\n")]);
    let detected = colored(assets.hl().args(&file));
    assert_eq!(
        detected,
        colored(assets.hl().args(["-l", "MyRust"]).args(&file))
    );
    assert_ne!(
        detected,
        colored(assets.hl().args(["-l", "Rust"]).args(&file))
    );
}

#[test]
fn skips_custom_syntaxes_that_cannot_be_loaded() {
    let assets = Assets::new();
    assets
        .syntax("HlTest", HLTEST_SYNTAX)
        .syntax("BadYaml", "name: x\n  bad: [\n")
        .syntax(
            "BadRegex",
            &HLTEST_SYNTAX.replace("\\bkw\\b", "'(unclosed'"),
        );
    let file = files(assets.dir(), &[("a.hltest", "kw other\n")]);
    let expected = colored(assets.hl().args(["-l", "hltest"]).args(&file));
    assets
        .hl()
        .args(["--color", "always"])
        .args(&file)
        .assert()
        .success()
        .stderr(
            predicate::str::contains("BadYaml.sublime-syntax: cannot load syntax, skipping it")
                .and(predicate::str::contains(
                    "BadRegex.sublime-syntax: cannot load syntax, skipping it",
                )),
        )
        .stdout(expected);
}

#[test]
fn prints_the_rest_after_a_syntax_error() {
    let assets = Assets::new();
    assets.syntax(
        "Foo",
        &HLTEST_SYNTAX.replace("[hltest]", "[foo]").replace(
            "scope: keyword.control.hltest",
            "push: scope:source.nonexistent",
        ),
    );
    let file = files(assets.dir(), &[("a.foo", "a\nkw\nb\n")]);
    let out = assets
        .hl()
        .args(["--color", "always"])
        .args(&file)
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(strip_ansi(&out.stdout), b"a\nkw\nb\n");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(
        stderr.matches("printing the rest as plain text").count(),
        1,
        "{stderr}"
    );
}

#[test]
fn keeps_the_line_that_ends_highlighting_after_a_syntax_error() {
    let assets = Assets::new();
    assets.syntax(
        "Foo",
        &HLTEST_SYNTAX.replace("[hltest]", "[foo]").replace(
            "scope: keyword.control.hltest",
            "push: scope:source.nonexistent",
        ),
    );
    // The syntax fails on `kw`, which is still waiting to be printed when
    // a later line goes over a limit or is not UTF-8.
    let long_line = format!("{}\n", "y".repeat(100));
    let mut not_utf8 = format!("{}\nkw\n", "a".repeat(9000)).into_bytes();
    not_utf8.extend_from_slice(b"\xffLINE\nb\n");
    let cases: [(&[&str], Vec<u8>); 3] = [
        (&["--max-size=12"], b"a\nkw\nb\nLINE4\nLINE5\n".to_vec()),
        (
            &["--max-line-length=10"],
            format!("a\nkw\n{long_line}b\n").into_bytes(),
        ),
        (&[], not_utf8),
    ];
    for (args, input) in cases {
        let out = assets
            .hl()
            .args(["--color", "always", "-l", "foo"])
            .args(args)
            .write_stdin(input.clone())
            .assert()
            .success()
            .get_output()
            .clone();
        assert_eq!(strip_ansi(&out.stdout), input, "{args:?}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert_eq!(
            stderr.matches("printing the rest as plain text").count(),
            1,
            "{args:?}: {stderr}"
        );
    }
}

/// A syntax that hands `x` back and forth between two contexts without
/// consuming it, which makes syntect loop forever.
const LOOPING_SYNTAX: &str = "%YAML 1.2
---
name: Loop
file_extensions: [loop]
scope: source.loop
contexts:
  main:
    - match: (?=x)
      set: other
  other:
    - match: (?=x)
      set: main
";

#[test]
fn prints_the_rest_plain_when_highlighting_hangs() {
    let assets = Assets::new();
    assets.syntax("Loop", LOOPING_SYNTAX);
    let file = files(assets.dir(), &[("a.loop", "a\nx\nb\n")]);
    let out = assets
        .hl()
        .args(["--color", "always", "-n"])
        .args(&file)
        .timeout(Duration::from_secs(20))
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = out.stdout;
    let plain = b"     2\tx\n     3\tb\n";
    assert!(stdout.ends_with(plain), "{stdout:?}");
    assert!(
        has_escape(&stdout[..stdout.len() - plain.len()]),
        "{stdout:?}"
    );
    assert_eq!(strip_ansi(&stdout), b"     1\ta\n     2\tx\n     3\tb\n");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains(
            "cannot highlight: a line took longer than 3s, printing the rest and any later inputs \
             as plain text"
        ),
        "{stderr}"
    );
}

#[test]
fn stops_highlighting_after_the_first_stall() {
    let assets = Assets::new();
    assets.syntax("Loop", LOOPING_SYNTAX);
    let inputs = [("a.loop", "a\nx\n"), ("b.loop", "x\n"), ("c.loop", "x\n")];
    let files = files(assets.dir(), &inputs);
    let run = |files: &[PathBuf]| {
        let start = Instant::now();
        let out = assets
            .hl()
            .args(["--color", "always"])
            .args(files)
            .timeout(Duration::from_secs(30))
            .assert()
            .success()
            .get_output()
            .clone();
        (out, start.elapsed())
    };
    let (_, one) = run(&files[..1]);
    let (out, three) = run(&files);
    // Each input getting stuck would take two more stalls of 3 seconds than
    // the first input alone; loading the syntaxes takes the same in both.
    assert!(
        three < one + Duration::from_secs(3),
        "three inputs took {three:?}, one {one:?}"
    );
    let stdout = out.stdout;
    assert!(stdout.ends_with(b"x\nx\nx\n"), "{stdout:?}");
    assert!(has_escape(&stdout[..stdout.len() - 6]), "{stdout:?}");
    assert_eq!(strip_ansi(&stdout), b"a\nx\nx\nx\n");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(stderr.matches("took longer than").count(), 1, "{stderr}");
    assert!(stderr.contains("a.loop: cannot highlight"), "{stderr}");
}

#[test]
fn caches_custom_syntaxes() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    let file = files(assets.dir(), &[("a.hltest", "kw other\n")]);
    let uncached = assets
        .hl()
        .args(["--color", "always"])
        .args(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains(NOT_CACHED))
        .get_output()
        .stdout
        .clone();

    assets
        .hl()
        .arg("--build-cache")
        .assert()
        .success()
        .stdout(predicate::str::contains("cached 1 of 1 custom syntaxes"));
    assert!(assets.cache().is_file());
    assets
        .hl()
        .args(["--color", "always"])
        .args(&file)
        .assert()
        .success()
        .stderr("")
        .stdout(uncached);

    // A changed syntax makes the cache stale until it is built again.
    assets.syntax(
        "Other",
        &HLTEST_SYNTAX
            .replace("hltest", "other")
            .replace("HlTest", "Other"),
    );
    assets
        .hl()
        .arg("--list-languages")
        .assert()
        .success()
        .stderr(predicate::str::contains(NOT_CACHED))
        .stdout(predicate::str::contains("Other: other\n"));
    assets.hl().arg("--build-cache").assert().success();
    assets
        .hl()
        .arg("--list-languages")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::contains("Other: other\n"));

    assets
        .hl()
        .arg("--clear-cache")
        .assert()
        .success()
        .stderr("");
    assert!(!assets.cache().exists());
    assets
        .hl()
        .arg("--clear-cache")
        .assert()
        .success()
        .stderr("");
}

#[test]
fn merges_again_when_the_cache_is_broken() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    let file = files(assets.dir(), &[("a.hltest", "kw other\n")]);
    let expected = colored(assets.hl().args(&file));
    for garbage in [&b"not a cache"[..], &[0xff; 64]] {
        files(assets.dir(), &[("cache/hl/syntaxes.bin", garbage)]);
        assets
            .hl()
            .args(["--color", "always"])
            .args(&file)
            .assert()
            .success()
            .stderr(predicate::str::contains(NOT_CACHED))
            .stdout(expected.clone());
    }
}

#[test]
fn build_cache_needs_a_cache_directory() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    assets
        .hl()
        .env_remove("XDG_CACHE_HOME")
        .env_remove("LOCALAPPDATA")
        .env_remove("HOME")
        .arg("--build-cache")
        .assert()
        .code(1)
        .stderr(if cfg!(windows) {
            "hl: no cache directory (set LOCALAPPDATA)\n"
        } else {
            "hl: no cache directory (set HOME)\n"
        });
}

#[test]
fn build_cache_without_custom_syntaxes_removes_the_cache() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    assets.hl().arg("--build-cache").assert().success();
    fs::remove_dir_all(assets.path("config/hl/syntaxes")).unwrap();
    assets
        .hl()
        .arg("--build-cache")
        .assert()
        .success()
        .stdout(predicate::str::contains("no custom syntaxes"));
    assert!(!assets.cache().exists());
}

#[test]
fn bundled_syntaxes_need_no_cache() {
    let assets = Assets::new();
    assets
        .hl()
        .args(["--color", "always"])
        .arg(fixture("sample.toml"))
        .assert()
        .success()
        .stderr("");
}

#[test]
fn uses_a_custom_theme() {
    let assets = Assets::new();
    assets.theme("Red", RED_THEME);
    let file = files(assets.dir(), &[("a.txt", "text\n")]);
    let out = colored(assets.hl().args(["--theme", "Red"]).args(&file));
    assert!(
        String::from_utf8(out.clone())
            .unwrap()
            .contains("\x1b[38;2;255;0;0m")
    );
    assert_eq!(
        out,
        colored(assets.hl().args(["--theme", "red"]).args(&file))
    );
    assets
        .hl()
        .arg("--list-themes")
        .assert()
        .success()
        .stdout(predicate::str::contains("\nRed\n"));
}

#[test]
fn custom_theme_replaces_a_bundled_one() {
    let assets = Assets::new();
    assets.theme("Nord", RED_THEME);
    let file = files(assets.dir(), &[("a.txt", "text\n")]);
    let out = colored(assets.hl().args(["--theme", "Nord"]).args(&file));
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("\x1b[38;2;255;0;0m")
    );
    let listed = stdout(assets.hl().arg("--list-themes"));
    let listed = String::from_utf8(listed).unwrap();
    assert_eq!(listed.lines().filter(|&n| n == "Nord").count(), 1);
}

#[test]
fn skips_custom_themes_that_cannot_be_loaded() {
    let assets = Assets::new();
    assets.theme("Broken", "not a theme\n");
    let file = files(assets.dir(), &[("a.txt", "text\n")]);
    assets
        .hl()
        .arg("--list-themes")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Broken.tmTheme: cannot load theme, skipping it",
        ))
        .stdout(predicate::str::contains("Broken").not());
    let expected = colored(assets.hl().args(&file));
    assets
        .hl()
        .args(["--color", "always", "--theme", "Broken"])
        .args(&file)
        .assert()
        .success()
        .stderr(
            predicate::str::contains("Broken.tmTheme: cannot load theme").and(
                predicate::str::contains("falling back to base16-ocean.dark"),
            ),
        )
        .stdout(expected);
}

#[test]
fn build_cache_reports_the_syntaxes_it_skipped() {
    let assets = Assets::new();
    assets
        .syntax("HlTest", HLTEST_SYNTAX)
        .syntax("Broken", "not: [a syntax");
    assets
        .hl()
        .arg("--build-cache")
        .assert()
        .success()
        .stdout(format!(
            "cached 1 of 2 custom syntaxes in {}\n",
            assets.shown_cache()
        ))
        .stderr(predicate::str::contains(
            "Broken.sublime-syntax: cannot load syntax, skipping it: ",
        ));
    assert!(assets.cache().is_file());
}

#[test]
fn warns_when_the_syntax_directory_is_a_file() {
    let assets = Assets::new();
    files(assets.dir(), &[("config/hl/syntaxes", "not a directory")]);
    let dir = assets.shown(&["config", "hl", "syntaxes"]);
    let expected = colored(Assets::new().hl().arg(fixture("sample.toml")));
    assets
        .hl()
        .args(["--color", "always"])
        .arg(fixture("sample.toml"))
        .assert()
        .success()
        .stderr(predicate::str::starts_with(format!("hl: {dir}: ")))
        .stdout(expected);
}

#[test]
fn build_cache_fails_when_the_cache_directory_is_a_file() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    files(assets.dir(), &[("cache/hl", "not a directory")]);
    assets
        .hl()
        .arg("--build-cache")
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            predicate::str::starts_with(format!("hl: {}: ", assets.shown_cache()))
                .and(predicate::str::contains("os error").not()),
        );
}

/// The cache, made a directory so that it can be neither replaced nor
/// removed.
fn cache_as_a_directory(assets: &Assets) {
    files(assets.dir(), &[("cache/hl/syntaxes.bin/keep", "")]);
}

/// The names of the files in the cache directory.
fn cache_dir_entries(assets: &Assets) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(assets.path("cache/hl"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn build_cache_leaves_no_temporary_file() {
    let assets = Assets::new();
    assets.syntax("HlTest", HLTEST_SYNTAX);
    cache_as_a_directory(&assets);
    assets
        .hl()
        .arg("--build-cache")
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::starts_with(format!(
            "hl: {}: ",
            assets.shown_cache()
        )));
    // The syntaxes were written to `syntaxes.bin.tmp.<pid>`, which could not
    // be renamed and is removed.
    assert_eq!(cache_dir_entries(&assets), ["syntaxes.bin"]);
}

#[test]
fn clear_cache_fails_when_the_cache_is_a_directory() {
    let assets = Assets::new();
    cache_as_a_directory(&assets);
    let prefix = format!("hl: {}: ", assets.shown_cache());
    let assert = assets.hl().arg("--clear-cache").assert().code(1).stdout("");
    if cfg!(target_os = "linux") {
        assert.stderr(format!("{prefix}Is a directory\n"));
    } else {
        // macOS and Windows have messages of their own.
        assert.stderr(
            predicate::str::starts_with(prefix).and(predicate::str::contains("os error").not()),
        );
    }
}
