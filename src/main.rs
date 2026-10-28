mod cli;
mod color;
mod encoding;
mod highlight;
mod lines;
mod pager;

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use clap::Parser;
use syntect::parsing::SyntaxReference;

use cli::{Args, ColorWhen};
use encoding::{Bom, UTF8_BOM, Utf16Decoder, is_utf8_prefix, sniff_bom};
use highlight::{Highlighter, Limits, write_plain};
use lines::{LineState, Visibility};
use pager::Pager;

/// How many leading bytes are inspected for binary detection.
const PEEK_LEN: usize = 8 * 1024;

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("hl: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<ExitCode> {
    if args.list_themes {
        return list_themes();
    }

    let tty = io::stdout().is_terminal();
    let vis = args.visibility();
    // `cat -v` style output is never highlighted, so it matches cat exactly.
    let color = !vis.any()
        && match args.color {
            ColorWhen::Always => true,
            ColorWhen::Never => false,
            ColorWhen::Auto => tty,
        };
    let highlighter = if color || args.list_languages || args.language.is_some() {
        Some(Highlighter::new(&args.theme, args.depth()))
    } else {
        None
    };

    if args.list_languages {
        return list_languages(highlighter.as_ref().expect("highlighter is built"));
    }

    let forced_syntax = match (&highlighter, &args.language) {
        (Some(h), Some(lang)) => Some(
            h.find_language(lang)
                .ok_or_else(|| anyhow!("unknown language '{lang}' (see --list-languages)"))?,
        ),
        _ => None,
    };
    let highlighter = if color { highlighter.as_ref() } else { None };

    let mut pager = if args.paging && tty {
        Some(Pager::spawn().map_err(|e| anyhow!("failed to start pager: {e}"))?)
    } else {
        None
    };
    let stdout = io::stdout();
    let mut out: BufWriter<Box<dyn Write>> = BufWriter::new(match pager.as_mut() {
        Some(p) => Box::new(p.stdin()),
        None => Box::new(stdout.lock()),
    });

    let inputs = if args.files.is_empty() {
        vec![PathBuf::from("-")]
    } else {
        args.files.clone()
    };

    let limits = args.limits();
    let mut lines = LineState::new(args.numbering(), args.squeeze_blank);
    let mut failed = false;
    for input in &inputs {
        let result = print_input(
            input,
            &mut out,
            highlighter,
            forced_syntax,
            &mut lines,
            vis,
            limits,
        );
        match result {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break,
            Err(e) => {
                eprintln!("hl: {}: {}", input.display(), describe(&e));
                failed = true;
            }
        }
    }

    match out.flush() {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => return Err(e.into()),
        _ => {}
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

fn print_input(
    path: &Path,
    out: &mut impl Write,
    highlighter: Option<&Highlighter>,
    forced_syntax: Option<&SyntaxReference>,
    lines: &mut LineState,
    vis: Visibility,
    limits: Limits,
) -> io::Result<()> {
    let is_stdin = path.as_os_str() == "-";
    let mut too_large = false;
    let source: Box<dyn io::Read> = if is_stdin {
        Box::new(io::stdin().lock())
    } else {
        let file = File::open(path)?;
        too_large = limits.exceeds_size(file.metadata()?.len());
        Box::new(file)
    };
    let mut reader = BufReader::with_capacity(64 * 1024, source);

    let Some(hl) = highlighter.filter(|_| !too_large) else {
        return write_plain(&mut reader, out, lines, vis);
    };
    let path = (!is_stdin).then_some(path);
    let head = reader.fill_buf()?;
    if let Some(Bom::Utf16 { big_endian }) = sniff_bom(head) {
        // Terminals expect UTF-8, so UTF-16 is converted for highlighting.
        reader.consume(2);
        let mut decoded = BufReader::new(Utf16Decoder::new(&mut reader, big_endian));
        let syntax = choose_syntax(hl, forced_syntax, &mut decoded, path)?;
        return hl.write(&mut decoded, out, syntax, lines, limits);
    }
    let peek = &head[..head.len().min(PEEK_LEN)];
    if peek.contains(&0) || !is_utf8_prefix(peek) {
        return write_plain(&mut reader, out, lines, vis);
    }
    let syntax = choose_syntax(hl, forced_syntax, &mut reader, path)?;
    hl.write(&mut reader, out, syntax, lines, limits)
}

/// Uses the forced syntax, or detects one from the path and the first line.
fn choose_syntax<'a>(
    hl: &'a Highlighter,
    forced: Option<&'a SyntaxReference>,
    reader: &mut impl BufRead,
    path: Option<&Path>,
) -> io::Result<&'a SyntaxReference> {
    if let Some(syntax) = forced {
        return Ok(syntax);
    }
    let head = reader.fill_buf()?;
    let head = head.strip_prefix(UTF8_BOM).unwrap_or(head);
    let first_line = head.split(|&b| b == b'\n').next().unwrap_or_default();
    Ok(hl.detect(path, &String::from_utf8_lossy(first_line)))
}

/// Strips the "(os error N)" suffix to match coreutils-style messages.
fn describe(err: &io::Error) -> String {
    let msg = err.to_string();
    match msg.find(" (os error") {
        Some(i) => msg[..i].to_string(),
        None => msg,
    }
}

fn list_themes() -> Result<ExitCode> {
    let themes = two_face::theme::LazyThemeSet::from(two_face::theme::extra());
    let mut names: Vec<_> = themes.theme_names().collect();
    names.sort_unstable();
    let mut out = io::stdout().lock();
    for name in names {
        writeln!(out, "{name}")?;
    }
    Ok(ExitCode::SUCCESS)
}

fn list_languages(hl: &Highlighter) -> Result<ExitCode> {
    let mut syntaxes: Vec<_> = hl.syntaxes().syntaxes().iter().filter(|s| !s.hidden).collect();
    syntaxes.sort_unstable_by_key(|s| s.name.to_lowercase());
    let mut out = io::stdout().lock();
    for s in syntaxes {
        writeln!(out, "{}: {}", s.name, s.file_extensions.join(", "))?;
    }
    Ok(ExitCode::SUCCESS)
}
