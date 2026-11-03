use std::io::{self, BufRead, Write};

use crate::color::RESET;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numbering {
    None,
    /// `-n`: number every output line.
    All,
    /// `-b`: number non-empty output lines.
    NonBlank,
}

/// Which characters `write_visible` makes visible (`cat -v`, `-E`, `-T`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Visibility {
    pub nonprinting: bool,
    pub ends: bool,
    pub tabs: bool,
}

impl Visibility {
    pub fn any(self) -> bool {
        self.nonprinting || self.ends || self.tabs
    }
}

/// The lines of each input to print (`--line-range`), 1-based and inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u64,
    /// `None` runs to the end of the input.
    pub end: Option<u64>,
}

impl LineRange {
    fn contains(self, line: u64) -> bool {
        line >= self.start && self.end.is_none_or(|end| line <= end)
    }
}

/// The lines of each input to show with a background (`--highlight-line`).
#[derive(Debug, Clone)]
pub struct LineHighlight {
    pub lines: Vec<LineRange>,
    /// The escape sequence that sets the background.
    pub sgr: String,
}

impl LineHighlight {
    fn contains(&self, line: u64) -> bool {
        self.lines.iter().any(|r| r.contains(line))
    }
}

/// Where the output is within the current line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    /// Before the first byte of a line.
    LineStart,
    /// Inside a line, which is being printed or skipped.
    InLine { printing: bool },
}

/// Line numbering and blank-line squeezing, carried across input files like
/// `cat` does (a file without a trailing newline continues into the next one).
///
/// With a line range, each input is counted on its own instead, and the line
/// numbers are those of the input, including the lines outside the range.
/// Highlighted lines are also those of each input, but leave the numbering
/// as it is.
pub struct LineState {
    numbering: Numbering,
    squeeze: bool,
    range: Option<LineRange>,
    highlight: Option<LineHighlight>,
    line_no: u64,
    /// Lines started in the current input.
    input_line: u64,
    prev_blank: bool,
    /// Whether the current line was started with the highlight background.
    highlighted: bool,
    cursor: Cursor,
}

impl LineState {
    pub fn new(numbering: Numbering, squeeze: bool, range: Option<LineRange>) -> Self {
        Self {
            numbering,
            squeeze,
            range,
            highlight: None,
            line_no: 0,
            input_line: 0,
            prev_blank: false,
            highlighted: false,
            cursor: Cursor::LineStart,
        }
    }

    /// Shows the lines of `highlight` with its background.
    pub fn with_highlight(mut self, highlight: LineHighlight) -> Self {
        self.highlight = Some(highlight);
        self
    }

    /// True when lines pass through without numbering, squeezing, a range or
    /// highlighting.
    pub fn is_passthrough(&self) -> bool {
        self.numbering == Numbering::None
            && !self.squeeze
            && self.range.is_none()
            && self.highlight.is_none()
    }

    /// True when the rest of the current input is past the line range, so
    /// it does not need to be read.
    pub fn is_done(&self) -> bool {
        self.cursor == Cursor::LineStart
            && self.range_end().is_some_and(|end| self.input_line >= end)
    }

    /// The last line of each input to print, if the line range has an end.
    pub fn range_end(&self) -> Option<u64> {
        self.range.and_then(|r| r.end)
    }

    /// Called before each input. With a line range, ends a line left
    /// unfinished by the previous input and restarts the counting, so that
    /// the range and the line numbers apply to each input on its own.
    /// Otherwise only the counting of the input lines restarts, which
    /// highlighted lines use; a line left unfinished continues as line 1 of
    /// the new input.
    pub fn start_input(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.range.is_some() {
            self.separate(out)?;
            self.line_no = 0;
            self.input_line = 0;
        } else {
            self.input_line = u64::from(self.cursor != Cursor::LineStart);
        }
        Ok(())
    }

