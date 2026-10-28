use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::{
    color::{self, Depth},
    highlight::Limits,
    lines::{Numbering, Visibility},
};

pub const DEFAULT_THEME: &str = "base16-ocean.dark";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorDepth {
    Auto,
    #[value(name = "truecolor", alias = "24bit")]
    TrueColor,
    #[value(name = "256")]
    Ansi256,
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

    /// Number non-empty output lines; overrides -n.
    #[arg(short = 'b', long)]
    pub number_nonblank: bool,

    /// Suppress repeated empty output lines.
    #[arg(short = 's', long)]
    pub squeeze_blank: bool,

    /// Equivalent to -vET.
    #[arg(short = 'A', long)]
    pub show_all: bool,

    /// Equivalent to -vE.
    #[arg(short = 'e')]
    pub show_nonprinting_ends: bool,

    /// Display $ at end of each line.
    #[arg(short = 'E', long)]
    pub show_ends: bool,

    /// Equivalent to -vT.
    #[arg(short = 't')]
    pub show_nonprinting_tabs: bool,

    /// Display TAB characters as ^I.
    #[arg(short = 'T', long)]
    pub show_tabs: bool,

    /// Use ^ and M- notation, except for LFD and TAB.
    #[arg(short = 'v', long)]
    pub show_nonprinting: bool,

    /// Ignored (accepted for cat compatibility).
    #[arg(short = 'u', hide = true)]
    pub unbuffered: bool,

    /// Language to use instead of auto-detection (name or extension, e.g. rust, py).
    #[arg(short = 'l', long, value_name = "LANG")]
    pub language: Option<String>,

    /// Color theme.
    #[arg(long, env = "HL_THEME", default_value = DEFAULT_THEME)]
    pub theme: String,

    /// Stop highlighting inputs larger than SIZE (K/M/G suffixes; 0 = no limit).
    #[arg(long, env = "HL_MAX_SIZE", value_name = "SIZE", default_value = "512K", value_parser = parse_size)]
    pub max_size: u64,

    /// Stop highlighting at the first line longer than LEN bytes (K/M/G suffixes; 0 = no limit).
    #[arg(long, env = "HL_MAX_LINE_LENGTH", value_name = "LEN", default_value = "16K", value_parser = parse_size)]
    pub max_line_length: u64,

    /// Send output through a pager ($PAGER, or `less -RFX`).
    #[arg(short = 'p', long)]
    pub paging: bool,

    /// When to use colors.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,

    /// Color depth: `auto` uses truecolor only when the terminal advertises it
    /// (COLORTERM, TERM, WT_SESSION) and 256 colors otherwise.
    #[arg(long, env = "HL_COLOR_DEPTH", value_enum, value_name = "DEPTH", default_value_t = ColorDepth::Auto)]
    pub color_depth: ColorDepth,

    /// List available themes and exit.
    #[arg(long)]
    pub list_themes: bool,

    /// List supported languages and exit.
    #[arg(long)]
    pub list_languages: bool,
}

impl Args {
    pub fn numbering(&self) -> Numbering {
        if self.number_nonblank {
            Numbering::NonBlank
        } else if self.number {
            Numbering::All
        } else {
            Numbering::None
        }
    }

    /// The `cat -v` / `-E` / `-T` options, with -A, -e and -t expanded.
    pub fn visibility(&self) -> Visibility {
        Visibility {
            nonprinting: self.show_nonprinting
                || self.show_all
                || self.show_nonprinting_ends
                || self.show_nonprinting_tabs,
            ends: self.show_ends || self.show_all || self.show_nonprinting_ends,
            tabs: self.show_tabs || self.show_all || self.show_nonprinting_tabs,
        }
    }

    /// The color depth, with `auto` resolved from the environment.
    pub fn depth(&self) -> Depth {
        match self.color_depth {
            ColorDepth::Auto => color::detect_from_env(),
            ColorDepth::TrueColor => Depth::TrueColor,
            ColorDepth::Ansi256 => Depth::Ansi256,
        }
    }

    /// The highlighting limits, with 0 meaning no limit.
    pub fn limits(&self) -> Limits {
        Limits {
            max_size: (self.max_size > 0).then_some(self.max_size),
            max_line: (self.max_line_length > 0).then_some(self.max_line_length),
        }
    }
}

/// Parses a byte count such as `512`, `16K`, `2m` or `1G` (powers of 1024).
fn parse_size(s: &str) -> Result<u64, String> {
    let (digits, unit) = match s.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => s.split_at(i),
        None => (s, ""),
    };
    let shift = match unit.to_ascii_lowercase().as_str() {
        "" => 0,
        "k" => 10,
        "m" => 20,
        "g" => 30,
        _ => return Err(format!("invalid unit '{unit}' (use K, M or G)")),
    };
    let n: u64 = digits
        .parse()
        .map_err(|_| format!("'{s}' is not a size (e.g. 512K)"))?;
    n.checked_mul(1 << shift)
        .ok_or_else(|| format!("'{s}' is too large"))
}
