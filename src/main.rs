mod assets;
mod cli;
mod color;
mod config;
mod diag;
mod encoding;
mod highlight;
mod lines;
mod pager;
mod print;

use std::{
    fs::File,
    io::{self, BufWriter, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Result, anyhow, bail};
use assets::CacheReport;
use cli::{Args, PagingWhen};
use config::Config;
use diag::describe;
use highlight::{Highlighter, Limits, find_language, resolve_mappings};
use lines::LineState;
use pager::Pager;
use print::{Headers, Input, Printer, TrackedWriter, is_stdin, print_all};

fn main() -> ExitCode {
    let args = cli::parse_with(Config::load().as_ref());
    match run(&args) {
        Ok(code) => code,
        // A closed pipe (`hl ... | head`) is a normal way to stop reading.
        Err(err) if err.downcast_ref::<io::Error>().is_some_and(is_broken_pipe) => {
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("hl: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<ExitCode> {
    if args.build_cache {
        print_cache_report(&assets::build_cache()?);
        return Ok(ExitCode::SUCCESS);
    }

    if args.clear_cache {
        assets::clear_cache()?;
        return Ok(ExitCode::SUCCESS);
    }

    if args.list_themes {
        return list_themes();
    }

    if args.list_languages {
        return list_languages();
    }

    let tty = io::stdout().is_terminal();
    let color = args.use_color(tty);
    let syntaxes = args.needs_syntaxes(color).then(assets::syntaxes);
    let forced = args
        .language
        .as_deref()
        .map(|lang| {
            let syntaxes = syntaxes
                .as_ref()
                .expect("syntaxes are loaded with --language");
            find_language(syntaxes, lang)
                .ok_or_else(|| anyhow!("unknown language '{lang}' (see --list-languages)"))
        })
        .transpose()?;
    let mappings = match &syntaxes {
        Some(syntaxes) => resolve_mappings(syntaxes, args.mappings()).map_err(|e| anyhow!(e))?,
        None => Vec::new(),
    };
    let highlighter = syntaxes.as_ref().filter(|_| color).map(|syntaxes| {
        let theme = assets::Themes::load().get_or_default(&args.theme);
        Highlighter::new(syntaxes, theme, args.depth(), mappings)
    });
    let printer = Printer {
        highlighter,
        forced,
        vis: args.visibility(),
        limits: args.limits(),
    };

    let mut pager = start_pager(args.paging(), tty)?;
    let mut out = TrackedWriter::new(BufWriter::new(match pager.as_mut() {
        Some(p) => Box::new(p.stdin()) as Box<dyn Write>,
        None => Box::new(io::stdout().lock()),
    }));

    let inputs = if args.files.is_empty() {
        vec![PathBuf::from("-")]
    } else {
        args.files.clone()
    };
    let lines = LineState::new(args.numbering(), args.squeeze_blank, args.line_range);
    let headers = args.header.then(|| Headers::new(color));
    let limits = printer.limits;
    let open = |path| open_input(path, limits);
    let (failed, write_error) = print_all(&inputs, open, &printer, &mut out, lines, headers);

    let flushed = match write_error {
        Some(e) => Err(e),
        None => out.flush(),
    };
    if let Err(e) = flushed
        && !is_broken_pipe(&e)
    {
        bail!("write error: {}", describe(&e));
    }
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
fn start_pager(when: PagingWhen, tty: bool) -> Result<Option<Pager>> {
    if !when.should_page(tty) {
        return Ok(None);
    }
    let pager = Pager::spawn(when == PagingWhen::Auto)
        .map_err(|e| anyhow!("failed to start pager: {e}"))?;
    Ok(Some(pager))
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

fn print_cache_report(report: &CacheReport) {
    match report {
        CacheReport::Empty { dir } => {
            println!("no custom syntaxes in {}, nothing to cache", dir.display());
        }
        CacheReport::Cached { added, total, path } => {
            println!(
                "cached {added} of {total} custom syntaxes in {}",
                path.display()
            );
        }
    }
}

fn list_themes() -> Result<ExitCode> {
    let themes = assets::Themes::load();
    let mut out = io::stdout().lock();
    for name in themes.names() {
        writeln!(out, "{name}")?;
    }
    Ok(ExitCode::SUCCESS)
}

fn list_languages() -> Result<ExitCode> {
    let syntaxes = assets::syntaxes();
    let mut out = io::stdout().lock();
    for (name, extensions) in highlight::languages(&syntaxes) {
        writeln!(out, "{name}: {}", extensions.join(", "))?;
    }
    Ok(ExitCode::SUCCESS)
}
