use std::{
    env,
    error::Error as _,
    ffi::{OsStr, OsString},
    fmt::Write as _,
    io::{self, Write},
    path::PathBuf,
};

use clap::{
    Command, CommandFactory, FromArgMatches, Parser, ValueEnum,
    error::{ContextKind, ContextValue, ErrorKind},
    parser::ValueSource,
};
use clap_complete::Shell;
use toml::Value;

use crate::{
    assets::DEFAULT_THEME,
    color::Depth,
    config::Config,
    diag::escape_controls,
    highlight::{Limits, Origin, SyntaxMapping},
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

impl PagingWhen {
    /// Whether to start a pager. Without a terminal there is nobody to page
    /// for, as with `--color=auto`.
    pub fn should_page(self, tty: bool) -> bool {
        self != Self::Never && tty
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StripAnsi {
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
    #[arg(short = 'n', long, overrides_with = "no_number")]
    number: bool,

    /// Cancel -n, e.g. when it is set in the config file.
    #[arg(long, overrides_with = "number", hide_short_help = true)]
    no_number: bool,

    /// Number non-empty output lines; overrides -n.
    #[arg(short = 'b', long, overrides_with = "no_number_nonblank")]
    number_nonblank: bool,

    /// Cancel -b, e.g. when it is set in the config file.
    #[arg(long, overrides_with = "number_nonblank", hide_short_help = true)]
    no_number_nonblank: bool,

    /// Suppress repeated empty output lines.
    #[arg(short = 's', long, overrides_with = "no_squeeze_blank")]
    pub squeeze_blank: bool,

    /// Cancel -s, e.g. when it is set in the config file.
    #[arg(long, overrides_with = "squeeze_blank", hide_short_help = true)]
    no_squeeze_blank: bool,

    /// Print only lines N to M of each input (N:M, N:, :M or N). Earlier
    /// lines are still read so that highlighting stays correct.
    #[arg(short = 'r', long, value_name = "N:M", value_parser = parse_range)]
    pub line_range: Option<LineRange>,

    /// Show lines N to M of each input with a background color, in the same
    /// forms as --line-range. Can be repeated. Only applies to colored output.
    #[arg(short = 'H', long, value_name = "N:M", value_parser = parse_range)]
    pub highlight_line: Vec<LineRange>,

    /// Print a `==> FILE <==` header before each input.
    #[arg(long, overrides_with = "no_header")]
    pub header: bool,

    /// Cancel --header, e.g. when it is set in the config file.
    #[arg(long, overrides_with = "header", hide_short_help = true)]
    no_header: bool,

    /// Equivalent to -vET.
    #[arg(short = 'A', long)]
    show_all: bool,

    /// Equivalent to -vE.
    #[arg(short = 'e')]
    show_nonprinting_ends: bool,

    /// Display $ at end of each line.
    #[arg(short = 'E', long)]
    show_ends: bool,

    /// Equivalent to -vT.
    #[arg(short = 't')]
    show_nonprinting_tabs: bool,

    /// Display TAB characters as ^I.
    #[arg(short = 'T', long)]
    show_tabs: bool,

    /// Use ^ and M- notation, except for LFD and TAB.
    #[arg(short = 'v', long)]
    show_nonprinting: bool,

    /// Ignored (accepted for cat compatibility).
    #[arg(short = 'u', hide = true)]
    pub unbuffered: bool,

    /// Language to use instead of auto-detection (name or extension, e.g. rust, py).
    #[arg(short = 'l', long, value_name = "LANG")]
    pub language: Option<String>,

    /// Use LANG for files matching the glob PATTERN (e.g. '*.conf:INI'). A
    /// pattern containing '/' matches the full path. Can be repeated; the
    /// last matching mapping wins.
    #[arg(long, value_name = "PATTERN:LANG", value_parser = parse_cli_mapping)]
    map_syntax: Vec<SyntaxMapping>,

    /// Mappings from the config file, checked before those of --map-syntax.
    #[arg(skip)]
    config_mappings: Vec<SyntaxMapping>,

    /// Color theme.
    #[arg(long, env = "HL_THEME", default_value = DEFAULT_THEME)]
    pub theme: String,

    /// Stop highlighting inputs larger than SIZE (K/M/G suffixes; 0 = no limit).
    #[arg(long, env = "HL_MAX_SIZE", value_name = "SIZE", default_value = "512K", value_parser = parse_size)]
    max_size: u64,

    /// Stop highlighting at the first line longer than LEN bytes (K/M/G suffixes; 0 = no limit).
    #[arg(long, env = "HL_MAX_LINE_LENGTH", value_name = "LEN", default_value = "16K", value_parser = parse_size)]
    max_line_length: u64,

    /// When to send output through a pager ($PAGER, or `less`): `auto` pages
    /// only when the output does not fit on one screen. A bare `less` in
    /// $PAGER gets -RX (-RFX with `auto`) unless $LESS is set.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = PagingWhen::Never, overrides_with = "page")]
    paging: PagingWhen,

    /// Same as --paging=always.
    #[arg(short = 'p', overrides_with = "paging")]
    page: bool,

    /// When to use colors.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = ColorWhen::Auto)]
    color: ColorWhen,

    /// Color depth: `auto` uses truecolor only when the terminal advertises it
    /// (`COLORTERM`, `TERM`, `WT_SESSION`) and 256 colors otherwise.
    #[arg(long, env = "HL_COLOR_DEPTH", value_enum, value_name = "DEPTH", default_value_t = ColorDepth::Auto)]
    color_depth: ColorDepth,

    /// When to remove escape sequences (colors, OSC, ...) from the input:
    /// `auto` removes them only when the output is colored, `always` also
    /// when it is not.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = StripAnsi::Auto)]
    strip_ansi: StripAnsi,

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

    /// Print the completion script for SHELL and exit.
    #[arg(long, value_enum, value_name = "SHELL")]
    pub completions: Option<Shell>,

    /// Print the man page (roff) and exit.
    #[arg(long)]
    pub man: bool,
}

/// Options whose default can be set in the config file, by their long name.
const CONFIG_KEYS: [&str; 7] = [
    "theme",
    "color",
    "color-depth",
    "strip-ansi",
    "paging",
    "max-size",
    "max-line-length",
];

/// A flag that the config file can turn on with `true`. Flags are applied
/// after parsing rather than as clap defaults: clap fills in the default of
/// a flag removed by `overrides_with` again, so `--no-*` could not turn
/// them off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigFlag {
    Number,
    NumberNonblank,
    SqueezeBlank,
    Header,
}

impl ConfigFlag {
    /// Every flag. A flag missing here can't be set in the config file.
    const ALL: [Self; 4] = [
        Self::Number,
        Self::NumberNonblank,
        Self::SqueezeBlank,
        Self::Header,
    ];

    /// The config key, which is the long name of the flag.
    fn key(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::NumberNonblank => "number-nonblank",
            Self::SqueezeBlank => "squeeze-blank",
            Self::Header => "header",
        }
    }

    /// The flag whose config key is `key`.
    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|flag| flag.key() == key)
    }
}

