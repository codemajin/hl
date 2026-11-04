//! Integration tests: they run the `hl` binary and check what reaches it
//! from the outside (options, environment variables, the config file) and
//! what comes out (stdout, stderr, the exit status). The decision tables
//! behind them are covered by the unit tests in `src/`.
//!
//! One file per feature; the helpers they share are in `common.rs`.

mod common;

mod basic;
mod cat_compat;
mod color_depth;
mod config;
mod custom_assets;
mod encoding;
mod escape;
mod header;
mod highlight_line;
mod info;
mod limits;
mod mapping;
mod pager;
mod range;
mod streams;
mod syntax_fixes;
mod theme;
