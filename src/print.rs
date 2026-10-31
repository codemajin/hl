//! Printing the inputs in turn, highlighted or plain, which is what `hl` is
//! for. Opening the inputs and choosing the output are left to the caller.

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};

use crate::{
    diag::describe,
    encoding::{UTF8_BOM, Utf16Decoder, is_utf8_prefix, utf16_bom},
    escape::StripEscapes,
    highlight::{Highlighter, Limits, Syntax},
    lines::{LineState, Visibility, write_plain},
};

/// How many leading bytes are inspected for binary detection.
const PEEK_LEN: usize = 8 * 1024;

/// Prints the inputs in turn, opening each with `open`. An input that cannot
/// be read is reported and skipped. Returns whether there was one, and the
/// write error that ended the output early, if any.
pub fn print_all<'p, W: Write>(
    inputs: &'p [PathBuf],
    mut open: impl FnMut(&'p Path) -> io::Result<Input<'p>>,
    printer: &Printer,
    out: &mut TrackedWriter<W>,
    mut lines: LineState,
    mut headers: Option<Headers>,
) -> (bool, Option<io::Error>) {
    let mut failed = false;
    for input in inputs {
        // A file that cannot be opened gets no header, like with `head`.
        let result = open(input).and_then(|source| {
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
/// `print_all` can be told apart from an error reading the input.
pub struct TrackedWriter<W> {
    inner: W,
    failed: bool,
}

impl<W: Write> TrackedWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            failed: false,
        }
    }

    pub fn failed(&self) -> bool {
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
pub struct Headers {
    bold: bool,
    printed: bool,
}

impl Headers {
    pub fn new(bold: bool) -> Self {
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
pub struct Input<'a> {
    pub source: Box<dyn Read>,
    pub path: Option<&'a Path>,
    /// Whether the input is too large to highlight.
    pub too_large: bool,
}

/// Whether `path` names stdin (`-`).
pub fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// What every input is printed with.
pub struct Printer<'a> {
    /// `None` when the output is not colored.
    pub highlighter: Option<Highlighter<'a>>,
    /// The syntax given by `--language`.
    pub forced: Option<&'a Syntax>,
    pub vis: Visibility,
    pub limits: Limits,
    /// Whether to remove the escape sequences of the input.
    pub strip: bool,
}

impl<'a> Printer<'a> {
    fn print(&self, input: Input, out: &mut impl Write, lines: &mut LineState) -> io::Result<()> {
        let Input {
            source,
            path,
            too_large,
        } = input;
        let mut reader = BufReader::with_capacity(64 * 1024, source);

        let hl = self.highlighter.as_ref().filter(|_| !too_large);
        if hl.is_none() && !self.strip {
            return write_plain(&mut reader, out, lines, self.vis);
        }
        let head = reader.fill_buf()?;
        if let Some(endian) = utf16_bom(head) {
            let Some(hl) = hl else {
                // UTF-16 printed as is keeps its escape sequences, because
                // dropping single bytes would break it.
                return write_plain(&mut reader, out, lines, self.vis);
            };
            // Terminals expect UTF-8, so UTF-16 is converted for highlighting.
            reader.consume(2);
            let decoded = BufReader::new(Utf16Decoder::new(&mut reader, endian));
            return self.filtered(decoded, |r| self.highlight(hl, r, path, out, lines));
        }
        let peek = &head[..head.len().min(PEEK_LEN)];
        let binary = peek.contains(&0) || !is_utf8_prefix(peek);
        match hl.filter(|_| !binary) {
            Some(hl) => self.filtered(reader, |r| self.highlight(hl, r, path, out, lines)),
            None => self.filtered(reader, |mut r| write_plain(&mut r, out, lines, self.vis)),
        }
    }

    /// Runs `f` on `reader`, with the escape sequences removed when asked.
    fn filtered<R: BufRead>(
        &self,
        reader: R,
        f: impl FnOnce(&mut dyn BufRead) -> io::Result<()>,
    ) -> io::Result<()> {
        if self.strip {
            f(&mut StripEscapes::new(reader))
        } else {
            let mut reader = reader;
            f(&mut reader)
        }
    }

    fn highlight(
        &self,
        hl: &Highlighter<'a>,
        mut reader: &mut dyn BufRead,
        path: Option<&Path>,
        out: &mut impl Write,
        lines: &mut LineState,
    ) -> io::Result<()> {
        let syntax = self.choose_syntax(hl, &mut reader, path)?;
        hl.write(&mut reader, out, syntax, lines, self.limits)
    }

    /// Uses the forced syntax, or detects one from the path and the first line.
    fn choose_syntax(
        &self,
        hl: &Highlighter<'a>,
        reader: &mut impl BufRead,
        path: Option<&Path>,
    ) -> io::Result<&'a Syntax> {
        if let Some(syntax) = self.forced {
            return Ok(syntax);
        }
        let head = reader.fill_buf()?;
        let head = head.strip_prefix(UTF8_BOM).unwrap_or(head);
        let first_line = head.split(|&b| b == b'\n').next().unwrap_or_default();
        Ok(hl.detect(path, &String::from_utf8_lossy(first_line)))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::lines::Numbering;

    fn plain() -> Printer<'static> {
        Printer {
            highlighter: None,
            forced: None,
            vis: Visibility::default(),
            limits: Limits {
                max_size: None,
                max_line: None,
            },
            strip: false,
        }
    }

    fn input(text: &'static str) -> Input<'static> {
        Input {
            source: Box::new(Cursor::new(text)),
            path: None,
            too_large: false,
        }
    }

    /// Runs `print_all` with `inputs` opened from `files`, a missing name
    /// failing to open.
    fn run(
        inputs: &[PathBuf],
        files: &[(&str, &'static str)],
        lines: LineState,
        headers: Option<Headers>,
    ) -> (String, bool) {
        let mut out = TrackedWriter::new(Vec::new());
        let open = |path: &Path| {
            files
                .iter()
                .find(|(name, _)| path == Path::new(name))
                .map(|&(_, text)| input(text))
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        };
        let (failed, write_error) = print_all(inputs, open, &plain(), &mut out, lines, headers);
        assert!(write_error.is_none());
        (String::from_utf8(out.inner).unwrap(), failed)
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn unreadable_input_is_skipped() {
        let lines = LineState::new(Numbering::All, false, None);
        let (out, failed) = run(
            &paths(&["a", "missing", "b"]),
            &[("a", "one\n"), ("b", "two\n")],
            lines,
            None,
        );
        assert!(failed);
        assert_eq!(out, "     1\tone\n     2\ttwo\n");
    }

    #[test]
    fn headers_name_each_input() {
        let lines = LineState::new(Numbering::None, false, None);
        let (out, failed) = run(
            &paths(&["a", "-"]),
            &[("a", "one\n"), ("-", "two\n")],
            lines,
            Some(Headers::new(false)),
        );
        assert!(!failed);
        assert_eq!(out, "==> a <==\none\n\n==> standard input <==\ntwo\n");
    }

    #[test]
    fn unopened_input_gets_no_header() {
        let lines = LineState::new(Numbering::None, false, None);
        let (out, _) = run(
            &paths(&["missing", "a"]),
            &[("a", "one\n")],
            lines,
            Some(Headers::new(true)),
        );
        assert_eq!(out, "\x1b[1m==> a <==\x1b[0m\none\n");
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_error_stops_printing() {
        let inputs = paths(&["a", "b"]);
        let mut opened = 0;
        let open = |_: &Path| {
            opened += 1;
            Ok(input("text\n"))
        };
        let mut out = TrackedWriter::new(Broken);
        let lines = LineState::new(Numbering::None, false, None);
        let (failed, write_error) = print_all(&inputs, open, &plain(), &mut out, lines, None);
        assert!(!failed);
        assert!(write_error.is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe));
        assert!(out.failed());
        assert_eq!(opened, 1);
    }
}