/// The config key listing `--map-syntax` mappings. Unlike the other keys, it
/// adds to the command-line values instead of supplying a default.
const MAP_SYNTAX_KEY: &str = "map-syntax";

/// The kinds of config keys, which differ in the values they take and in
/// how they are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    /// A flag, set to `true` or `false`.
    Flag(ConfigFlag),
    /// One of [`CONFIG_KEYS`], whose string or integer value becomes the
    /// default of the option.
    Option,
    /// [`MAP_SYNTAX_KEY`], an array of mappings.
    MapSyntax,
}

/// The kind of the config key `key`, or `None` if it is unknown.
fn classify(key: &str) -> Option<Key> {
    if let Some(flag) = ConfigFlag::from_key(key) {
        Some(Key::Flag(flag))
    } else if CONFIG_KEYS.contains(&key) {
        Some(Key::Option)
    } else if key == MAP_SYNTAX_KEY {
        Some(Key::MapSyntax)
    } else {
        None
    }
}

/// What the config file adds to the command line besides option defaults.
#[derive(Default)]
struct ConfigExtras {
    /// Flags set to `true`.
    flags: Vec<ConfigFlag>,
    mappings: Vec<SyntaxMapping>,
}

/// Parses the command line, with `config` supplying the defaults.
///
/// Config values replace the built-in defaults of their options, so clap
/// itself gives the precedence: command line, environment, config file,
/// built-in default.
///
/// An invalid value is a usage error (exit status 2). When it comes from an
/// environment variable, the error names the variable instead of the
/// option, which the user did not write.
pub fn parse_with(config: Option<&Config>) -> Args {
    let (cmd, extras) = match config {
        Some(config) => apply_config(Args::command(), config),
        None => (Args::command(), ConfigExtras::default()),
    };
    let mut matches = cmd
        .clone()
        .try_get_matches()
        .unwrap_or_else(|e| env_error(&cmd, e).exit());
    let mut args = Args::from_arg_matches_mut(&mut matches).unwrap_or_else(|e| e.exit());
    args.apply_extras(extras);
    args
}

