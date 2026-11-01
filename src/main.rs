mod assets;
mod cli;
mod color;
mod config;
mod diag;
mod encoding;
mod escape;
mod highlight;
mod lines;
mod pager;
mod print;
mod stdout;

use std::{
    env,
    fs::File,
    io::{self, BufWriter, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Result, anyhow, bail};
use assets::CacheReport;
use cli::{Args, PagingWhen};
use config::Config;
use diag::{describe, escape_controls};
use highlight::{Highlighter, Limits, find_language, resolve_mappings};
use lines::{LineHighlight, LineState};
use pager::Pager;
use print::{Headers, Input, Printer, TrackedWriter, is_stdin, print_all};
use syntect::parsing::SyntaxSet;

fn main() -> ExitCode {
    let args = cli::parse_with(Config::load().as_ref());
    match run(&args, stdout::check()) {
        Ok(code) => code,
        // A closed pipe (`hl ... | head`) is a normal way to stop reading.
        Err(err) if err.downcast_ref::<io::Error>().is_some_and(is_broken_pipe) => {
            ExitCode::SUCCESS
        }
        Err(err) => {
            // The message can contain a path or a name from the user.
            diag::note(escape_controls(&format!("{err}")));
            ExitCode::FAILURE
        }
    }
}

/// Runs `hl`. `stdout` tells whether stdout is open, and is checked only by
/// the commands that write to it.
fn run(args: &Args, stdout: io::Result<()>) -> Result<ExitCode> {
    if let Some(shell) = args.completions {
        return print_and_exit(stdout, |out| cli::print_completions(shell, out));
    }

    if args.man {
        return print_and_exit(stdout, |out| cli::print_man(out));
    }

    if args.build_cache {
        let report = assets::build_cache()?;
        return print_and_exit(stdout, |out| print_cache_report(&report, out));
    }

    if args.clear_cache {
        assets::clear_cache()?;
        return Ok(ExitCode::SUCCESS);
    }

    if args.list_themes {
        return print_and_exit(stdout, list_themes);
    }

    if args.list_languages {
        return print_and_exit(stdout, list_languages);
    }

    let tty = io::stdout().is_terminal();
    let color = args.use_color(tty);
    // Leaked so that highlighting threads can share them; they live as long
    // as the process anyway.
    let syntaxes: Option<&'static SyntaxSet> = args
        .needs_syntaxes(color)
        .then(|| &*Box::leak(Box::new(assets::syntaxes())));
    let forced = args
        .language
        .as_deref()
        .map(|lang| {
            let syntaxes = syntaxes.expect("syntaxes are loaded with --language");
            find_language(syntaxes, lang)
                .ok_or_else(|| anyhow!("unknown language '{lang}' (see --list-languages)"))
        })
        .transpose()?;
    let mappings = match syntaxes {
        Some(syntaxes) => resolve_mappings(syntaxes, args.mappings()).map_err(|e| anyhow!(e))?,
        None => Vec::new(),
    };
    let highlighter = syntaxes.filter(|_| color).map(|syntaxes| {
        let theme = assets::Themes::load().get_or_default(&args.theme);
        Highlighter::new(syntaxes, theme, args.depth(), mappings)
    });
    let printer = Printer {
        highlighter,
        forced,
        vis: args.visibility(),
        limits: args.limits(),
        strip: args.strip_escapes(color),
    };

    check_write(stdout)?;
    let pager_env = env::var("PAGER").ok();
    // An empty `LESS` gives `less` no options, so it counts as unset.
    let less_set = env::var_os("LESS").is_some_and(|v| !v.is_empty());
    let mut pager = start_pager(args.paging(), tty, pager_env.as_deref(), less_set)?;
    let mut out = TrackedWriter::new(BufWriter::new(match pager.as_mut() {
        Some(p) => Box::new(p.stdin()) as Box<dyn Write>,
        None => Box::new(io::stdout().lock()),
    }));

    let inputs = if args.files.is_empty() {
        vec![PathBuf::from("-")]
    } else {
        args.files.clone()
    };
    let mut lines = LineState::new(args.numbering(), args.squeeze_blank, args.line_range);
    if let Some(hl) = &printer.highlighter
        && !args.highlight_line.is_empty()
    {
        lines = lines.with_highlight(LineHighlight {
            lines: args.highlight_line.clone(),
            sgr: hl.line_highlight_sgr(),
        });
    }
    let headers = args.header.then(|| Headers::new(color));
    let limits = printer.limits;
    let open = |path| open_input(path, limits);
    let (failed, write_error) = print_all(&inputs, open, &printer, &mut out, lines, headers);

    let flushed = match write_error {
        Some(e) => Err(e),
        None => out.flush(),
    };
    check_write(flushed)?;
    drop(out);
    if let Some(p) = pager {
        p.finish()?;
    }
    Ok(if failed {
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
    Ok(Some(Pager::spawn(
        pager,
        less_set,
        when == PagingWhen::Auto,
    )?))
}

fn open_input(path: &Path, limits: Limits) -> io::Result<Input<'_>> {
    if is_stdin(path) {
        return Ok(Input {
            source: Box::new(io::stdin().lock()),
            path: None,
            too_large: false,
        });
    }
    let file = File::open(path)?;
    Ok(Input {
        too_large: limits.exceeds_size(file.metadata()?.len()),
        source: Box::new(file),
        path: Some(path),
    })
}

fn is_broken_pipe(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::BrokenPipe
}

/// Turns the result of writing the output into the error `hl` reports. A
/// closed pipe (`hl ... | head`) is a normal way to stop reading.
fn check_write(result: io::Result<()>) -> Result<()> {
    match result {
        Err(e) if !is_broken_pipe(&e) => bail!("write error: {}", describe(&e)),
        _ => Ok(()),
    }
}

/// Writes the output of a command that exits without printing the inputs
/// (`--list-themes`, ...), with write errors handled like the normal output.
fn print_and_exit(
    stdout: io::Result<()>,
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<ExitCode> {
    check_write(stdout)?;
    let mut out = io::stdout().lock();
    check_write(write(&mut out).and_then(|()| out.flush()))?;
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

fn list_themes(out: &mut dyn Write) -> io::Result<()> {
    let themes = assets::Themes::load();
    for name in themes.names() {
        writeln!(out, "{name}")?;
    }
    Ok(())
}

fn list_languages(out: &mut dyn Write) -> io::Result<()> {
    let syntaxes = assets::syntaxes();
    for (name, extensions) in highlight::languages(&syntaxes) {
        writeln!(out, "{name}: {}", extensions.join(", "))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
