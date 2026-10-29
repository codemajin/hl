use std::io::{self, Write};

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
    at_line_start: bool,
    /// Whether the current line is being printed.
    printing: bool,
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
            at_line_start: true,
            printing: false,
        }
    }

    /// True when lines pass through without numbering, squeezing or a range.
    pub fn is_passthrough(&self) -> bool {
        self.numbering == Numbering::None && !self.squeeze && self.range.is_none()
    }

    /// True when the rest of the current input is past the line range, so
    /// it does not need to be read.
    pub fn is_done(&self) -> bool {
        self.at_line_start
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
        let starts_line = self.at_line_start;
        self.at_line_start = line.ends_with(b"\n");
        if !starts_line {
            return Ok(self.printing);
        }
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
                self.printing = false;
                return Ok(false);
            }
        }
        if blank && self.squeeze && self.prev_blank {
            self.printing = false;
            return Ok(false);
        }
        self.prev_blank = blank;
        self.printing = true;
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
        if !self.at_line_start && self.printing {
            out.write_all(b"\n")?;
        }
        self.at_line_start = true;
        self.prev_blank = false;
        Ok(())
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