    /// Called with each chunk of a line, the last of which ends with the
    /// newline (if the line has one). Writes the line number if one is due
    /// and returns whether the line should be printed.
    pub fn begin_line(&mut self, line: &[u8], out: &mut impl Write) -> io::Result<bool> {
        let printing = match self.cursor {
            Cursor::LineStart => self.start_line(line, out)?,
            Cursor::InLine { printing } => printing,
        };
        self.cursor = if line.ends_with(b"\n") {
            Cursor::LineStart
        } else {
            Cursor::InLine { printing }
        };
        Ok(printing)
    }

    /// Handles the first chunk of a line: writes the line number if one is
    /// due and returns whether the line should be printed.
    fn start_line(&mut self, line: &[u8], out: &mut impl Write) -> io::Result<bool> {
        let blank = line == b"\n";
        let numbered = match self.numbering {
            Numbering::None => false,
            Numbering::All => true,
            Numbering::NonBlank => !blank,
        };
        self.highlighted = false;
        self.input_line += 1;
        if let Some(range) = self.range {
            // Skipped and squeezed lines are counted too, to keep the
            // numbers of the input.
            if numbered {
                self.line_no += 1;
            }
            if !range.contains(self.input_line) {
                return Ok(false);
            }
        }
        if blank && self.squeeze && self.prev_blank {
            return Ok(false);
        }
        self.prev_blank = blank;
        if let Some(highlight) = &self.highlight
            && highlight.contains(self.input_line)
        {
            out.write_all(highlight.sgr.as_bytes())?;
            self.highlighted = true;
        }
        if numbered {
            if self.range.is_none() {
                self.line_no += 1;
            }
            write!(out, "{:>6}\t", self.line_no)?;
        }
        Ok(true)
    }

    /// Ends an unfinished line before something that is not part of the
    /// input (a header), so that it starts on its own line and squeezing
    /// does not reach across it.
    pub fn separate(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.cursor == (Cursor::InLine { printing: true }) {
            out.write_all(b"\n")?;
        }
        self.cursor = Cursor::LineStart;
        self.prev_blank = false;
        Ok(())
    }

    /// Called after input was copied without going through `begin_line`,
    /// with whether its last byte was a newline.
    fn passed_through(&mut self, ends_line: bool) {
        self.cursor = if ends_line {
            Cursor::LineStart
        } else {
            Cursor::InLine { printing: true }
        };
    }

    /// Writes a chunk of the current line as it is, ending the highlight
    /// background before the newline. A chunk without a newline leaves the
    /// background on for the rest of the line.
    pub fn write_raw(&self, line: &[u8], out: &mut impl Write) -> io::Result<()> {
        match line.strip_suffix(b"\n").filter(|_| self.highlighted) {
            Some(body) => {
                out.write_all(body)?;
                out.write_all(RESET.as_bytes())?;
                out.write_all(b"\n")
            }
            None => out.write_all(line),
        }
    }

    /// Called at the end of an input. Ends the highlight background of a
    /// line left unfinished by `write_raw`.
    pub fn end_input(&self, out: &mut impl Write) -> io::Result<()> {
        if self.highlighted && self.cursor == (Cursor::InLine { printing: true }) {
            out.write_all(RESET.as_bytes())?;
        }
        Ok(())
    }
}

/// The most bytes `write_plain` reads into memory at once. Longer lines
/// are handled in chunks, so memory does not grow with the line length.
const CHUNK: usize = 64 * 1024;

/// Copies `reader` without highlighting, applying numbering, squeezing, the
/// line range and `cat -v` style notation as requested.
pub fn write_plain(
    reader: &mut impl BufRead,
    out: &mut impl Write,
    lines: &mut LineState,
    vis: Visibility,
) -> io::Result<()> {
    write_plain_chunked(reader, out, lines, vis, CHUNK)
}