/// Parses `argv` (without the program name) like the command line, but
/// ignores the `HL_*` variables, so that the developer's environment can't
/// change what the unit tests see.
#[cfg(test)]
pub fn parse_without_env(argv: &[&str]) -> Args {
    let matches = without_env(Args::command())
        .try_get_matches_from(["hl"].iter().chain(argv))
        .unwrap();
    Args::from_arg_matches(&matches).unwrap()
}

/// `cmd` without the environment variables of its options.
fn without_env(cmd: Command) -> Command {
    cmd.mut_args(|arg| arg.env(None))
}

/// The error to report for `err`, an error from parsing the command line
/// with `cmd`. If the value of an environment variable caused it, this is an
/// error naming the variable; otherwise it is `err` itself.
fn env_error(cmd: &Command, err: clap::Error) -> clap::Error {
    if !matches!(
        err.kind(),
        ErrorKind::ValueValidation | ErrorKind::InvalidValue
    ) {
        return err;
    }
    // The config values and the built-in defaults are valid, so if the
    // command line parses without the environment, a variable is to blame.
    let probe = without_env(cmd.clone());
    let Ok(matches) = probe.clone().try_get_matches() else {
        return err;
    };
    for arg in cmd.get_arguments() {
        let (Some(name), Some(long)) = (arg.get_env(), arg.get_long()) else {
            continue;
        };
        // A variable is used only when the command line lacks the option.
        if matches.value_source(arg.get_id().as_str()) == Some(ValueSource::CommandLine) {
            continue;
        }
        let Some(value) = env::var_os(name) else {
            continue;
        };
        let mut option = OsString::from(format!("--{long}="));
        option.push(&value);
        if let Err(e) = probe
            .clone()
            .try_get_matches_from([OsString::from("hl"), option])
        {
            let message = format!("{}\n", env_message(name, &value, &e));
            return clap::Error::raw(e.kind(), message).with_cmd(cmd);
        }
    }
    err
}

/// The message for the invalid `value` of the environment variable `name`,
/// with the reason or the possible values from `err`, the error for the
/// same value given to the option.
fn env_message(name: &OsStr, value: &OsStr, err: &clap::Error) -> String {
    let value = value.to_string_lossy();
    let mut message = format!(
        "invalid value '{}' for {}",
        escape_controls(&value),
        name.to_string_lossy()
    );
    if let Some(reason) = err.source() {
        let _ = write!(message, ": {reason}");
    }
    if let Some(ContextValue::Strings(values)) = err.get(ContextKind::ValidValue) {
        let _ = write!(message, " (possible values: {})", values.join(", "));
    }
    message
}

