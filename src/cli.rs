use std::path::PathBuf;

use clap::{Command, CommandFactory, FromArgMatches, Parser, ValueEnum};
use toml::Value;

use crate::{
    color::{self, Depth},
    config::Config,
    highlight::{DEFAULT_THEME, Limits, SyntaxMapping},
    lines::{LineRange, Numbering, Visibility},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PagingWhen {
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
#[expect(clippy::struct_excessive_bools, reason = "clap flags")]
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

    /// Print only lines N to M of each input (N:M, N:, :M or N). Earlier
    /// lines are still read so that highlighting stays correct.
    #[arg(short = 'r', long, value_name = "N:M", value_parser = parse_range)]
    pub line_range: Option<LineRange>,

    /// Print a `==> FILE <==` header before each input.
    #[arg(long)]
    pub header: bool,

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

    /// Use LANG for files matching the glob PATTERN (e.g. '*.conf:INI'). A
    /// pattern containing '/' matches the full path. Can be repeated; the
    /// last matching mapping wins.
    #[arg(long, value_name = "PATTERN:LANG", value_parser = SyntaxMapping::parse)]
    pub map_syntax: Vec<SyntaxMapping>,

    /// Mappings from the config file, checked before those of --map-syntax.
    #[arg(skip)]
    config_mappings: Vec<SyntaxMapping>,

    /// Color theme.
    #[arg(long, env = "HL_THEME", default_value = DEFAULT_THEME)]
    pub theme: String,

    /// Stop highlighting inputs larger than SIZE (K/M/G suffixes; 0 = no limit).
    #[arg(long, env = "HL_MAX_SIZE", value_name = "SIZE", default_value = "512K", value_parser = parse_size)]
    pub max_size: u64,

    /// Stop highlighting at the first line longer than LEN bytes (K/M/G suffixes; 0 = no limit).
    #[arg(long, env = "HL_MAX_LINE_LENGTH", value_name = "LEN", default_value = "16K", value_parser = parse_size)]
    pub max_line_length: u64,

    /// When to send output through a pager ($PAGER, or `less`): `auto` pages
    /// only when the output does not fit on one screen.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = PagingWhen::Never, overrides_with = "page")]
    paging: PagingWhen,

    /// Same as --paging=always.
    #[arg(short = 'p', overrides_with = "paging")]
    page: bool,

    /// When to use colors.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,

    /// Color depth: `auto` uses truecolor only when the terminal advertises it
    /// (`COLORTERM`, `TERM`, `WT_SESSION`) and 256 colors otherwise.
    #[arg(long, env = "HL_COLOR_DEPTH", value_enum, value_name = "DEPTH", default_value_t = ColorDepth::Auto)]
    pub color_depth: ColorDepth,

    /// List available themes and exit.
    #[arg(long)]
    pub list_themes: bool,

    /// List supported languages and exit.
    #[arg(long)]
    pub list_languages: bool,

    /// Cache the custom syntaxes in the config directory so that they load
    /// fast, and exit. Run it again after changing them.
    #[arg(long, conflicts_with = "clear_cache")]
    pub build_cache: bool,

    /// Remove the cache made by --build-cache and exit.
    #[arg(long)]
    pub clear_cache: bool,
}

/// Options whose default can be set in the config file, by their long name.
/// Flags are left out because the command line could not turn them off.
const CONFIG_KEYS: [&str; 6] = [
    "theme",
    "color",
    "color-depth",
    "paging",
    "max-size",
    "max-line-length",
];

/// The config key listing `--map-syntax` mappings. Unlike the keys above, it
/// adds to the command-line values instead of supplying a default.
const MAP_SYNTAX_KEY: &str = "map-syntax";

/// Parses the command line, with the config file supplying the defaults.
///
/// Config values replace the built-in defaults of their options, so clap
/// itself gives the precedence: command line, environment, config file,
/// built-in default.
pub fn parse() -> Args {
    let (cmd, config_mappings) = match Config::load() {
        Some(config) => apply_config(Args::command(), &config),
        None => (Args::command(), Vec::new()),
    };
    let mut args = Args::from_arg_matches_mut(&mut cmd.get_matches()).unwrap_or_else(|e| e.exit());
    args.config_mappings = config_mappings;
    args
}

/// Sets the config values as the defaults of `cmd` and returns the config
/// mappings. A bad entry warns and is ignored.
fn apply_config(mut cmd: Command, config: &Config) -> (Command, Vec<SyntaxMapping>) {
    // Checks each value on its own, without the environment, so that a bad
    // value is reported as coming from the config file.
    let probe = Args::command().mut_args(|arg| arg.env(None));
    let mut mappings = Vec::new();
    for (key, value) in &config.entries {
        let is_option = CONFIG_KEYS.contains(&key.as_str());
        let value = match value {
            Value::String(s) if is_option => s.clone(),
            Value::Integer(n) if is_option => n.to_string(),
            Value::Array(items) if key == MAP_SYNTAX_KEY && items.iter().all(Value::is_str) => {
                let items = items.iter().filter_map(Value::as_str);
                mappings.extend(items.filter_map(|item| parse_mapping(config, item)));
                continue;
            }
            _ if key == MAP_SYNTAX_KEY => {
                config.warn(&format!("'{key}' must be an array of strings, ignoring it"));
                continue;
            }
            _ if is_option => {
                config.warn(&format!(
                    "'{key}' must be a string or an integer, ignoring it"
                ));
                continue;
            }
            _ => {
                config.warn(&format!("unknown key '{key}', ignoring it"));
                continue;
            }
        };
        let option = format!("--{key}={value}");
        if probe.clone().try_get_matches_from(["hl", &option]).is_err() {
            config.warn(&format!("invalid value '{value}' for '{key}', ignoring it"));
            continue;
        }
        cmd = cmd.mut_arg(arg_id(key), |arg| arg.default_value(value));
    }
    (cmd, mappings)
}

/// A `map-syntax` entry of the config file, or `None` with a warning.
fn parse_mapping(config: &Config, item: &str) -> Option<SyntaxMapping> {
    match SyntaxMapping::parse(item) {
        Ok(mut mapping) => {
            mapping.origin = Some(config.path.clone());
            Some(mapping)
        }
        Err(e) => {
            config.warn(&format!(
                "invalid {MAP_SYNTAX_KEY} entry '{item}': {e}, ignoring it"
            ));
            None
        }
    }
}

/// The clap id of the option with the long name `key`.
fn arg_id(key: &str) -> String {
    key.replace('-', "_")
}

impl Args {
    /// The syntax mappings from the config file, then the command line, so
    /// that the last matching one wins.
    pub fn mappings(&self) -> impl Iterator<Item = &SyntaxMapping> {
        self.config_mappings.iter().chain(&self.map_syntax)
    }

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

    /// When to page, with -p expanded.
    pub fn paging(&self) -> PagingWhen {
        if self.page {
            PagingWhen::Always
        } else {
            self.paging
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

/// Parses a line range such as `120:160`, `120:`, `:160` or `120`.
fn parse_range(s: &str) -> Result<LineRange, String> {
    let invalid = || format!("'{s}' is not a line range (e.g. 120:160)");
    let line = |n: &str| match n.parse() {
        Ok(0) => Err("line numbers start at 1".to_string()),
        Ok(n) => Ok(n),
        Err(_) => Err(invalid()),
    };
    let (start, end) = match s.split_once(':') {
        Some(("", "")) => return Err(invalid()),
        Some(bounds) => bounds,
        None => (s, s),
    };
    let start = if start.is_empty() { 1 } else { line(start)? };
    let end = if end.is_empty() {
        None
    } else {
        Some(line(end)?)
    };
    if end.is_some_and(|end| end < start) {
        return Err(format!("'{s}' ends before it starts"));
    }
    Ok(LineRange { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_consistent() {
        Args::command().debug_assert();
    }

    /// `mut_arg` panics on an unknown id, so every config key must name an
    /// option.
    #[test]
    fn config_keys_name_options() {
        let cmd = Args::command();
        for key in CONFIG_KEYS {
            let id = arg_id(key);
            assert!(
                cmd.get_arguments().any(|arg| arg.get_id() == id.as_str()),
                "no option for config key '{key}'"
            );
        }
    }
}