fn write_plain_chunked(
    reader: &mut impl BufRead,
    out: &mut impl Write,
    lines: &mut LineState,
    vis: Visibility,
    limit: usize,
) -> io::Result<()> {
    if lines.is_passthrough() && !vis.any() {
        return copy_through(reader, out, lines);
    }
    let mut buf = Vec::new();
    loop {
        if lines.is_done() {
            return Ok(());
        }
        buf.clear();
        if read_chunk(reader, &mut buf, limit)? == 0 {
            return lines.end_input(out);
        }
        if !lines.begin_line(&buf, out)? {
            continue;
        }
        if vis.any() {
            write_visible(&buf, vis, out)?;
        } else {
            lines.write_raw(&buf, out)?;
        }
    }
}

/// Copies `reader` as it is, like `io::copy`, and tells `lines` whether the
/// last byte ended a line, so that a header after it starts on its own line.
fn copy_through(
    reader: &mut impl BufRead,
    out: &mut impl Write,
    lines: &mut LineState,
) -> io::Result<()> {
    let mut last = None;
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let Some(&byte) = available.last() else {
            break;
        };
        out.write_all(available)?;
        let len = available.len();
        reader.consume(len);
        last = Some(byte);
    }
    if let Some(byte) = last {
        lines.passed_through(byte == b'\n');
    }
    Ok(())
}

/// Reads up to and including the next newline like `read_until(b'\n')`, but
/// stops after `limit` bytes. A CRLF is not split: when the chunk would end
/// between `\r` and `\n`, the `\n` is taken too, so `-E` still sees the
/// line ending as a whole. Returns the number of bytes read.
fn read_chunk(reader: &mut impl BufRead, buf: &mut Vec<u8>, limit: usize) -> io::Result<usize> {
    debug_assert!(limit > 0, "a zero limit reads nothing");
    let mut read = 0;
    while read < limit {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok(read);
        }
        let room = &available[..available.len().min(limit - read)];
        let (n, newline) = match room.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (room.len(), false),
        };
        buf.extend_from_slice(&room[..n]);
        reader.consume(n);
        read += n;
        if newline {
            return Ok(read);
        }
    }
    if buf.ends_with(b"\r") {
        let next = loop {
            match reader.fill_buf() {
                Ok(available) => break available.first().copied(),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        };
        if next == Some(b'\n') {
            buf.push(b'\n');
            reader.consume(1);
            read += 1;
        }
    }
    Ok(read)
}