/// Sets the config values as the defaults of `cmd` and returns the flags
/// and mappings of the config. A bad entry warns and is ignored.
fn apply_config(mut cmd: Command, config: &Config) -> (Command, ConfigExtras) {
    // Checks each value on its own, without the environment, so that a bad
    // value is reported as coming from the config file.
    let probe = without_env(Args::command());
    let mut extras = ConfigExtras::default();
    for (key, value) in &config.entries {
        match (classify(key), value) {
            (Some(Key::Flag(flag)), Value::Boolean(true)) => extras.flags.push(flag),
            (Some(Key::Flag(_)), Value::Boolean(false)) => {}
            (Some(Key::Flag(_)), _) => {
                config.warn(&format!("'{key}' must be true or false, ignoring it"));
            }
            (Some(Key::Option), Value::String(s)) => {
                cmd = set_default(cmd, &probe, config, key, s);
            }
            (Some(Key::Option), Value::Integer(n)) => {
                cmd = set_default(cmd, &probe, config, key, &n.to_string());
            }
            (Some(Key::Option), _) => {
                config.warn(&format!(
                    "'{key}' must be a string or an integer, ignoring it"
                ));
            }
            (Some(Key::MapSyntax), Value::Array(items)) if items.iter().all(Value::is_str) => {
                let items = items.iter().filter_map(Value::as_str);
                extras
                    .mappings
                    .extend(items.filter_map(|item| parse_mapping(config, item)));
            }
            (Some(Key::MapSyntax), _) => {
                config.warn(&format!("'{key}' must be an array of strings, ignoring it"));
            }
            (None, _) => config.warn(&format!("unknown key '{key}', ignoring it")),
        }
    }
    (cmd, extras)
}

/// `cmd` with `value` as the default of the option named by the config key
/// `key`, or `cmd` as is with a warning if `probe`, the command without the
/// environment, rejects the value.
fn set_default(cmd: Command, probe: &Command, config: &Config, key: &str, value: &str) -> Command {
    let option = format!("--{key}={value}");
    if probe.clone().try_get_matches_from(["hl", &option]).is_err() {
        config.warn(&format!("invalid value '{value}' for '{key}', ignoring it"));
        return cmd;
    }
    cmd.mut_arg(arg_id(key), |arg| arg.default_value(value.to_owned()))
}

/// A `--map-syntax` value.
fn parse_cli_mapping(s: &str) -> Result<SyntaxMapping, String> {
    SyntaxMapping::parse(s, Origin::CommandLine)
}

