//! Removing the escape sequences of the input, so that they neither mix
//! with the colors of `hl` nor control the terminal (window title,
//! hyperlinks, clipboard).

use std::io::{self, BufRead, Read};

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// Where the filter is within an escape sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Outside any sequence.
    Ground,
    /// After `ESC`.
    Esc,
    /// After `ESC` and intermediate bytes, as in `ESC ( B`.
    EscIntermediate,
    /// Inside a CSI sequence (`ESC [`).
    Csi,
    /// Inside a string: OSC, DCS, SOS, PM or APC.
    Str,
    /// After `ESC` inside a string, which may be the start of ST (`ESC \`).
    StrEsc,
}

/// Reads `inner` with its escape sequences removed: CSI, the strings (OSC,
/// DCS, SOS, PM, APC) ended by BEL or ST, and the other sequences starting
/// with ESC. A sequence split across reads is still removed.
///
/// A string also ends at a newline, which is kept, so that an unterminated
/// one does not swallow the rest of the input. A sequence cut off by the end
/// of the input is dropped. Only ASCII bytes are removed, apart from the
/// whole content of a string, so valid UTF-8 stays valid.
pub struct StripEscapes<R> {
    inner: R,
    state: State,
    /// Filtered bytes not returned yet, from `pos` on.
    buf: Vec<u8>,
    pos: usize,
}

impl<R: BufRead> StripEscapes<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            state: State::Ground,
            buf: Vec::new(),
            pos: 0,
        }
    }
}

/// Feeds `b` to the state machine in `state`, keeping it in `out` if it is
/// not part of a sequence, and returns the next state.
fn feed(state: State, b: u8, out: &mut Vec<u8>) -> State {
    match (state, b) {
        (State::Ground | State::EscIntermediate | State::Csi, ESC) => State::Esc,
        (State::Ground, _) => {
            out.push(b);
            State::Ground
        }
        (State::Esc | State::StrEsc, _) => after_esc(state, b, out),
        (State::EscIntermediate, 0x20..=0x2f) | (State::Csi, 0x20..=0x3f) => state,
        (State::EscIntermediate, 0x30..=0x7e) | (State::Csi, 0x40..=0x7e) => State::Ground,
        // A byte that cannot be part of the sequence ends it and is kept.
        (State::EscIntermediate | State::Csi, _) | (State::Str, b'\n') => {
            out.push(b);
            State::Ground
        }
        (State::Str, BEL) => State::Ground,
        (State::Str, ESC) => State::StrEsc,
        (State::Str, _) => State::Str,
    }
}

/// The state after `ESC b`. Inside a string, anything but `\` ends the
/// string and starts a new sequence, as with xterm.
fn after_esc(state: State, b: u8, out: &mut Vec<u8>) -> State {
    match b {
        b'\\' if state == State::StrEsc => State::Ground,
        b'[' => State::Csi,
        b']' | b'P' | b'X' | b'^' | b'_' => State::Str,
        0x20..=0x2f => State::EscIntermediate,
        0x30..=0x7e => State::Ground,
        ESC => State::Esc,
        // A lone ESC is dropped and the byte after it is kept.
        _ => {
            out.push(b);
            State::Ground
        }
    }
}

impl<R: BufRead> Read for StripEscapes<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl<R: BufRead> BufRead for StripEscapes<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        // A chunk can be all escape sequences, so read until something is
        // left or the input ends.
        while self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
            let chunk = self.inner.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            for &b in chunk {
                self.state = feed(self.state, b, &mut self.buf);
            }
            let n = chunk.len();
            self.inner.consume(n);
        }
        Ok(&self.buf[self.pos..])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = (self.pos + amt).min(self.buf.len());
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    /// Returns one byte per `read`, so every sequence is split across reads.
    struct OneByte<'a>(&'a [u8]);

    impl Read for OneByte<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.0.len().min(1).min(buf.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    fn strip(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        StripEscapes::new(input).read_to_end(&mut out).unwrap();
        out
    }

    /// Checks `input` both in one read and one byte at a time.
    fn assert_strips(input: &[u8], expected: &[u8]) {
        assert_eq!(strip(input), expected, "{input:?}");
        let mut out = Vec::new();
        StripEscapes::new(BufReader::new(OneByte(input)))
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, expected, "{input:?} one byte at a time");
    }

    #[test]
    fn keeps_text_without_escapes() {
        assert_strips(
            "a\tb\r\n日本語\x07\n".as_bytes(),
            "a\tb\r\n日本語\x07\n".as_bytes(),
        );
        assert_strips(b"", b"");
    }

    #[test]
    fn removes_csi_sequences() {
        assert_strips(b"a\x1b[31mred\x1b[0m\n", b"ared\n");
        assert_strips(b"\x1b[?25lx\x1b[2;5H\x1b[38;2;1;2;3my", b"xy");
        assert_strips(b"\x1b[1 qz", b"z");
    }

    #[test]
    fn removes_osc_ended_by_bel_or_st() {
        assert_strips(b"a\x1b]0;title\x07b\n", b"ab\n");
        assert_strips(
            b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\\n",
            b"link\n",
        );
        assert_strips("\x1b]2;日本語\x07x".as_bytes(), b"x");
    }

    #[test]
    fn removes_other_strings() {
        assert_strips(b"\x1bP1$r0m\x1b\\a", b"a");
        assert_strips(b"\x1b_apc\x1b\\\x1b^pm\x1b\\\x1bXsos\x1b\\b", b"b");
    }

    #[test]
    fn removes_two_byte_and_intermediate_sequences() {
        assert_strips(b"\x1bca\x1b7b\x1b8\x1bM", b"ab");
        assert_strips(b"\x1b(Bx\x1b#8y", b"xy");
    }

    #[test]
    fn drops_a_sequence_cut_off_at_the_end() {
        assert_strips(b"a\x1b", b"a");
        assert_strips(b"a\x1b[31", b"a");
        assert_strips(b"a\x1b]0;tit", b"a");
        assert_strips(b"a\x1b]0;title\x1b", b"a");
        assert_strips(b"a\x1b(", b"a");
    }

    #[test]
    fn a_newline_ends_an_unfinished_sequence() {
        assert_strips(b"a\x1b[31\nb\n", b"a\nb\n");
        assert_strips(b"a\x1b]0;title\nb\n", b"a\nb\n");
        assert_strips(b"a\x1b\nb", b"a\nb");
    }

    #[test]
    fn keeps_non_ascii_after_a_broken_sequence() {
        assert_strips("\x1b[3日".as_bytes(), "日".as_bytes());
        assert_strips("\x1b日".as_bytes(), "日".as_bytes());
    }

    #[test]
    fn esc_starts_a_new_sequence() {
        assert_strips(b"\x1b\x1b[31ma", b"a");
        assert_strips(b"\x1b[3\x1b[1mb", b"b");
        assert_strips(b"\x1b]0;title\x1b[31mc", b"c");
        assert_strips(b"\x1b(\x1b[1md", b"d");
    }

    #[test]
    fn reads_across_chunks_with_bufread() {
        let mut reader = StripEscapes::new(BufReader::new(OneByte(b"\x1b[1mab\n\x1b[0mc\n")));
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line).unwrap();
        assert_eq!(line, b"ab\n");
        line.clear();
        reader.read_until(b'\n', &mut line).unwrap();
        assert_eq!(line, b"c\n");
    }
}
