//! The pager.

use std::{fs, path::Path};

use predicates::prelude::*;

use crate::common::{hl, tempdir};

#[test]
fn never_starts_a_pager_without_a_terminal() {
    let dir = tempdir();
    let marker = dir.path().join("paged");
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let content = fs::read(&input).unwrap();
    for opts in [&["-p"][..], &["--paging=always"], &["--paging=auto"]] {
        // The marker is relative to the working directory, so that PAGER
        // holds no path that could contain spaces. There is no `touch` on
        // Windows, but the test holds there too: a pager that hl tried to
        // start would fail, and so would `hl`.
        hl().args(opts)
            .arg(&input)
            .current_dir(dir.path())
            .env("PAGER", "touch paged")
            .assert()
            .success()
            .stdout(content.clone());
        assert!(!marker.exists(), "pager started with {opts:?}");
    }
}

/// Without a terminal both orders print the input as it is; which one wins
/// is checked by the unit tests in cli.rs.
#[test]
fn accepts_p_together_with_paging() {
    let content = fs::read("Cargo.toml").unwrap();
    for opts in [["-p", "--paging=never"], ["--paging=never", "-p"]] {
        hl().args(opts)
            .arg("Cargo.toml")
            .assert()
            .success()
            .stdout(content.clone());
    }
}

#[test]
fn rejects_unknown_paging_mode() {
    hl().args(["--paging=sometimes", "Cargo.toml"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'sometimes'"));
}