/// A `map-syntax` entry of the config file, or `None` with a warning.
fn parse_mapping(config: &Config, item: &str) -> Option<SyntaxMapping> {
    match SyntaxMapping::parse(item, Origin::ConfigFile(config.path.clone())) {
        Ok(mapping) => Some(mapping),
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
    /// Applies what the config file adds after parsing.
    fn apply_extras(&mut self, extras: ConfigExtras) {
        for flag in extras.flags {
            self.apply_config_flag(flag);
        }
        self.config_mappings = extras.mappings;
    }

    /// Turns on `flag`, unless its `--no-*` option was given.
    fn apply_config_flag(&mut self, flag: ConfigFlag) {
        let (on, off) = match flag {
            ConfigFlag::Number => (&mut self.number, self.no_number),
            ConfigFlag::NumberNonblank => (&mut self.number_nonblank, self.no_number_nonblank),
            ConfigFlag::SqueezeBlank => (&mut self.squeeze_blank, self.no_squeeze_blank),
            ConfigFlag::Header => (&mut self.header, self.no_header),
        };
        *on |= !off;
    }

    /// Whether to color the output, given whether it goes to a terminal.
    /// `cat -v` style output is never highlighted, so it matches cat exactly.
    pub fn use_color(&self, tty: bool) -> bool {
        !self.visibility().any()
            && match self.color {
                ColorWhen::Always => true,
                ColorWhen::Never => false,
                ColorWhen::Auto => tty,
            }
    }

    /// Whether to remove the escape sequences of the input, given whether
    /// the output is colored.
    pub fn strip_escapes(&self, color: bool) -> bool {
        match self.strip_ansi {
            StripAnsi::Auto => color,
            StripAnsi::Always => true,
            StripAnsi::Never => false,
        }
    }

    /// Whether the syntaxes are needed. They are also loaded to report
    /// unknown languages given on the command line, even when nothing gets
    /// highlighted.
    pub fn needs_syntaxes(&self, color: bool) -> bool {
        color || self.language.is_some() || !self.map_syntax.is_empty()
    }

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

    /// The color depth, with `auto` resolved by `detected` (main passes
    /// `color::detect_from_env`, so that only main reads the environment).
    pub fn depth(&self, detected: impl FnOnce() -> Depth) -> Depth {
        match self.color_depth {
            ColorDepth::Auto => detected(),
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

/// Writes the completion script for `shell`. Like [`print_man`], it uses
/// the built-in defaults, so the output does not depend on the config file.
pub fn print_completions(shell: Shell, out: &mut dyn Write) -> io::Result<()> {
    // `generate` panics when it cannot write, so the script is written here.
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut Args::command(), "hl", &mut script);
    out.write_all(&script)
}

/// Writes the man page in roff.
pub fn print_man(out: &mut dyn Write) -> io::Result<()> {
    clap_mangen::Man::new(Args::command()).render(out)
}

/// Parses a byte count such as `512`, `16K`, `2m` or `1G` (powers of 1024):
/// a whole number of ASCII digits, optionally followed by one of K, M or G
/// in either case. Signs, fractions, spaces and other units are rejected.
///
/// The messages do not repeat the value, as clap prints it before them.
fn parse_size(s: &str) -> Result<u64, String> {
    const NOT_A_SIZE: &str =
        "not a size: use a whole number with an optional K, M or G suffix (e.g. 512K)";
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (digits, unit) = s.split_at(split);
    let shift = match unit.to_ascii_lowercase().as_str() {
        _ if digits.is_empty() => return Err(NOT_A_SIZE.to_string()),
        "" => 0,
        "k" => 10,
        "m" => 20,
        "g" => 30,
        _ => return Err(NOT_A_SIZE.to_string()),
    };
    // Only digits are left, so the parse can fail only by overflowing.
    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(1 << shift))
        .ok_or_else(|| format!("too large (at most {}G)", u64::MAX >> 30))
}

/// Parses a line range such as `120:160`, `120:`, `:160` or `120`. The line
/// numbers are written in ASCII digits only, so signs such as `+3` and
/// spaces are rejected.
fn parse_range(s: &str) -> Result<LineRange, String> {
    let invalid = || format!("'{s}' is not a line range (e.g. 120:160)");
    let line = |n: &str| match n.parse() {
        // `u64::from_str` takes a leading `+`, which is not a line number.
        _ if !n.bytes().all(|b| b.is_ascii_digit()) => Err(invalid()),
        Ok(0) => Err("line numbers start at 1".to_string()),
        Ok(n) => Ok(n),
        Err(_) => Err(invalid()),
    };
    let (start, end) = match s.split_once(':') {
        Some(("", "")) => return Err(invalid()),
        Some(bounds) => bounds,
        None if s.is_empty() => return Err(invalid()),
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

    /// Parses `argv`. Like `args_with`, it ignores the `HL_*` variables, so
    /// that the developer's environment can't change the result; the
    /// integration tests check how they are read.
    fn args(argv: &[&str]) -> Args {
        parse_without_env(argv)
    }

    /// Parses `argv` with `config` applied, as `parse_with` does, but
    /// without the environment.
    fn args_with(config: &Config, argv: &[&str]) -> Args {
        let (cmd, extras) = apply_config(Args::command(), config);
        let matches = without_env(cmd)
            .try_get_matches_from(["hl"].iter().chain(argv))
            .unwrap();
        let mut parsed = Args::from_arg_matches(&matches).unwrap();
        parsed.apply_extras(extras);
        parsed
    }

    fn config(entries: &str) -> Config {
        Config {
            path: PathBuf::from("config.toml"),
            entries: entries
                .parse::<toml::Table>()
                .unwrap()
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn build_cache_and_clear_cache_conflict() {
        for argv in [
            ["hl", "--build-cache", "--clear-cache"],
            ["hl", "--clear-cache", "--build-cache"],
        ] {
            let err = without_env(Args::command())
                .try_get_matches_from(argv)
                .err()
                .unwrap();
            assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{argv:?}");
        }
    }

    #[test]
    fn color_follows_the_terminal_by_default() {
        assert!(args(&[]).use_color(true));
        assert!(!args(&[]).use_color(false));
        assert!(args(&["--color=always"]).use_color(false));
        assert!(!args(&["--color=never"]).use_color(true));
    }

    #[test]
    fn auto_depth_comes_from_the_detector() {
        for detected in [Depth::TrueColor, Depth::Ansi256] {
            assert_eq!(args(&[]).depth(|| detected), detected);
            assert_eq!(args(&["--color-depth=auto"]).depth(|| detected), detected);
        }
    }

    #[test]
    fn explicit_depth_ignores_the_detector() {
        let detected = || -> Depth { panic!("the detector must not be called") };
        assert_eq!(
            args(&["--color-depth=truecolor"]).depth(detected),
            Depth::TrueColor
        );
        assert_eq!(args(&["--color-depth=256"]).depth(detected), Depth::Ansi256);
    }

    #[test]
    fn nonprinting_notation_is_never_colored() {
        for flag in ["-v", "-A", "-e", "-E", "-t", "-T"] {
            assert!(!args(&["--color=always", flag]).use_color(true), "{flag}");
        }
    }

    #[test]
    fn strips_escapes_only_when_colored_by_default() {
        assert!(args(&[]).strip_escapes(true));
        assert!(!args(&[]).strip_escapes(false));
        assert!(args(&["--strip-ansi=always"]).strip_escapes(false));
        assert!(!args(&["--strip-ansi=never"]).strip_escapes(true));
    }

    #[test]
    fn syntaxes_are_needed_to_check_languages() {
        assert!(args(&[]).needs_syntaxes(true));
        assert!(!args(&[]).needs_syntaxes(false));
        assert!(args(&["-l", "rust"]).needs_syntaxes(false));
        assert!(args(&["--map-syntax", "*.conf:INI"]).needs_syntaxes(false));
    }

    #[test]
    fn paging_needs_a_terminal() {
        assert!(PagingWhen::Always.should_page(true));
        assert!(PagingWhen::Auto.should_page(true));
        assert!(!PagingWhen::Never.should_page(true));
        assert!(!PagingWhen::Always.should_page(false));
        assert_eq!(args(&["-p"]).paging(), PagingWhen::Always);
    }

    #[test]
    fn last_of_p_and_paging_wins() {
        assert_eq!(args(&["-p", "--paging=never"]).paging(), PagingWhen::Never);
        assert_eq!(args(&["--paging=never", "-p"]).paging(), PagingWhen::Always);
        assert_eq!(args(&["-p", "--paging=auto"]).paging(), PagingWhen::Auto);
    }

    #[test]
    fn config_sets_paging() {
        let auto = config(r#"paging = "auto""#);
        assert_eq!(args_with(&auto, &[]).paging(), PagingWhen::Auto);
        assert_eq!(
            args_with(&auto, &["--paging=never"]).paging(),
            PagingWhen::Never
        );
        assert_eq!(args_with(&auto, &["-p"]).paging(), PagingWhen::Always);
    }

    #[test]
    fn highlight_line_can_be_repeated() {
        let ranges = args(&["-H", "3", "--highlight-line", "5:6"]).highlight_line;
        assert_eq!(
            ranges,
            [
                LineRange {
                    start: 3,
                    end: Some(3),
                },
                LineRange {
                    start: 5,
                    end: Some(6),
                },
            ]
        );
    }

    #[test]
    fn config_sets_defaults() {
        let config = config(r#"color = "never""#);
        assert_eq!(args_with(&config, &[]).color, ColorWhen::Never);
        assert_eq!(
            args_with(&config, &["--color=always"]).color,
            ColorWhen::Always
        );
    }

    #[test]
    fn bad_config_entries_are_ignored() {
        let config = config(
            r#"
            color = "sometimes"
            paging = 1
            pager = "less"
            header = "yes"
            "#,
        );
        let args = args_with(&config, &[]);
        assert_eq!(args.color, ColorWhen::Auto);
        assert_eq!(args.paging(), PagingWhen::Never);
        assert!(!args.header);
    }

    #[test]
    fn config_turns_flags_on() {
        let on = config(
            r"
            number = true
            squeeze-blank = true
            header = true
            ",
        );
        let args = args_with(&on, &[]);
        assert_eq!(args.numbering(), Numbering::All);
        assert!(args.squeeze_blank);
        assert!(args.header);

        let off = config("number = false\nheader = false");
        let args = args_with(&off, &[]);
        assert_eq!(args.numbering(), Numbering::None);
        assert!(!args.header);
    }

    #[test]
    fn no_flags_cancel_the_config() {
        let config = config(
            r"
            number = true
            squeeze-blank = true
            header = true
            ",
        );
        let cancelled = args_with(
            &config,
            &["--no-number", "--no-squeeze-blank", "--no-header"],
        );
        assert_eq!(cancelled.numbering(), Numbering::None);
        assert!(!cancelled.squeeze_blank);
        assert!(!cancelled.header);
        // The last of a flag and its --no-* option wins.
        assert_eq!(
            args_with(&config, &["--no-number", "-n"]).numbering(),
            Numbering::All
        );
        assert_eq!(args(&["-n", "--no-number"]).numbering(), Numbering::None);
    }

    #[test]
    fn number_nonblank_wins_across_config_and_command_line() {
        let nonblank = config("number-nonblank = true");
        assert_eq!(
            args_with(&nonblank, &["-n"]).numbering(),
            Numbering::NonBlank
        );
        assert_eq!(
            args_with(&nonblank, &["--no-number-nonblank", "-n"]).numbering(),
            Numbering::All
        );
        let number = config("number = true");
        assert_eq!(args_with(&number, &["-b"]).numbering(), Numbering::NonBlank);
        let both = config("number = true\nnumber-nonblank = true");
        assert_eq!(args_with(&both, &[]).numbering(), Numbering::NonBlank);
        assert_eq!(
            args_with(&both, &["--no-number-nonblank"]).numbering(),
            Numbering::All
        );
    }

    #[test]
    fn config_mappings_come_first() {
        let config = config(r#"map-syntax = ["*.conf:INI", "bad"]"#);
        let args = args_with(&config, &["--map-syntax", "*.conf:TOML"]);
        let mappings: Vec<_> = args
            .mappings()
            .map(|m| (m.spec.as_str(), m.origin().clone()))
            .collect();
        assert_eq!(
            mappings,
            [
                ("*.conf:INI", Origin::ConfigFile("config.toml".into())),
                ("*.conf:TOML", Origin::CommandLine),
            ]
        );
    }

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

    /// Every config flag is recognized by its key and turns on its own
    /// field, and only that one. A new flag fails to compile in the match
    /// below until it gets a field, and then fails the length check until
    /// it is added to `ConfigFlag::ALL`.
    #[test]
    fn each_config_flag_turns_its_field_on() {
        for flag in ConfigFlag::ALL {
            assert_eq!(ConfigFlag::from_key(flag.key()), Some(flag));
            let parsed = args_with(&config(&format!("{} = true", flag.key())), &[]);
            let fields = [
                parsed.number,
                parsed.number_nonblank,
                parsed.squeeze_blank,
                parsed.header,
            ];
            let on = match flag {
                ConfigFlag::Number => 0,
                ConfigFlag::NumberNonblank => 1,
                ConfigFlag::SqueezeBlank => 2,
                ConfigFlag::Header => 3,
            };
            assert_eq!(fields.len(), ConfigFlag::ALL.len());
            for (i, field) in fields.into_iter().enumerate() {
                assert_eq!(field, i == on, "{}: field {i}", flag.key());
            }
        }
    }

    #[test]
    fn keys_are_classified() {
        assert_eq!(classify("header"), Some(Key::Flag(ConfigFlag::Header)));
        assert_eq!(classify("max-size"), Some(Key::Option));
        assert_eq!(classify("map-syntax"), Some(Key::MapSyntax));
        assert_eq!(classify("pager"), None);
        // Keys are the long names, not the clap ids.
        assert_eq!(classify("max_size"), None);
    }

    /// The expected result of a parser: the value, or a part of the message.
    #[derive(Debug)]
    enum Expect<T> {
        Value(T),
        Error(&'static str),
    }

    fn check<T: PartialEq + std::fmt::Debug>(
        parse: fn(&str) -> Result<T, String>,
        cases: &[(&str, Expect<T>)],
    ) {
        for (input, expected) in cases {
            match (parse(input), expected) {
                (Ok(value), Expect::Value(want)) => assert_eq!(&value, want, "{input:?}"),
                (Err(message), Expect::Error(want)) => {
                    assert!(message.contains(want), "{input:?}: {message:?}");
                }
                (result, _) => panic!("{input:?}: expected {expected:?}, got {result:?}"),
            }
        }
    }

    #[test]
    fn parses_sizes() {
        use Expect::{Error, Value};
        const NOT_A_SIZE: &str =
            "not a size: use a whole number with an optional K, M or G suffix (e.g. 512K)";
        let max_g = u64::MAX >> 30;
        check(
            parse_size,
            &[
                ("0", Value(0)),
                ("1024", Value(1024)),
                ("1k", Value(1024)),
                ("1K", Value(1024)),
                ("2m", Value(2 << 20)),
                ("1G", Value(1 << 30)),
                ("0K", Value(0)),
                ("18446744073709551615", Value(u64::MAX)),
                ("17179869183G", Value(max_g << 30)),
                ("17179869184G", Error("too large (at most 17179869183G)")),
                (
                    "18446744073709551616",
                    Error("too large (at most 17179869183G)"),
                ),
                ("", Error(NOT_A_SIZE)),
                ("K", Error(NOT_A_SIZE)),
                ("1KB", Error(NOT_A_SIZE)),
                ("1T", Error(NOT_A_SIZE)),
                ("1.5K", Error(NOT_A_SIZE)),
                ("-1", Error(NOT_A_SIZE)),
                ("+1", Error(NOT_A_SIZE)),
                (" 1K", Error(NOT_A_SIZE)),
                ("1K ", Error(NOT_A_SIZE)),
                ("1 K", Error(NOT_A_SIZE)),
            ],
        );
    }

    #[test]
    fn parses_line_ranges() {
        use Expect::{Error, Value};
        const START_AT_1: &str = "line numbers start at 1";
        let range = |start, end| Value(LineRange { start, end });
        check(
            parse_range,
            &[
                ("3", range(3, Some(3))),
                ("2:5", range(2, Some(5))),
                ("2:2", range(2, Some(2))),
                ("2:", range(2, None)),
                (":5", range(1, Some(5))),
                ("18446744073709551615", range(u64::MAX, Some(u64::MAX))),
                ("0", Error(START_AT_1)),
                (":0", Error(START_AT_1)),
                ("2:0", Error(START_AT_1)),
                ("0:", Error(START_AT_1)),
                ("5:3", Error("'5:3' ends before it starts")),
                ("", Error("'' is not a line range (e.g. 120:160)")),
                (":", Error("':' is not a line range")),
                ("a:b", Error("'a:b' is not a line range")),
                ("1-3", Error("'1-3' is not a line range")),
                ("1:2:3", Error("'1:2:3' is not a line range")),
                ("+3", Error("'+3' is not a line range")),
                ("+2:+3", Error("'+2:+3' is not a line range")),
                ("-1", Error("'-1' is not a line range")),
                (" 3", Error("' 3' is not a line range")),
                ("3 ", Error("'3 ' is not a line range")),
                (
                    "18446744073709551616",
                    Error("'18446744073709551616' is not a line range"),
                ),
            ],
        );
    }
}
