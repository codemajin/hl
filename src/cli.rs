use std::path::PathBuf;

use clap::{Parser, ValueEnum};

pub const DEFAULT_THEME: &str = "base16-ocean.dark";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

/// cat with syntax highlighting.
#[derive(Debug, Parser)]
#[command(name = "hl", version, about)]
pub struct Args {
    /// Files to print. Reads stdin when omitted or when FILE is "-".
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Number all output lines (continuous across files, like `cat -n`).
    #[arg(short = 'n', long)]
    pub number: bool,

    /// Language to use instead of auto-detection (name or extension, e.g. rust, py).
    #[arg(short = 'l', long, value_name = "LANG")]
    pub language: Option<String>,

    /// Color theme.
    #[arg(short = 't', long, env = "HL_THEME", default_value = DEFAULT_THEME)]
    pub theme: String,

    /// Send output through a pager ($PAGER, or `less -RFX`).
    #[arg(short = 'p', long)]
    pub paging: bool,

    /// When to use colors.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,

    /// List available themes and exit.
    #[arg(long)]
    pub list_themes: bool,

    /// List supported languages and exit.
    #[arg(long)]
    pub list_languages: bool,
}
