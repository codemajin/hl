use std::{
    io::{self, Write},
    path::PathBuf,
};

use clap::{Command, CommandFactory, FromArgMatches, Parser, ValueEnum};
use clap_complete::Shell;
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
    pub number: bool,

    /// Cancel -n, e.g. when it is set in the config file.
    #[arg(long, overrides_with = "number", hide_short_help = true)]
    no_number: bool,

    /// Number non-empty output lines; overrides -n.
    #[arg(short = 'b', long, overrides_with = "no_number_nonblank")]
    pub number_nonblank: bool,

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

    /// When to remove escape sequences (colors, OSC, ...) from the input:
    /// `auto` removes them only when the output is colored, `always` also
    /// when it is not.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = StripAnsi::Auto)]
    pub strip_ansi: StripAnsi,

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

/// Flags that the config file can turn on with `true`. They are applied
/// after parsing rather than as clap defaults: clap fills in the default of
/// a flag removed by `overrides_with` again, so `--no-*` could not turn
/// them off.
const CONFIG_FLAGS: [&str; 4] = ["number", "number-nonblank", "squeeze-blank", "header"];

/// The config key listing `--map-syntax` mappings. Unlike the keys above, it
/// adds to the command-line values instead of supplying a default.
const MAP_SYNTAX_KEY: &str = "map-syntax";

/// What the config file adds to the command line besides option defaults.
#[derive(Default)]
struct ConfigExtras {
    /// Flags set to `true`.
    flags: Vec<&'static str>,
    mappings: Vec<SyntaxMapping>,
}

/// Parses the command line, with `config` supplying the defaults.
///
/// Config values replace the built-in defaults of their options, so clap
/// itself gives the precedence: command line, environment, config file,
/// built-in default.
pub fn parse_with(config: Option<&Config>) -> Args {
    let (cmd, extras) = match config {
        Some(config) => apply_config(Args::command(), config),
        None => (Args::command(), ConfigExtras::default()),
    };
    let mut args = Args::from_arg_matches_mut(&mut cmd.get_matches()).unwrap_or_else(|e| e.exit());
    args.apply_extras(extras);
    args
}

/// Sets the config values as the defaults of `cmd` and returns the flags
/// and mappings of the config. A bad entry warns and is ignored.
fn apply_config(mut cmd: Command, config: &Config) -> (Command, ConfigExtras) {
    // Checks each value on its own, without the environment, so that a bad
    // value is reported as coming from the config file.
    let probe = Args::command().mut_args(|arg| arg.env(None));
    let mut extras = ConfigExtras::default();
    for (key, value) in &config.entries {
        if let Some(&flag) = CONFIG_FLAGS.iter().find(|&&flag| flag == key) {
            match value {
                Value::Boolean(true) => extras.flags.push(flag),
                Value::Boolean(false) => {}
                _ => config.warn(&format!("'{key}' must be true or false, ignoring it")),
            }
            continue;
        }
        let is_option = CONFIG_KEYS.contains(&key.as_str());
        let value = match value {
            Value::String(s) if is_option => s.clone(),
            Value::Integer(n) if is_option => n.to_string(),
            Value::Array(items) if key == MAP_SYNTAX_KEY && items.iter().all(Value::is_str) => {
                let items = items.iter().filter_map(Value::as_str);
                extras
                    .mappings
                    .extend(items.filter_map(|item| parse_mapping(config, item)));
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
    (cmd, extras)
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
    /// Applies what the config file adds after parsing.
    fn apply_extras(&mut self, extras: ConfigExtras) {
        for flag in extras.flags {
            self.apply_config_flag(flag);
        }
        self.config_mappings = extras.mappings;
    }

    /// Turns on the flag named `key` in [`CONFIG_FLAGS`], unless its `--no-*`
    /// option was given.
    fn apply_config_flag(&mut self, key: &str) {
        let (on, off) = match key {
            "number" => (&mut self.number, self.no_number),
            "number-nonblank" => (&mut self.number_nonblank, self.no_number_nonblank),
            "squeeze-blank" => (&mut self.squeeze_blank, self.no_squeeze_blank),
            "header" => (&mut self.header, self.no_header),
            _ => unreachable!("unknown config flag '{key}'"),
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
    use std::path::Path;

    use super::*;

    fn args(argv: &[&str]) -> Args {
        Args::try_parse_from(["hl"].iter().chain(argv)).unwrap()
    }

    /// Parses `argv` with `config` applied, as `parse_with` does.
    fn args_with(config: &Config, argv: &[&str]) -> Args {
        let (cmd, extras) = apply_config(Args::command(), config);
        let matches = cmd.try_get_matches_from(["hl"].iter().chain(argv)).unwrap();
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
    fn color_follows_the_terminal_by_default() {
        assert!(args(&[]).use_color(true));
        assert!(!args(&[]).use_color(false));
        assert!(args(&["--color=always"]).use_color(false));
        assert!(!args(&["--color=never"]).use_color(true));
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
            r#"
            number = true
            squeeze-blank = true
            header = true
            "#,
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
            r#"
            number = true
            squeeze-blank = true
            header = true
            "#,
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
            .map(|m| (m.spec.as_str(), m.origin.as_deref()))
            .collect();
        assert_eq!(
            mappings,
            [
                ("*.conf:INI", Some(Path::new("config.toml"))),
                ("*.conf:TOML", None),
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

    /// `apply_config_flag` panics on an unknown key, so it must handle every
    /// config flag.
    #[test]
    fn config_flags_are_handled() {
        for key in CONFIG_FLAGS {
            let mut parsed = args(&[]);
            parsed.apply_config_flag(key);
        }
    }
}
