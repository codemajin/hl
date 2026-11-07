mod assets;
mod cli;
mod color;
mod config;
mod console;
mod diag;
mod encoding;
mod escape;
mod highlight;
mod lines;
mod pager;
mod print;
mod stdout;
#[cfg(test)]
mod test_support;

use std::{
    env,
    fs::File,
    io::{self, BufWriter, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use anyhow::{Result, anyhow, bail};
use assets::{CacheReport, DEFAULT_THEME, ThemeFallback};
use cli::{Args, PagingWhen};
use config::{Config, Dirs};
use diag::{Warning, describe, describe_error, escape_controls};
use highlight::{GiveUp, Highlighter, Limits, Origin, Syntax, SyntaxMapping, find_language};
use lines::{LineHighlight, LineState};
use pager::{Pager, SpawnError};
use print::{Headers, Input, PrintOutcome, Printer, Problem, TrackedWriter, is_stdin, print_all};
use syntect::{highlighting::Theme, parsing::SyntaxSet};

fn main() -> ExitCode {
    let dirs = Dirs::from_env();
    let config = Config::load(&dirs, show_warning);
    let args = cli::parse_with(config.as_ref(), show_warning);
    match run(&args, &dirs, &stdout::check()) {
        Ok(code) => code,
        // A closed pipe (`hl ... | head`) is a normal way to stop reading.
        Err(err) if err.downcast_ref::<io::Error>().is_some_and(is_broken_pipe) => {
            ExitCode::SUCCESS
        }
        Err(err) => {
            // The message can contain a path or a name from the user.
            diag::note(escape_controls(&describe_error(&err)));
            ExitCode::FAILURE
        }
    }
}

/// Runs `hl`, with custom syntaxes, themes and the cache in `dirs`.
/// `stdout` tells whether stdout is open, and is checked only by the
/// commands that write to it.
fn run(args: &Args, dirs: &Dirs, stdout: &io::Result<()>) -> Result<ExitCode> {
    if let Some(code) = run_info_command(args, dirs, stdout)? {
        return Ok(code);
    }

    let tty = io::stdout().is_terminal();
    let color = args.use_color(tty);
    let syntaxes = args
        .needs_syntaxes(color)
        .then(|| Arc::new(assets::syntaxes(dirs, show_warning)));
    let printer = build_printer(args, color, syntaxes, || load_theme(&args.theme, dirs))?;
    let lines = build_line_state(args, printer.highlighter.as_ref());
    print_inputs(args, &printer, lines, color, tty, stdout)
}

/// Runs the command that prints something other than the inputs
/// (`--completions`, `--list-themes`, ...), if one is given.
fn run_info_command(args: &Args, dirs: &Dirs, stdout: &io::Result<()>) -> Result<Option<ExitCode>> {
    let code = if let Some(shell) = args.completions {
        print_and_exit(stdout, |out| cli::print_completions(shell, out))?
    } else if args.man {
        print_and_exit(stdout, |out| cli::print_man(out))?
    } else if args.build_cache {
        let report = assets::build_cache(dirs, show_warning)?;
        print_and_exit(stdout, |out| print_cache_report(&report, out))?
    } else if args.clear_cache {
        assets::clear_cache(dirs)?;
        ExitCode::SUCCESS
    } else if args.list_themes {
        print_and_exit(stdout, |out| list_themes(dirs, out))?
    } else if args.list_languages {
        print_and_exit(stdout, |out| list_languages(dirs, out))?
    } else {
        return Ok(None);
    };
    Ok(Some(code))
}

/// The printer for `args`. `syntaxes` are those loaded when
/// `Args::needs_syntaxes` asks for them, and `theme` is called only when the
/// output is highlighted.
fn build_printer(
    args: &Args,
    color: bool,
    syntaxes: Option<Arc<SyntaxSet>>,
    theme: impl FnOnce() -> Theme,
) -> Result<Printer> {
    let (forced, mappings) = match &syntaxes {
        Some(syntaxes) => {
            let forced = args
                .language
                .as_deref()
                .map(|lang| {
                    find_language(syntaxes, lang)
                        .ok_or_else(|| anyhow!("unknown language '{lang}' (see --list-languages)"))
                })
                .transpose()?;
            let mappings = resolve_mappings(syntaxes, args.mappings(), show_warning)?;
            (forced, mappings)
        }
        None => (None, Vec::new()),
    };
    let highlighter = syntaxes.filter(|_| color).map(|syntaxes| {
        Highlighter::new(
            syntaxes,
            theme(),
            args.depth(color::detect_from_env),
            mappings,
        )
    });
    Ok(Printer {
        highlighter,
        forced,
        vis: args.visibility(),
        limits: args.limits(),
        strip: args.strip_escapes(color),
    })
}

/// Looks up the language of each syntax mapping. An unknown language is an
/// error on the command line, but only a warning in the config file, passed
/// to `report`, which never stops `hl` from printing.
fn resolve_mappings<'m>(
    syntaxes: &Arc<SyntaxSet>,
    mappings: impl IntoIterator<Item = &'m SyntaxMapping>,
    mut report: impl FnMut(Warning),
) -> Result<Vec<(SyntaxMapping, Syntax)>> {
    let mut resolved = Vec::new();
    for mapping in mappings {
        let Some(syntax) = find_language(syntaxes, &mapping.language) else {
            let msg = format!(
                "unknown language '{}' in map-syntax '{}'",
                mapping.language, mapping.spec
            );
            match mapping.origin() {
                Origin::ConfigFile(path) => report(Warning::File {
                    path: path.clone(),
                    message: format!("{msg}, ignoring it"),
                }),
                Origin::CommandLine => bail!("{msg} (see --list-languages)"),
            }
            continue;
        };
        resolved.push((mapping.clone(), syntax));
    }
    Ok(resolved)
}

/// The theme called `name`, or the default theme with a warning.
fn load_theme(name: &str, dirs: &Dirs) -> Theme {
    let themes = load_themes(dirs);
    themes.get(name).unwrap_or_else(|fallback| {
        warn_theme_fallback(name, &fallback);
        themes.default_theme()
    })
}

fn warn_theme_fallback(name: &str, fallback: &ThemeFallback) {
    match fallback {
        ThemeFallback::Unknown => diag::note(format_args!(
            "unknown theme '{}', falling back to {DEFAULT_THEME} (see --list-themes)",
            escape_controls(name)
        )),
        ThemeFallback::Broken { path, error } => {
            warn_bad_theme(path, error);
            diag::note(format_args!("falling back to {DEFAULT_THEME}"));
        }
    }
}

/// Warns that the custom theme at `path` is skipped because of `error`.
fn warn_bad_theme(path: &Path, error: &str) {
    diag::warn(path, &format!("cannot load theme, skipping it: {error}"));
}

/// Shows a problem that another module found and went on after.
fn show_warning(warning: Warning) {
    match warning {
        Warning::File { path, message } => diag::warn(&path, &message),
        Warning::Note(message) => diag::note(escape_controls(&message)),
    }
}

/// Shows a problem with an input that `print_all` went on after.
fn report(path: &Path, problem: Problem) {
    match problem {
        Problem::Unreadable(e) => diag::warn(path, &describe(e)),
        Problem::CannotHighlight(reason @ GiveUp::Failed(_)) => diag::warn(
            path,
            &format!("cannot highlight: {reason}, printing the rest as plain text"),
        ),
        Problem::CannotHighlight(reason @ GiveUp::Stalled(_)) => diag::warn(
            path,
            &format!(
                "cannot highlight: {reason}, printing the rest and any later inputs as plain text"
            ),
        ),
    }
}

/// The line numbering, squeezing and range of `args`. `--highlight-line`
/// needs the theme's colors, so it applies only with a `highlighter`.
fn build_line_state(args: &Args, highlighter: Option<&Highlighter>) -> LineState {
    let lines = LineState::new(args.numbering(), args.squeeze_blank, args.line_range);
    match highlighter {
        Some(hl) if !args.highlight_line.is_empty() => lines.with_highlight(LineHighlight {
            lines: args.highlight_line.clone(),
            sgr: hl.line_highlight_sgr(),
        }),
        _ => lines,
    }
}

/// Prints the inputs to the pager or stdout, and returns the exit code:
/// failure when an input could not be read.
fn print_inputs(
    args: &Args,
    printer: &Printer,
    lines: LineState,
    color: bool,
    tty: bool,
    stdout: &io::Result<()>,
) -> Result<ExitCode> {
    check_write(stdout)?;
    let pager_env = env::var("PAGER").ok();
    // An empty `LESS` gives `less` no options, so it counts as unset.
    let less_set = env::var_os("LESS").is_some_and(|v| !v.is_empty());
    let mut pager = start_pager(args.paging(), tty, pager_env.as_deref(), less_set)?;
    if color && pager.is_none() {
        console::enable_vt();
    }
    let mut out = TrackedWriter::new(BufWriter::new(match pager.as_mut() {
        Some(p) => Box::new(p.stdin()) as Box<dyn Write>,
        None => Box::new(io::stdout().lock()),
    }));

    let inputs = if args.files.is_empty() {
        vec![PathBuf::from("-")]
    } else {
        args.files.clone()
    };
    let headers = args.header.then(|| Headers::new(color));
    let limits = printer.limits;
    let open = |path| open_input(path, limits);
    let PrintOutcome {
        input_failed,
        write_error,
    } = print_all(&inputs, open, printer, &mut out, lines, headers, report);

    let flushed = match write_error {
        Some(e) => Err(e),
        None => out.flush(),
    };
    check_write(&flushed)?;
    drop(out);
    if let Some(p) = pager {
        p.finish()?;
    }
    Ok(if input_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Starts the pager when `when` asks for one and there is a terminal.
/// `pager` is the value of `$PAGER`, and `less_set` whether `$LESS` is set.
fn start_pager(
    when: PagingWhen,
    tty: bool,
    pager: Option<&str>,
    less_set: bool,
) -> Result<Option<Pager>> {
    if !when.should_page(tty) {
        return Ok(None);
    }
    pager_or_stdout(Pager::spawn(pager, less_set, when == PagingWhen::Auto))
}

/// The pager that `spawned` started, or `None` with a warning when the
/// default pager is missing, as `less` usually is on Windows, so that the
/// output goes to stdout.
fn pager_or_stdout(spawned: Result<Pager, SpawnError>) -> Result<Option<Pager>> {
    match spawned {
        Ok(pager) => Ok(Some(pager)),
        Err(err @ SpawnError::DefaultMissing) => {
            diag::note(err);
            Ok(None)
        }
        Err(err) => Err(err.into()),
    }
}

fn open_input(path: &Path, limits: Limits) -> io::Result<Input<'_>> {
    if is_stdin(path) {
        return Ok(Input {
            source: Box::new(io::stdin().lock()),
            path: None,
            too_large: false,
        });
    }
    let file = File::open(path).map_err(|e| directory_error(path, e))?;
    Ok(Input {
        too_large: limits.exceeds_size(file.metadata()?.len()),
        source: Box::new(file),
        path: Some(path),
    })
}

/// `err`, from opening `path`, as "Is a directory" on Windows when `path` is
/// one. On Unix a directory opens and fails to read with that message, but
/// on Windows opening it fails with "Access is denied".
fn directory_error(path: &Path, err: io::Error) -> io::Error {
    if cfg!(windows) && path.is_dir() {
        io::Error::new(io::ErrorKind::IsADirectory, "Is a directory")
    } else {
        err
    }
}

fn is_broken_pipe(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::BrokenPipe
}

/// Turns the result of writing the output into the error `hl` reports. A
/// closed pipe (`hl ... | head`) is a normal way to stop reading.
fn check_write(result: &io::Result<()>) -> Result<()> {
    match result {
        Err(e) if !is_broken_pipe(e) => bail!("write error: {}", describe(e)),
        _ => Ok(()),
    }
}

/// Writes the output of a command that exits without printing the inputs
/// (`--list-themes`, ...), with write errors handled like the normal output.
fn print_and_exit(
    stdout: &io::Result<()>,
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<ExitCode> {
    check_write(stdout)?;
    let mut out = io::stdout().lock();
    check_write(&write(&mut out).and_then(|()| out.flush()))?;
    Ok(ExitCode::SUCCESS)
}

fn print_cache_report(report: &CacheReport, out: &mut dyn Write) -> io::Result<()> {
    match report {
        CacheReport::Empty { dir } => {
            writeln!(
                out,
                "no custom syntaxes in {}, nothing to cache",
                dir.display()
            )
        }
        CacheReport::Cached { added, total, path } => {
            writeln!(
                out,
                "cached {added} of {total} custom syntaxes in {}",
                path.display()
            )
        }
    }
}

fn load_themes(dirs: &Dirs) -> assets::Themes {
    assets::Themes::load(assets::theme_dir(dirs).as_deref(), show_warning)
}

fn list_themes(dirs: &Dirs, out: &mut dyn Write) -> io::Result<()> {
    let themes = load_themes(dirs);
    let (names, broken) = themes.names();
    for (path, error) in &broken {
        warn_bad_theme(path, error);
    }
    for name in names {
        writeln!(out, "{name}")?;
    }
    Ok(())
}

fn list_languages(dirs: &Dirs, out: &mut dyn Write) -> io::Result<()> {
    let syntaxes = assets::syntaxes(dirs, show_warning);
    for (name, extensions) in highlight::languages(&syntaxes) {
        writeln!(out, "{name}: {}", extensions.join(", "))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::color::Depth;

    /// two-face's syntaxes, so that the user's custom ones stay out. They
    /// are loaded once for all the tests, which is much faster.
    fn syntaxes() -> Arc<SyntaxSet> {
        static SYNTAXES: LazyLock<Arc<SyntaxSet>> =
            LazyLock::new(|| Arc::new(two_face::syntax::extra_newlines()));
        Arc::clone(&SYNTAXES)
    }

    /// `argv` with a fixed color depth, so that the terminal of the
    /// developer does not matter.
    fn args(argv: &[&str]) -> Args {
        let argv: Vec<&str> = ["--color-depth=256"].iter().chain(argv).copied().collect();
        cli::parse_without_env(&argv)
    }

    fn printer(argv: &[&str], color: bool) -> Result<Printer> {
        let parsed = args(argv);
        let syntaxes = parsed.needs_syntaxes(color).then(syntaxes);
        build_printer(&parsed, color, syntaxes, Theme::default)
    }

    fn highlighter() -> Highlighter {
        Highlighter::new(syntaxes(), Theme::default(), Depth::Ansi256, Vec::new())
    }

    #[test]
    fn rejects_an_unknown_language() {
        for color in [true, false] {
            let err = printer(&["--language=nope"], color).err().unwrap();
            assert_eq!(
                err.to_string(),
                "unknown language 'nope' (see --list-languages)"
            );
        }
    }

    #[test]
    fn rejects_an_unknown_language_in_a_mapping() {
        let err = printer(&["--map-syntax=*.foo:nope"], false).err().unwrap();
        assert_eq!(
            err.to_string(),
            "unknown language 'nope' in map-syntax '*.foo:nope' (see --list-languages)"
        );
    }

    #[test]
    fn unknown_language_in_the_config_is_skipped() {
        let origin = Origin::ConfigFile(PathBuf::from("config.toml"));
        let unknown = SyntaxMapping::parse("*.x:no-such-language", origin).unwrap();
        let known = SyntaxMapping::parse("*.y:Rust", Origin::CommandLine).unwrap();
        let mut warnings = Vec::new();
        let resolved =
            resolve_mappings(&syntaxes(), [&unknown, &known], |w| warnings.push(w)).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0.spec, "*.y:Rust");
        assert_eq!(resolved[0].1.name, "Rust");
        assert_eq!(
            warnings,
            [Warning::File {
                path: PathBuf::from("config.toml"),
                message: "unknown language 'no-such-language' in map-syntax \
                          '*.x:no-such-language', ignoring it"
                    .to_string(),
            }]
        );
    }

    #[test]
    fn known_languages_in_mappings_warn_nothing() {
        let origin = Origin::ConfigFile(PathBuf::from("config.toml"));
        let mapping = SyntaxMapping::parse("*.y:Rust", origin).unwrap();
        let mut warnings = Vec::new();
        let resolved = resolve_mappings(&syntaxes(), [&mapping], |w| warnings.push(w)).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(warnings, []);
    }

    #[test]
    fn forces_the_language_even_without_color() {
        for color in [true, false] {
            let printer = printer(&["--language=rust"], color).unwrap();
            assert_eq!(printer.forced.unwrap().name, "Rust");
            assert_eq!(printer.highlighter.is_some(), color);
        }
    }

    #[test]
    fn loads_no_theme_without_color() {
        let args = args(&[]);
        let printer = build_printer(&args, false, None, || -> Theme {
            panic!("the theme must not be loaded")
        })
        .unwrap();
        assert!(printer.highlighter.is_none());
        assert!(printer.forced.is_none());
        assert!(!printer.strip);
    }

    #[test]
    fn highlights_and_strips_escapes_with_color() {
        let printer = printer(&[], true).unwrap();
        assert!(printer.highlighter.is_some());
        assert!(printer.forced.is_none());
        assert!(printer.strip);

        let printer = self::printer(&["--strip-ansi=never"], true).unwrap();
        assert!(!printer.strip);
    }

    #[test]
    fn highlights_lines_only_with_a_highlighter() {
        let args = args(&["-H", "2"]);
        assert!(build_line_state(&args, None).is_passthrough());
        assert!(!build_line_state(&args, Some(&highlighter())).is_passthrough());
    }

    #[test]
    fn passes_lines_through_without_line_options() {
        let args = args(&[]);
        assert!(build_line_state(&args, None).is_passthrough());
        assert!(build_line_state(&args, Some(&highlighter())).is_passthrough());
    }

    #[test]
    fn numbers_lines_without_a_highlighter() {
        let args = args(&["-n"]);
        assert!(!build_line_state(&args, None).is_passthrough());
    }

    const MISSING: Option<&str> = Some("hl-no-such-pager");

    #[test]
    fn starts_no_pager_unless_asked_and_on_a_terminal() {
        // A missing pager would fail if it were started.
        for (when, tty) in [
            (PagingWhen::Never, true),
            (PagingWhen::Always, false),
            (PagingWhen::Auto, false),
        ] {
            let pager = start_pager(when, tty, MISSING, false).unwrap();
            assert!(pager.is_none(), "{when:?}, tty {tty}");
        }
    }

    #[test]
    fn a_missing_default_pager_falls_back_to_stdout() {
        assert!(
            pager_or_stdout(Err(SpawnError::DefaultMissing))
                .unwrap()
                .is_none()
        );
        let err = SpawnError::Start {
            program: "less".into(),
            err: io::ErrorKind::NotFound.into(),
        };
        assert!(pager_or_stdout(Err(err)).is_err());
    }

    /// The error goes up through `run`, and `main` turns every error into
    /// `hl: <message>` and exit status 1.
    #[test]
    fn reports_a_pager_that_fails_to_start() {
        for when in [PagingWhen::Always, PagingWhen::Auto] {
            let err = start_pager(when, true, MISSING, false).err().unwrap();
            let msg = err.to_string();
            assert!(
                msg.starts_with("failed to start pager 'hl-no-such-pager': "),
                "{msg}"
            );
            assert!(!msg.contains("os error"), "{msg}");
        }
    }
}
