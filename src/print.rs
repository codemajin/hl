//! Printing the inputs in turn, highlighted or plain, which is what `hl` is
//! for. Opening the inputs and choosing the output are left to the caller.

use std::{
    io::{self, BufRead, BufReader, Cursor, Read, Write},
    path::{Path, PathBuf},
};

use crate::{
    color::{BOLD, RESET},
    diag::display_safe,
    encoding::{Endian, UTF8_BOM, Utf16Decoder, is_utf8_prefix, utf16_bom},
    escape::StripEscapes,
    highlight::{GiveUp, Highlighter, Limits, Syntax},
    lines::{LineState, Visibility, write_plain},
};

/// How many leading bytes are read before deciding how to print an input:
/// whether it starts with a UTF-16 BOM, whether it is binary or not UTF-8,
/// and what its first line (a shebang) is.
const PEEK_LEN: usize = 8 * 1024;

/// The size of the buffer each input is read through.
const BUF_LEN: usize = 64 * 1024;

/// A problem with one input that does not stop `print_all`, passed to the
/// caller to show.
#[derive(Debug, Clone, Copy)]
pub enum Problem<'a> {
    /// The input could not be opened or read, and was skipped.
    Unreadable(&'a io::Error),
    /// Highlighting failed for this reason, and the rest of the input was
    /// printed plain. After `GiveUp::Stalled`, so are the later inputs.
    CannotHighlight(&'a GiveUp),
}

/// What happened in `print_all`.
#[derive(Debug)]
pub struct PrintOutcome {
    /// Whether an input could not be read.
    pub input_failed: bool,
    /// The write error that ended the output early, if any.
    pub write_error: Option<io::Error>,
}

/// Prints the inputs in turn, opening each with `open`. An input that cannot
/// be read is skipped. Problems that do not stop the output are passed to
/// `report` with the path of their input.
pub fn print_all<'p, W: Write>(
    inputs: &'p [PathBuf],
    mut open: impl FnMut(&'p Path) -> io::Result<Input<'p>>,
    printer: &Printer,
    out: &mut TrackedWriter<W>,
    mut lines: LineState,
    mut headers: Option<Headers>,
    mut report: impl FnMut(&Path, Problem),
) -> PrintOutcome {
    let mut input_failed = false;
    for input in inputs {
        let result = open(input).and_then(|source| {
            let mut reader = BufReader::with_capacity(BUF_LEN, source.source);
            // A file that cannot be opened gets no header, like with `head`,
            // nor does one that opens but cannot be read, like a directory.
            reader.fill_buf()?;
            if let Some(headers) = headers.as_mut() {
                headers.write(input, out, &mut lines)?;
            }
            lines.start_input(out)?;
            printer.print(
                &mut reader,
                source.path,
                source.too_large,
                out,
                &mut lines,
                &mut report,
            )
        });
        match result {
            Ok(()) => {}
            // The output is gone, so there is no point in reading more input.
            Err(e) if out.failed() => {
                return PrintOutcome {
                    input_failed,
                    write_error: Some(e),
                };
            }
            Err(e) => {
                report(input, Problem::Unreadable(&e));
                input_failed = true;
            }
        }
    }
    PrintOutcome {
        input_failed,
        write_error: None,
    }
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
            display_safe(path)
        };
        if self.bold {
            writeln!(out, "{BOLD}==> {name} <=={RESET}")
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
pub struct Printer {
    /// `None` when the output is not colored.
    pub highlighter: Option<Highlighter>,
    /// The syntax given by `--language`.
    pub forced: Option<Syntax>,
    /// `-v`, `-E` and `-T`. With any of them, nothing is highlighted, even
    /// with a `highlighter`.
    pub vis: Visibility,
    pub limits: Limits,
    /// Whether to remove the escape sequences of the input.
    pub strip: bool,
}

impl Printer {
    /// Prints the input read from `reader`, whose path is `path` (`None`
    /// for stdin).
    fn print(
        &self,
        reader: &mut impl BufRead,
        path: Option<&Path>,
        too_large: bool,
        out: &mut impl Write,
        lines: &mut LineState,
        report: &mut dyn FnMut(&Path, Problem),
    ) -> io::Result<()> {
        // `-v`, `-E` and `-T` change how the lines are shown, which cannot be
        // combined with highlighting. `Args::use_color` turns colors off with
        // them too, but the printer does not rely on that.
        // After a stall, highlighting another input could get stuck too, and
        // leave one more thread spinning (see `Highlighter::has_stalled`).
        let hl = self
            .highlighter
            .as_ref()
            .filter(|hl| !too_large && !self.vis.any() && !hl.has_stalled());
        if hl.is_none() && !self.strip {
            return write_plain(reader, out, lines, self.vis);
        }
        // A pipe may return only a few bytes per read, so the head is read in
        // full before anything is decided from it.
        let mut head = Vec::with_capacity(PEEK_LEN);
        reader
            .by_ref()
            .take(PEEK_LEN as u64)
            .read_to_end(&mut head)?;
        let utf16 = utf16_bom(&head);
        let binary = utf16.is_none() && (head.contains(&0) || !is_utf8_prefix(&head));
        let target = match hl.filter(|_| !binary) {
            Some(hl) => Some((hl, self.choose_syntax(hl, &head, utf16, path)?)),
            None => None,
        };
        let name = path.unwrap_or(Path::new("-"));
        let mut on_give_up = |reason: &GiveUp| report(name, Problem::CannotHighlight(reason));
        let mut head = Cursor::new(head);
        if let Some(endian) = utf16 {
            // Terminals expect UTF-8, so UTF-16 is converted for highlighting.
            // It is also converted to remove its escape sequences, because
            // dropping single bytes of UTF-16 would break it.
            head.set_position(2);
            let rest = head.chain(reader);
            let decoded = BufReader::new(Utf16Decoder::new(rest, endian));
            return match target {
                Some((hl, syntax)) => self.filtered(decoded, |r| {
                    self.highlight(hl, r, syntax, out, lines, &mut on_give_up)
                }),
                None => self.filtered(decoded, |mut r| write_plain(&mut r, out, lines, self.vis)),
            };
        }
        let rest = head.chain(reader);
        match target {
            Some((hl, syntax)) => self.filtered(rest, |r| {
                self.highlight(hl, r, syntax, out, lines, &mut on_give_up)
            }),
            None => self.filtered(rest, |mut r| write_plain(&mut r, out, lines, self.vis)),
        }
    }

    /// The first line of the input starting with `head`, as the syntax is
    /// detected from it: converted from UTF-16, without a UTF-8 BOM, and
    /// with the escape sequences removed when asked.
    fn first_line(&self, head: &[u8], utf16: Option<Endian>) -> io::Result<String> {
        let mut decoded = Vec::new();
        let head = match utf16 {
            Some(endian) => {
                Utf16Decoder::new(&head[2..], endian).read_to_end(&mut decoded)?;
                &decoded[..]
            }
            None => head.strip_prefix(UTF8_BOM).unwrap_or(head),
        };
        let line = head.split(|&b| b == b'\n').next().unwrap_or_default();
        let mut stripped = Vec::new();
        let line = if self.strip {
            StripEscapes::new(line).read_to_end(&mut stripped)?;
            &stripped[..]
        } else {
            line
        };
        Ok(String::from_utf8_lossy(line).into_owned())
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
        hl: &Highlighter,
        mut reader: &mut dyn BufRead,
        syntax: Syntax,
        out: &mut impl Write,
        lines: &mut LineState,
        on_give_up: &mut dyn FnMut(&GiveUp),
    ) -> io::Result<()> {
        hl.write(&mut reader, on_give_up, out, syntax, lines, self.limits)
    }

    /// Uses the forced syntax, or detects one from the path and the first
    /// line in `head`.
    fn choose_syntax(
        &self,
        hl: &Highlighter,
        head: &[u8],
        utf16: Option<Endian>,
        path: Option<&Path>,
    ) -> io::Result<Syntax> {
        if let Some(syntax) = &self.forced {
            return Ok(syntax.clone());
        }
        Ok(hl.detect(path, &self.first_line(head, utf16)?))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        sync::{Arc, LazyLock},
        time::{Duration, Instant},
    };

    use syntect::{highlighting::Theme, parsing::SyntaxSet};

    use super::*;
    use crate::{
        color::Depth,
        highlight::find_language,
        lines::Numbering,
        test_support::{Chunked, LOOPING_SYNTAX, custom_syntax},
    };

    fn plain() -> Printer {
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

    /// What `run` saw: the output, whether an input failed, and the paths
    /// of the inputs reported as unreadable.
    struct Run {
        out: String,
        input_failed: bool,
        unreadable: Vec<PathBuf>,
    }

    /// An input that opens but cannot be read, like a directory.
    struct Unreadable;

    impl Read for Unreadable {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::IsADirectory.into())
        }
    }

    /// The name of an input that opens as `Unreadable`.
    const DIRECTORY: &str = "dir";

    /// Runs `print_all` with `inputs` opened from `files`, a missing name
    /// failing to open and `DIRECTORY` failing to be read.
    fn run(
        inputs: &[PathBuf],
        files: &[(&str, &'static str)],
        lines: LineState,
        headers: Option<Headers>,
    ) -> Run {
        let mut out = TrackedWriter::new(Vec::new());
        let open = |path: &Path| {
            if path == Path::new(DIRECTORY) {
                return Ok(Input {
                    source: Box::new(Unreadable),
                    path: Some(Path::new(DIRECTORY)),
                    too_large: false,
                });
            }
            files
                .iter()
                .find(|(name, _)| path == Path::new(name))
                .map(|&(_, text)| input(text))
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        };
        let mut unreadable = Vec::new();
        let report = |path: &Path, problem: Problem| match problem {
            Problem::Unreadable(e) => {
                let expected = if path == Path::new(DIRECTORY) {
                    io::ErrorKind::IsADirectory
                } else {
                    io::ErrorKind::NotFound
                };
                assert_eq!(e.kind(), expected);
                unreadable.push(path.to_path_buf());
            }
            Problem::CannotHighlight(reason) => panic!("cannot highlight: {reason}"),
        };
        let outcome = print_all(inputs, open, &plain(), &mut out, lines, headers, report);
        assert!(outcome.write_error.is_none());
        Run {
            out: String::from_utf8(out.inner).unwrap(),
            input_failed: outcome.input_failed,
            unreadable,
        }
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn unreadable_input_is_skipped() {
        let lines = LineState::new(Numbering::All, false, None);
        let run = run(
            &paths(&["a", "missing", "b"]),
            &[("a", "one\n"), ("b", "two\n")],
            lines,
            None,
        );
        assert!(run.input_failed);
        assert_eq!(run.unreadable, paths(&["missing"]));
        assert_eq!(run.out, "     1\tone\n     2\ttwo\n");
    }

    #[test]
    fn headers_name_each_input() {
        let lines = LineState::new(Numbering::None, false, None);
        let run = run(
            &paths(&["a", "-"]),
            &[("a", "one\n"), ("-", "two\n")],
            lines,
            Some(Headers::new(false)),
        );
        assert!(!run.input_failed);
        assert_eq!(run.unreadable, Vec::<PathBuf>::new());
        assert_eq!(run.out, "==> a <==\none\n\n==> standard input <==\ntwo\n");
    }

    #[test]
    fn unopened_input_gets_no_header() {
        let lines = LineState::new(Numbering::None, false, None);
        let run = run(
            &paths(&["missing", "a"]),
            &[("a", "one\n")],
            lines,
            Some(Headers::new(true)),
        );
        assert_eq!(run.unreadable, paths(&["missing"]));
        assert_eq!(run.out, "\x1b[1m==> a <==\x1b[0m\none\n");
    }

    #[test]
    fn unreadable_input_gets_no_header() {
        let lines = LineState::new(Numbering::All, false, None);
        let run = run(
            &paths(&[DIRECTORY, "a"]),
            &[("a", "one\n")],
            lines,
            Some(Headers::new(false)),
        );
        assert!(run.input_failed);
        assert_eq!(run.unreadable, paths(&[DIRECTORY]));
        assert_eq!(run.out, "==> a <==\n     1\tone\n");
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
        let report = |path: &Path, problem: Problem| panic!("{}: {problem:?}", path.display());
        let outcome = print_all(&inputs, open, &plain(), &mut out, lines, None, report);
        assert!(!outcome.input_failed);
        let write_error = outcome.write_error;
        assert!(write_error.is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe));
        assert!(out.failed());
        assert_eq!(opened, 1);
    }

    #[test]
    fn stops_highlighting_inputs_after_a_stall() {
        // Each input would get stuck if it were highlighted, so a later one
        // that is would be reported and take another `timeout`. Only the
        // first leaves a thread spinning.
        let timeout = Duration::from_secs(1);
        let syntax = custom_syntax(LOOPING_SYNTAX);
        let highlighter =
            Highlighter::new(syntaxes(), Theme::default(), Depth::TrueColor, Vec::new());
        let printer = Printer {
            highlighter: Some(highlighter.with_stall_timeout(timeout)),
            forced: Some(syntax),
            ..plain()
        };
        let mut out = TrackedWriter::new(Vec::new());
        let lines = LineState::new(Numbering::None, false, None);
        let mut reasons = Vec::new();
        let report = |path: &Path, problem: Problem| match problem {
            Problem::CannotHighlight(reason) => reasons.push(reason.clone()),
            Problem::Unreadable(e) => panic!("{}: {e}", path.display()),
        };
        let start = Instant::now();
        let open = |_: &Path| Ok(input("x\n"));
        let outcome = print_all(
            &paths(&["a", "b", "c"]),
            open,
            &printer,
            &mut out,
            lines,
            None,
            report,
        );
        let elapsed = start.elapsed();
        assert!(!outcome.input_failed);
        assert!(outcome.write_error.is_none());
        assert_eq!(reasons, [GiveUp::Stalled(timeout)]);
        assert_eq!(out.inner, b"x\nx\nx\n");
        assert!(elapsed < timeout * 2, "took {elapsed:?}");
        let has_stalled = printer
            .highlighter
            .as_ref()
            .is_some_and(Highlighter::has_stalled);
        assert!(has_stalled);
    }

    /// The bundled syntaxes only, so the user's custom ones stay out. They
    /// are loaded once for all the tests, which is much faster.
    fn syntaxes() -> Arc<SyntaxSet> {
        static SYNTAXES: LazyLock<Arc<SyntaxSet>> =
            LazyLock::new(|| Arc::new(crate::assets::bundled_syntaxes()));
        Arc::clone(&SYNTAXES)
    }

    /// A printer that colors the output.
    fn colored() -> Printer {
        Printer {
            highlighter: Some(Highlighter::new(
                syntaxes(),
                Theme::default(),
                Depth::TrueColor,
                Vec::new(),
            )),
            ..plain()
        }
    }

    fn stripping(printer: Printer) -> Printer {
        Printer {
            strip: true,
            ..printer
        }
    }

    /// The output of `printer` for `bytes` read from `path`, which must
    /// cause no problem. The output is the same when `bytes` are read one
    /// byte at a time, as from a slow pipe.
    fn print(printer: &Printer, bytes: &[u8], path: &str, too_large: bool) -> Vec<u8> {
        let out = print_from(printer, &mut Cursor::new(bytes), path, too_large);
        let one_byte = print_one_byte(printer, bytes, path, too_large);
        assert_eq!(one_byte, out, "read one byte at a time");
        out
    }

    /// `print`, with `bytes` read one byte at a time only.
    fn print_one_byte(printer: &Printer, bytes: &[u8], path: &str, too_large: bool) -> Vec<u8> {
        let mut reader = BufReader::new(Chunked::one_byte(bytes));
        print_from(printer, &mut reader, path, too_large)
    }

    fn print_from(
        printer: &Printer,
        reader: &mut impl BufRead,
        path: &str,
        too_large: bool,
    ) -> Vec<u8> {
        let path = Path::new(path);
        let mut out = Vec::new();
        let mut lines = LineState::new(Numbering::None, false, None);
        let mut report = |path: &Path, problem: Problem| panic!("{}: {problem:?}", path.display());
        printer
            .print(
                reader,
                Some(path),
                too_large,
                &mut out,
                &mut lines,
                &mut report,
            )
            .unwrap();
        out
    }

    /// Removes SGR escape sequences, which must be terminated.
    fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while let Some((&b, tail)) = rest.split_first() {
            if let Some(csi) = rest.strip_prefix(b"\x1b[") {
                let end = csi
                    .iter()
                    .position(|&c| c == b'm')
                    .unwrap_or_else(|| panic!("unterminated CSI in {bytes:?}"));
                rest = &csi[end + 1..];
            } else {
                out.push(b);
                rest = tail;
            }
        }
        out
    }

    fn has_escape(bytes: &[u8]) -> bool {
        bytes.contains(&0x1b)
    }

    fn utf16(text: &str, endian: Endian) -> Vec<u8> {
        std::iter::once(0xFEFF)
            .chain(text.encode_utf16())
            .flat_map(|u| match endian {
                Endian::Little => u.to_le_bytes(),
                Endian::Big => u.to_be_bytes(),
            })
            .collect()
    }

    /// Text of `len` bytes, in lines of `x`, followed by `tail`.
    fn padded(len: usize, tail: &[u8]) -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..len)
            .map(|i| if i % 64 == 63 { b'\n' } else { b'x' })
            .collect();
        bytes.extend_from_slice(tail);
        bytes.extend_from_slice(b"\nfn main() {}\n");
        bytes
    }

    const RUST: &[u8] = b"fn main() {}\n";

    #[test]
    fn copies_the_input_without_color_or_stripping() {
        let bytes = b"a\x1b[31mb\n\x00\xff";
        assert_eq!(print(&plain(), bytes, "a.rs", false), bytes);
    }

    #[test]
    fn highlights_utf8() {
        let out = print(&colored(), RUST, "a.rs", false);
        assert!(has_escape(&out), "{out:?}");
        assert_eq!(strip_ansi(&out), RUST);
    }

    #[test]
    fn copies_input_with_a_nul_in_the_first_8_kb() {
        for at in [0, PEEK_LEN - 1] {
            let bytes = padded(at, b"\0");
            assert_eq!(
                print(&colored(), &bytes, "a.rs", false),
                bytes,
                "NUL at {at}"
            );
        }
    }

    #[test]
    fn copies_input_with_a_nul_on_the_second_line_read_byte_by_byte() {
        let bytes = b"fn main() {}\na\0b\n";
        assert_eq!(print_one_byte(&colored(), bytes, "a.rs", false), bytes);
    }

    #[test]
    fn highlights_input_with_a_nul_past_the_first_8_kb() {
        let bytes = padded(PEEK_LEN, b"\0");
        let out = print(&colored(), &bytes, "a.rs", false);
        assert!(has_escape(&out));
        assert_eq!(strip_ansi(&out), bytes);
    }

    #[test]
    fn copies_input_that_is_not_utf8_in_the_first_8_kb() {
        for at in [0, PEEK_LEN - 1] {
            let bytes = padded(at, b"\xff");
            assert_eq!(
                print(&colored(), &bytes, "a.rs", false),
                bytes,
                "\\xff at {at}"
            );
        }
    }

    #[test]
    fn highlights_input_with_a_character_cut_off_by_the_first_8_kb() {
        // "日" takes 3 bytes, so the first 8 KB end after 2 of them.
        let bytes = padded(PEEK_LEN - 2, "日".as_bytes());
        let out = print(&colored(), &bytes, "a.rs", false);
        assert!(has_escape(&out));
        assert_eq!(strip_ansi(&out), bytes);
    }

    #[test]
    fn copies_input_that_is_too_large() {
        assert_eq!(print(&colored(), RUST, "a.rs", true), RUST);
    }

    #[test]
    fn shows_nonprinting_like_cat_a_even_with_a_highlighter() {
        let printer = Printer {
            vis: Visibility {
                nonprinting: true,
                ends: true,
                tabs: true,
            },
            ..colored()
        };
        let out = print(&printer, b"a\x1b[31m\tb\n", "a.rs", false);
        assert_eq!(out, b"a^[[31m^Ib$\n");
    }

    #[test]
    fn highlights_utf16_as_utf8() {
        let expected = print(&colored(), RUST, "a.rs", false);
        for endian in [Endian::Little, Endian::Big] {
            let text = std::str::from_utf8(RUST).unwrap();
            let out = print(&colored(), &utf16(text, endian), "a.rs", false);
            assert_eq!(out, expected, "{endian:?}");
        }
    }

    #[test]
    fn detects_a_utf16_bom_split_across_reads() {
        let expected = print(&colored(), RUST, "a.rs", false);
        for endian in [Endian::Little, Endian::Big] {
            let text = std::str::from_utf8(RUST).unwrap();
            let bytes = utf16(text, endian);
            let out = print_one_byte(&colored(), &bytes, "a.rs", false);
            assert_eq!(out, expected, "{endian:?}");
        }
        let bytes = utf16("a\x1b[31mb\n", Endian::Little);
        let out = print_one_byte(&stripping(plain()), &bytes, "a.txt", false);
        assert_eq!(out, b"ab\n");
    }

    #[test]
    fn converts_utf16_to_strip_it_without_color() {
        let bytes = utf16("a\x1b[31mb\n", Endian::Little);
        assert_eq!(print(&stripping(plain()), &bytes, "a.txt", false), b"ab\n");
    }

    #[test]
    fn copies_utf16_without_color_or_stripping() {
        let bytes = utf16("a\n", Endian::Big);
        assert_eq!(print(&plain(), &bytes, "a.txt", false), bytes);
    }

    #[test]
    fn prints_nothing_for_a_utf16_bom_alone() {
        for printer in [colored(), stripping(plain())] {
            assert_eq!(print(&printer, b"\xFF\xFE", "a.txt", false), b"");
        }
    }

    #[test]
    fn copies_utf32_as_binary() {
        let bytes = b"\xFF\xFE\x00\x00a\x00\x00\x00\n\x00\x00\x00";
        assert_eq!(print(&colored(), bytes, "a.txt", false), bytes);
    }

    #[test]
    fn strips_escapes_from_plain_and_highlighted_output() {
        let text = b"fn \x1b]0;title\x07main() {}\x1b[31m\n";
        let binary = b"\x00\x1b[31mx\n";
        assert_eq!(print(&stripping(plain()), text, "a.rs", false), RUST);
        assert_eq!(
            print(&stripping(colored()), binary, "a.bin", false),
            b"\x00x\n"
        );
        let out = print(&stripping(colored()), text, "a.rs", false);
        assert_eq!(out, print(&colored(), RUST, "a.rs", false));
    }

    #[test]
    fn detects_a_shebang_split_across_reads() {
        let script = b"#!/usr/bin/env python3\nprint(1)\n";
        let python = Printer {
            forced: find_language(&syntaxes(), "py"),
            ..colored()
        };
        let expected = print(&python, script, "script", false);
        assert_ne!(expected, print(&colored(), script, "script.txt", false));
        assert_eq!(
            print_one_byte(&colored(), script, "script", false),
            expected
        );
    }

    #[test]
    fn detects_the_shebang_of_utf16_and_after_removing_escapes() {
        let python = Printer {
            forced: find_language(&syntaxes(), "py"),
            ..colored()
        };
        let expected = print(&python, b"#!/usr/bin/env python3\n", "script", false);
        let bytes = utf16("#!/usr/bin/env python3\n", Endian::Big);
        assert_eq!(print(&colored(), &bytes, "script", false), expected);
        let escaped = b"#!/usr/bin/env \x1b[1mpython3\n";
        let out = print(&stripping(colored()), escaped, "script", false);
        assert_eq!(out, expected);
    }

    #[test]
    fn forced_syntax_beats_the_path() {
        let rust = print(&colored(), RUST, "a.rs", false);
        assert_ne!(print(&colored(), RUST, "a.txt", false), rust);
        let forced = Printer {
            forced: find_language(&syntaxes(), "rs"),
            ..colored()
        };
        assert_eq!(print(&forced, RUST, "a.txt", false), rust);
    }
}