/// Writes one line using `cat -v` / `-E` / `-T` notation.
pub fn write_visible(line: &[u8], vis: Visibility, out: &mut impl Write) -> io::Result<()> {
    let (body, newline) = match line.strip_suffix(b"\n") {
        Some(body) => (body, true),
        None => (line, false),
    };
    // Like coreutils 9.x, `-E` shows a CRLF ending as `^M$` even without `-v`.
    let (body, cr) = match body.strip_suffix(b"\r") {
        Some(rest) if vis.ends && newline => (rest, true),
        _ => (body, false),
    };
    for &b in body {
        match b {
            b'\t' if vis.tabs => out.write_all(b"^I")?,
            b'\t' => out.write_all(b"\t")?,
            _ if vis.nonprinting => write_nonprinting(b, out)?,
            _ => out.write_all(&[b])?,
        }
    }
    if cr {
        out.write_all(b"^M")?;
    }
    if newline {
        if vis.ends {
            out.write_all(b"$")?;
        }
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn write_nonprinting(b: u8, out: &mut impl Write) -> io::Result<()> {
    let (meta, c) = if b >= 0x80 {
        (true, b - 0x80)
    } else {
        (false, b)
    };
    if meta {
        out.write_all(b"M-")?;
    }
    match c {
        0x00..=0x1f => out.write_all(&[b'^', c + 0x40]),
        0x7f => out.write_all(b"^?"),
        _ => out.write_all(&[c]),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn plain(input: &[u8], lines: &mut LineState, vis: Visibility) -> Vec<u8> {
        let mut out = Vec::new();
        write_plain(&mut Cursor::new(input), &mut out, lines, vis).unwrap();
        out
    }

    fn numbered(numbering: Numbering, range: Option<LineRange>) -> LineState {
        LineState::new(numbering, false, range)
    }

    #[test]
    fn passes_bytes_through_unchanged() {
        let input = b"a\n\xffb";
        let mut lines = numbered(Numbering::None, None);
        assert_eq!(plain(input, &mut lines, Visibility::default()), input);
    }

    #[test]
    fn ends_a_line_left_open_by_passthrough_before_a_header() {
        let mut lines = numbered(Numbering::None, None);
        let mut out = plain(b"a\nb", &mut lines, Visibility::default());
        lines.separate(&mut out).unwrap();
        assert_eq!(out, b"a\nb\n");
    }

    #[test]
    fn adds_nothing_after_a_finished_or_empty_passthrough() {
        for input in [&b"a\n"[..], b""] {
            let mut lines = numbered(Numbering::None, None);
            let mut out = plain(input, &mut lines, Visibility::default());
            lines.separate(&mut out).unwrap();
            assert_eq!(out, input, "input {input:?}");
        }
    }

    #[test]
    fn writes_nothing_for_empty_input() {
        let mut lines = numbered(Numbering::All, None);
        assert_eq!(plain(b"", &mut lines, Visibility::default()), b"");
    }

    #[test]
    fn numbers_every_line_and_keeps_a_missing_newline() {
        let mut lines = numbered(Numbering::All, None);
        assert_eq!(
            plain(b"a\n\nb", &mut lines, Visibility::default()),
            b"     1\ta\n     2\t\n     3\tb"
        );
    }

    #[test]
    fn numbers_only_non_blank_lines() {
        let mut lines = numbered(Numbering::NonBlank, None);
        assert_eq!(
            plain(b"a\n\nb\n", &mut lines, Visibility::default()),
            b"     1\ta\n\n     2\tb\n"
        );
    }

    #[test]
    fn squeezes_blank_lines() {
        let mut lines = LineState::new(Numbering::None, true, None);
        assert_eq!(
            plain(b"a\n\n\n\nb\n", &mut lines, Visibility::default()),
            b"a\n\nb\n"
        );
    }

    #[test]
    fn stops_reading_after_the_line_range() {
        let range = LineRange {
            start: 2,
            end: Some(3),
        };
        let mut lines = numbered(Numbering::All, Some(range));
        let mut reader = Cursor::new(&b"a\nb\nc\nd\n"[..]);
        let mut out = Vec::new();
        write_plain(&mut reader, &mut out, &mut lines, Visibility::default()).unwrap();
        assert_eq!(out, b"     2\tb\n     3\tc\n");
        assert_eq!(reader.position(), 6, "read past the range");
    }

    #[test]
    fn prints_an_open_line_range_to_the_end() {
        let range = LineRange {
            start: 3,
            end: None,
        };
        let mut lines = numbered(Numbering::All, Some(range));
        assert_eq!(
            plain(b"a\nb\nc\nd\n", &mut lines, Visibility::default()),
            b"     3\tc\n     4\td\n"
        );
    }

    #[test]
    fn continues_an_unfinished_line_into_the_next_input() {
        let mut lines = numbered(Numbering::All, None);
        let mut out = plain(b"a", &mut lines, Visibility::default());
        lines.start_input(&mut out).unwrap();
        out.extend(plain(b"b\n", &mut lines, Visibility::default()));
        assert_eq!(out, b"     1\tab\n");
    }

    fn highlight(lines: &[LineRange]) -> LineHighlight {
        LineHighlight {
            lines: lines.to_vec(),
            sgr: "<bg>".to_string(),
        }
    }

    fn line(n: u64) -> LineRange {
        LineRange {
            start: n,
            end: Some(n),
        }
    }

    #[test]
    fn highlights_the_line_and_its_number() {
        let mut lines = numbered(Numbering::All, None).with_highlight(highlight(&[line(2)]));
        assert_eq!(
            plain(b"a\nb\nc", &mut lines, Visibility::default()),
            b"     1\ta\n<bg>     2\tb\x1b[0m\n     3\tc"
        );
    }

    #[test]
    fn ends_the_highlight_on_a_line_without_newline() {
        let mut lines = numbered(Numbering::None, None).with_highlight(highlight(&[line(1)]));
        assert_eq!(
            plain(b"a", &mut lines, Visibility::default()),
            b"<bg>a\x1b[0m"
        );
    }

    #[test]
    fn resets_once_after_the_last_chunk_of_a_line() {
        let mut lines = numbered(Numbering::None, None).with_highlight(highlight(&[line(1)]));
        let mut out = Vec::new();
        for chunk in [&b"bb"[..], b"bb", b"b\n"] {
            assert!(lines.begin_line(chunk, &mut out).unwrap());
            lines.write_raw(chunk, &mut out).unwrap();
        }
        assert_eq!(out, b"<bg>bbbbb\x1b[0m\n");
    }

    #[test]
    fn resets_at_the_end_of_an_input_without_newline() {
        let mut lines = numbered(Numbering::None, None).with_highlight(highlight(&[line(1)]));
        let mut out = Vec::new();
        for chunk in [&b"bb"[..], b"bb"] {
            assert!(lines.begin_line(chunk, &mut out).unwrap());
            lines.write_raw(chunk, &mut out).unwrap();
        }
        lines.end_input(&mut out).unwrap();
        assert_eq!(out, b"<bg>bbbb\x1b[0m");
    }

    #[test]
    fn does_not_reset_at_the_end_of_an_input_without_highlight() {
        let mut lines = numbered(Numbering::None, None).with_highlight(highlight(&[line(2)]));
        let mut out = Vec::new();
        assert!(lines.begin_line(b"a", &mut out).unwrap());
        lines.write_raw(b"a", &mut out).unwrap();
        lines.end_input(&mut out).unwrap();
        assert_eq!(out, b"a");
    }

    #[test]
    fn does_not_highlight_skipped_or_squeezed_lines() {
        let range = LineRange {
            start: 2,
            end: None,
        };
        let squeezed = LineRange {
            start: 3,
            end: Some(4),
        };
        let mut lines = LineState::new(Numbering::None, true, Some(range))
            .with_highlight(highlight(&[line(1), squeezed]));
        assert_eq!(
            plain(b"a\n\n\n\n", &mut lines, Visibility::default()),
            b"\n"
        );
    }

    #[test]
    fn highlights_the_lines_of_each_input() {
        let mut lines = numbered(Numbering::All, None).with_highlight(highlight(&[line(2)]));
        let mut out = Vec::new();
        for input in [&b"a\nb\n"[..], b"c\nd\n"] {
            lines.start_input(&mut out).unwrap();
            out.extend(plain(input, &mut lines, Visibility::default()));
        }
        assert_eq!(
            out,
            b"     1\ta\n<bg>     2\tb\x1b[0m\n     3\tc\n<bg>     4\td\x1b[0m\n"
        );
    }

    #[test]
    fn counts_an_unfinished_line_as_line_1_of_the_next_input() {
        let mut lines = numbered(Numbering::None, None).with_highlight(highlight(&[line(2)]));
        let mut out = Vec::new();
        for input in [&b"a"[..], b"b\nc\n"] {
            lines.start_input(&mut out).unwrap();
            out.extend(plain(input, &mut lines, Visibility::default()));
        }
        assert_eq!(out, b"ab\n<bg>c\x1b[0m\n");
    }

    #[test]
    fn shows_line_ends_without_adding_a_newline() {
        let vis = Visibility {
            ends: true,
            ..Visibility::default()
        };
        let mut lines = numbered(Numbering::None, None);
        assert_eq!(plain(b"a\r\nb", &mut lines, vis), b"a^M$\nb");
    }
    fn chunked(input: &[u8], lines: &mut LineState, vis: Visibility, limit: usize) -> Vec<u8> {
        let mut out = Vec::new();
        write_plain_chunked(&mut Cursor::new(input), &mut out, lines, vis, limit).unwrap();
        out
    }

    /// Checks that reading in chunks of every small size gives the same
    /// output as reading each line at once.
    fn assert_same_in_chunks(input: &[u8], new_lines: impl Fn() -> LineState, vis: Visibility) {
        let whole = chunked(input, &mut new_lines(), vis, usize::MAX);
        for limit in 1..=5 {
            assert_eq!(
                chunked(input, &mut new_lines(), vis, limit),
                whole,
                "chunks of {limit} bytes"
            );
        }
    }

    const LONG_LINES: &[u8] = b"abcdefghij\n\n\n\nab\tc\x01\xffdefgh\r\nxy\r\r\nlast line";

    #[test]
    fn numbers_long_lines_read_in_chunks() {
        assert_same_in_chunks(
            LONG_LINES,
            || numbered(Numbering::All, None),
            Visibility::default(),
        );
    }

    #[test]
    fn squeezes_and_numbers_non_blank_long_lines_read_in_chunks() {
        assert_same_in_chunks(
            LONG_LINES,
            || LineState::new(Numbering::NonBlank, true, None),
            Visibility::default(),
        );
    }

    #[test]
    fn shows_nonprinting_characters_of_long_lines_read_in_chunks() {
        let vis = Visibility {
            nonprinting: true,
            ends: true,
            tabs: true,
        };
        assert_same_in_chunks(LONG_LINES, || numbered(Numbering::None, None), vis);
    }

    #[test]
    fn shows_crlf_endings_split_by_chunks() {
        let vis = Visibility {
            ends: true,
            ..Visibility::default()
        };
        assert_same_in_chunks(LONG_LINES, || numbered(Numbering::None, None), vis);
        assert_eq!(
            chunked(b"abc\r\n", &mut numbered(Numbering::None, None), vis, 4),
            b"abc^M$\n"
        );
    }

    #[test]
    fn highlights_long_lines_read_in_chunks() {
        for input in [LONG_LINES, b"abcdefghij\n"] {
            assert_same_in_chunks(
                input,
                || {
                    numbered(Numbering::All, None).with_highlight(highlight(&[
                        line(1),
                        line(5),
                        line(7),
                    ]))
                },
                Visibility::default(),
            );
        }
    }

    #[test]
    fn applies_the_line_range_to_long_lines_read_in_chunks() {
        let range = LineRange {
            start: 2,
            end: Some(5),
        };
        assert_same_in_chunks(
            LONG_LINES,
            || numbered(Numbering::All, Some(range)),
            Visibility::default(),
        );
    }

    #[test]
    fn reads_a_chunk_up_to_the_limit_or_the_newline() {
        let mut reader = Cursor::new(&b"abcdef\ngh"[..]);
        let mut chunks = Vec::new();
        loop {
            let mut buf = Vec::new();
            if read_chunk(&mut reader, &mut buf, 4).unwrap() == 0 {
                break;
            }
            chunks.push(buf);
        }
        assert_eq!(chunks, [&b"abcd"[..], b"ef\n", b"gh"]);
    }

    #[test]
    fn does_not_split_a_crlf_between_chunks() {
        let mut reader = Cursor::new(&b"abc\r\nd\r"[..]);
        let mut buf = Vec::new();
        assert_eq!(read_chunk(&mut reader, &mut buf, 4).unwrap(), 5);
        assert_eq!(buf, b"abc\r\n");
        buf.clear();
        assert_eq!(read_chunk(&mut reader, &mut buf, 2).unwrap(), 2);
        assert_eq!(buf, b"d\r");
    }
}
