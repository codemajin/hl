use std::io::{self, BufRead, Write};

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
pub struct LineState {
    numbering: Numbering,
    squeeze: bool,
    range: Option<LineRange>,
    line_no: u64,
    /// Lines started in the current input, counted only with a range.
    input_line: u64,
    prev_blank: bool,
    cursor: Cursor,
}

impl LineState {
    pub fn new(numbering: Numbering, squeeze: bool, range: Option<LineRange>) -> Self {
        Self {
            numbering,
            squeeze,
            range,
            line_no: 0,
            input_line: 0,
            prev_blank: false,
            cursor: Cursor::LineStart,
        }
    }

    /// True when lines pass through without numbering, squeezing or a range.
    pub fn is_passthrough(&self) -> bool {
        self.numbering == Numbering::None && !self.squeeze && self.range.is_none()
    }

    /// True when the rest of the current input is past the line range, so
    /// it does not need to be read.
    pub fn is_done(&self) -> bool {
        self.cursor == Cursor::LineStart
            && self
                .range
                .and_then(|r| r.end)
                .is_some_and(|end| self.input_line >= end)
    }

    /// Called before each input. With a line range, ends a line left
    /// unfinished by the previous input and restarts the counting, so that
    /// the range and the line numbers apply to each input on its own.
    pub fn start_input(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.range.is_none() {
            return Ok(());
        }
        self.separate(out)?;
        self.line_no = 0;
        self.input_line = 0;
        Ok(())
    }

    /// Called with each chunk from `read_until(b'\n')`. Writes the line
    /// number if one is due and returns whether the line should be printed.
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
        if let Some(range) = self.range {
            self.input_line += 1;
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
}

/// Copies `reader` without highlighting, applying numbering, squeezing, the
/// line range and `cat -v` style notation as requested.
pub fn write_plain(
    reader: &mut impl BufRead,
    out: &mut impl Write,
    lines: &mut LineState,
    vis: Visibility,
) -> io::Result<()> {
    if lines.is_passthrough() && !vis.any() {
        io::copy(reader, out)?;
        return Ok(());
    }
    let mut buf = Vec::new();
    loop {
        if lines.is_done() {
            return Ok(());
        }
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        if !lines.begin_line(&buf, out)? {
            continue;
        }
        if vis.any() {
            write_visible(&buf, vis, out)?;
        } else {
            out.write_all(&buf)?;
        }
    }
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

    #[test]
    fn shows_line_ends_without_adding_a_newline() {
        let vis = Visibility {
            ends: true,
            ..Visibility::default()
        };
        let mut lines = numbered(Numbering::None, None);
        assert_eq!(plain(b"a\r\nb", &mut lines, vis), b"a^M$\nb");
    }
}
