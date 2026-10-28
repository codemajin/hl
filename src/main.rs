mod cli;
mod color;
mod encoding;
mod highlight;
mod lines;
mod pager;

use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use cli::{Args, ColorWhen};
use encoding::{Bom, UTF8_BOM, Utf16Decoder, is_utf8_prefix, sniff_bom};
use highlight::{DEFAULT_THEME, Highlighter, Limits, find_language, find_theme, write_plain};
use lines::{LineState, Visibility};
use pager::Pager;
use syntect::{
    highlighting::Theme,
    parsing::{SyntaxReference, SyntaxSet},
};

/// How many leading bytes are inspected for binary detection.
const PEEK_LEN: usize = 8 * 1024;

fn main() -> ExitCode {
    let args = Args::parse();
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
    if args.list_themes {
        return list_themes();
    }

    if args.list_languages {
        return list_languages(&two_face::syntax::extra_newlines());
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
    let syntaxes = (color || args.language.is_some()).then(two_face::syntax::extra_newlines);
    let forced_syntax = match (&syntaxes, &args.language) {
        (Some(syntaxes), Some(lang)) => Some(
            find_language(syntaxes, lang)
                .ok_or_else(|| anyhow!("unknown language '{lang}' (see --list-languages)"))?,
        ),
        _ => None,
    };
    let highlighter = syntaxes
        .as_ref()
        .filter(|_| color)
        .map(|syntaxes| Highlighter::new(syntaxes, load_theme(&args.theme), args.depth()));

    let mut pager = if args.paging && tty {
        Some(Pager::spawn().map_err(|e| anyhow!("failed to start pager: {e}"))?)
    } else {
        None
    };
    let stdout = io::stdout();
    let mut out = TrackedWriter::new(BufWriter::new(match pager.as_mut() {
        Some(p) => Box::new(p.stdin()) as Box<dyn Write>,
        None => Box::new(stdout.lock()),
    }));

    let inputs = if args.files.is_empty() {
        vec![PathBuf::from("-")]
    } else {
        args.files.clone()
    };

    let limits = args.limits();
    let mut lines = LineState::new(args.numbering(), args.squeeze_blank);
    let mut failed = false;
    let mut write_error = None;
    for input in &inputs {
        let result = print_input(
            input,
            &mut out,
            highlighter.as_ref(),
            forced_syntax,
            &mut lines,
            vis,
            limits,
        );
        match result {
            Ok(()) => {}
            // The output is gone, so there is no point in reading more input.
            Err(e) if out.failed() => {
                write_error = Some(e);
                break;
            }
            Err(e) => {
                eprintln!("hl: {}: {}", input.display(), describe(&e));
                failed = true;
            }
        }
    }

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

/// Remembers whether writing failed, so that an error coming out of
/// `print_input` can be told apart from an error reading the input.
struct TrackedWriter<W> {
    inner: W,
    failed: bool,
}

impl<W: Write> TrackedWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            failed: false,
        }
    }

    fn failed(&self) -> bool {
        self.failed
    }

    fn track<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        if let Err(e) = &result
            && e.kind() != io::ErrorKind::Interrupted
        {
            self.failed = true;
        }
        result
    }
}

impl<W: Write> Write for TrackedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let result = self.inner.write(buf);
        self.track(result)
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self.inner.flush();
        self.track(result)
    }
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
    hl: &Highlighter<'a>,
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

fn is_broken_pipe(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::BrokenPipe
}

/// Strips the "(os error N)" suffix to match coreutils-style messages.
fn describe(err: &io::Error) -> String {
    let msg = err.to_string();
    match msg.find(" (os error") {
        Some(i) => msg[..i].to_string(),
        None => msg,
    }
}

/// An unknown theme is not fatal: it warns and falls back to the default,
/// so a typo in `HL_THEME` never hides the file contents.
fn load_theme(name: &str) -> Theme {
    let themes = highlight::themes();
    find_theme(&themes, name)
        .unwrap_or_else(|| {
            eprintln!(
                "hl: unknown theme '{name}', falling back to {DEFAULT_THEME} (see --list-themes)"
            );
            themes.get(DEFAULT_THEME).expect("default theme is bundled")
        })
        .clone()
}

fn list_themes() -> Result<ExitCode> {
    let themes = highlight::themes();
    let mut names: Vec<_> = themes.theme_names().collect();
    names.sort_unstable();
    let mut out = io::stdout().lock();
    for name in names {
        writeln!(out, "{name}")?;
    }
    Ok(ExitCode::SUCCESS)
}

fn list_languages(syntaxes: &SyntaxSet) -> Result<ExitCode> {
    let mut syntaxes: Vec<_> = syntaxes.syntaxes().iter().filter(|s| !s.hidden).collect();
    syntaxes.sort_unstable_by_key(|s| s.name.to_lowercase());
    let mut out = io::stdout().lock();
    for s in syntaxes {
        writeln!(out, "{}: {}", s.name, s.file_extensions.join(", "))?;
    }
    Ok(ExitCode::SUCCESS)
}
