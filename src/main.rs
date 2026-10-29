mod assets;
mod cli;
mod color;
mod config;
mod diag;
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
use cli::{Args, ColorWhen, PagingWhen};
use diag::describe;
use encoding::{UTF8_BOM, Utf16Decoder, is_utf8_prefix, utf16_bom};
use highlight::{Highlighter, Limits, SyntaxMapping, find_language, write_plain};
use lines::{LineState, Visibility};
use pager::Pager;
use syntect::parsing::{SyntaxReference, SyntaxSet};

/// How many leading bytes are inspected for binary detection.
const PEEK_LEN: usize = 8 * 1024;

fn main() -> ExitCode {
    let args = cli::parse();
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
        return assets::build_cache();
    }

    if args.clear_cache {
        return assets::clear_cache();
    }

    if args.list_themes {
        return list_themes();
    }

    if args.list_languages {
        return list_languages(&assets::syntaxes());
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
    // Syntaxes are also loaded to report unknown languages given on the
    // command line, even when nothing gets highlighted.
    let syntaxes =
        (color || args.language.is_some() || !args.map_syntax.is_empty()).then(assets::syntaxes);
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
        Some(syntaxes) => resolve_mappings(syntaxes, args)?,
        None => Vec::new(),
    };
    let highlighter = syntaxes.as_ref().filter(|_| color).map(|syntaxes| {
        let theme = assets::Themes::load().get_or_default(&args.theme);
        Highlighter::new(syntaxes, theme, args.depth(), mappings)
    });
    let printer = Printer {
        highlighter,
        forced,
        vis,
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
    let (failed, write_error) = print_all(&inputs, &printer, &mut out, lines, headers);

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

/// Starts the pager when `when` asks for one. Without a terminal there is
/// nobody to page for, as with `--color=auto`.
fn start_pager(when: PagingWhen, tty: bool) -> Result<Option<Pager>> {
    if when == PagingWhen::Never || !tty {
        return Ok(None);
    }
    let pager = Pager::spawn(when == PagingWhen::Auto)
        .map_err(|e| anyhow!("failed to start pager: {e}"))?;
    Ok(Some(pager))
}

/// Prints the inputs in turn. An input that cannot be read is reported and
/// skipped. Returns whether there was one, and the write error that ended
/// the output early, if any.
fn print_all<W: Write>(
    inputs: &[PathBuf],
    printer: &Printer,
    out: &mut TrackedWriter<W>,
    mut lines: LineState,
    mut headers: Option<Headers>,
) -> (bool, Option<io::Error>) {
    let mut failed = false;
    for input in inputs {
        // A file that cannot be opened gets no header, like with `head`.
        let result = open_input(input, printer.limits).and_then(|source| {
            if let Some(headers) = headers.as_mut() {
                headers.write(input, out, &mut lines)?;
            }
            lines.start_input(out)?;
            printer.print(source, out, &mut lines)
        });
        match result {
            Ok(()) => {}
            // The output is gone, so there is no point in reading more input.
            Err(e) if out.failed() => return (failed, Some(e)),
            Err(e) => {
                eprintln!("hl: {}: {}", input.display(), describe(&e));
                failed = true;
            }
        }
    }
    (failed, None)
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

/// `==> FILE <==` lines written before each input with `--header`.
struct Headers {
    bold: bool,
    printed: bool,
}

impl Headers {
    fn new(bold: bool) -> Self {
        Self {
            bold,
            printed: false,
        }
    }

    fn write(
        &mut self,
        path: &Path,
        out: &mut impl Write,
        lines: &mut LineState,
    ) -> io::Result<()> {
        lines.separate(out)?;
        if self.printed {
            out.write_all(b"\n")?;
        }
        self.printed = true;
        let name = if is_stdin(path) {
            "standard input".into()
        } else {
            path.display().to_string()
        };
        if self.bold {
            writeln!(out, "\x1b[1m==> {name} <==\x1b[0m")
        } else {
            writeln!(out, "==> {name} <==")
        }
    }
}

/// An opened input. `path` is `None` for stdin.
struct Input<'a> {
    source: Box<dyn io::Read>,
    path: Option<&'a Path>,
    too_large: bool,
}

fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
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

/// What every input is printed with.
struct Printer<'a> {
    /// `None` when the output is not colored.
    highlighter: Option<Highlighter<'a>>,
    /// The syntax given by `--language`.
    forced: Option<&'a SyntaxReference>,
    vis: Visibility,
    limits: Limits,
}

impl<'a> Printer<'a> {
    fn print(&self, input: Input, out: &mut impl Write, lines: &mut LineState) -> io::Result<()> {
        let Input {
            source,
            path,
            too_large,
        } = input;
        let mut reader = BufReader::with_capacity(64 * 1024, source);

        let Some(hl) = self.highlighter.as_ref().filter(|_| !too_large) else {
            return write_plain(&mut reader, out, lines, self.vis);
        };
        let head = reader.fill_buf()?;
        if let Some(endian) = utf16_bom(head) {
            // Terminals expect UTF-8, so UTF-16 is converted for highlighting.
            reader.consume(2);
            let mut decoded = BufReader::new(Utf16Decoder::new(&mut reader, endian));
            let syntax = self.choose_syntax(hl, &mut decoded, path)?;
            return hl.write(&mut decoded, out, syntax, lines, self.limits);
        }
        let peek = &head[..head.len().min(PEEK_LEN)];
        if peek.contains(&0) || !is_utf8_prefix(peek) {
            return write_plain(&mut reader, out, lines, self.vis);
        }
        let syntax = self.choose_syntax(hl, &mut reader, path)?;
        hl.write(&mut reader, out, syntax, lines, self.limits)
    }

    /// Uses the forced syntax, or detects one from the path and the first line.
    fn choose_syntax(
        &self,
        hl: &Highlighter<'a>,
        reader: &mut impl BufRead,
        path: Option<&Path>,
    ) -> io::Result<&'a SyntaxReference> {
        if let Some(syntax) = self.forced {
            return Ok(syntax);
        }
        let head = reader.fill_buf()?;
        let head = head.strip_prefix(UTF8_BOM).unwrap_or(head);
        let first_line = head.split(|&b| b == b'\n').next().unwrap_or_default();
        Ok(hl.detect(path, &String::from_utf8_lossy(first_line)))
    }
}

fn is_broken_pipe(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::BrokenPipe
}

/// Looks up the language of each syntax mapping. An unknown language is an
/// error on the command line, but only a warning in the config file, which
/// never stops `hl` from printing.
fn resolve_mappings<'a>(
    syntaxes: &'a SyntaxSet,
    args: &Args,
) -> Result<Vec<(SyntaxMapping, &'a SyntaxReference)>> {
    let mut resolved = Vec::new();
    for mapping in args.mappings() {
        let Some(syntax) = find_language(syntaxes, &mapping.language) else {
            let msg = format!(
                "unknown language '{}' in map-syntax '{}'",
                mapping.language, mapping.spec
            );
            match &mapping.origin {
                Some(path) => diag::warn(path, &format!("{msg}, ignoring it")),
                None => bail!("{msg} (see --list-languages)"),
            }
            continue;
        };
        resolved.push((mapping.clone(), syntax));
    }
    Ok(resolved)
}

fn list_themes() -> Result<ExitCode> {
    let themes = assets::Themes::load();
    let mut out = io::stdout().lock();
    for name in themes.names() {
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
