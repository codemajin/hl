//! The color depth: that the environment and `--color-depth` reach the
//! detection. The detection table itself is covered by the unit tests in
//! `src/color.rs`.

use std::fs;

use predicates::prelude::*;

use crate::common::{ANSI256, TRUECOLOR, assert_depth, colored, has_escape, hl, strip_ansi};

#[test]
fn uses_truecolor_when_colorterm_advertises_it() {
    assert_depth(hl().env("COLORTERM", "truecolor"), TRUECOLOR);
}

#[test]
fn uses_truecolor_for_direct_term_and_windows_terminal() {
    assert_depth(hl().env("TERM", "xterm-direct"), TRUECOLOR);
    assert_depth(hl().env("WT_SESSION", "1"), TRUECOLOR);
    // An empty WT_SESSION counts as unset.
    assert_depth(hl().env("WT_SESSION", ""), ANSI256);
}

#[test]
fn color_depth_option_overrides_detection() {
    assert_depth(
        hl().env("COLORTERM", "truecolor")
            .args(["--color-depth", "256"]),
        ANSI256,
    );
    assert_depth(hl().args(["--color-depth", "truecolor"]), TRUECOLOR);
    assert_depth(hl().args(["--color-depth", "24bit"]), TRUECOLOR);
    assert_depth(
        hl().env("COLORTERM", "truecolor")
            .args(["--color-depth", "auto"]),
        TRUECOLOR,
    );
}

#[test]
fn color_depth_can_be_set_from_env() {
    assert_depth(
        hl().env("COLORTERM", "truecolor")
            .env("HL_COLOR_DEPTH", "256"),
        ANSI256,
    );
    assert_depth(
        hl().env("HL_COLOR_DEPTH", "256")
            .args(["--color-depth", "truecolor"]),
        TRUECOLOR,
    );
}

#[test]
fn rejects_unknown_color_depth() {
    hl().args(["--color-depth", "16", "Cargo.toml"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn keeps_the_text_with_256_colors() {
    let content = fs::read("src/main.rs").unwrap();
    let out = colored(hl().args(["--color-depth", "256", "src/main.rs"]));
    assert!(has_escape(&out));
    assert_eq!(strip_ansi(&out), content);
}
