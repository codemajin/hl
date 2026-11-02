//! Removing the escape sequences of the input, so that they neither mix
//! with the colors of `hl` nor control the terminal (window title,
//! hyperlinks, clipboard).

use std::io::{self, BufRead, Read};

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// The first byte of U+0080..=U+009F, the C1 controls, in UTF-8.
const C1_LEAD: u8 = 0xc2;

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
    /// After `0xC2`, which may be the start of a C1 control.
    C1,
    /// After `0xC2` inside a string, which may be the start of ST (U+009C).
    StrC1,
}

/// Reads `inner` with its escape sequences removed: CSI, the strings (OSC,
/// DCS, SOS, PM, APC) ended by BEL or ST, and the other sequences starting
/// with ESC. The C1 controls encoded in UTF-8 (U+0080..=U+009F) are removed
/// too, and the 8-bit CSI and strings among them start a sequence like their
/// 7-bit forms. A sequence split across reads is still removed.
///
/// A string also ends at a newline, which is kept, so that an unterminated
/// one does not swallow the rest of the input. A sequence cut off by the end
/// of the input is dropped. Only ASCII bytes and whole C1 characters are
/// removed, apart from the whole content of a string, so valid UTF-8 stays
/// valid.
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
        (State::Ground, C1_LEAD) => State::C1,
        #[expect(
            clippy::match_same_arms,
            reason = "the ground state keeps every byte; a string keeps only the newline that ends it"
        )]
        (State::Ground, _) => {
            out.push(b);
            State::Ground
        }
        (State::Esc | State::StrEsc, _) => after_esc(state, b, out),
        (State::C1, _) => after_c1_lead(b, out),
        (State::EscIntermediate, 0x20..=0x2f) | (State::Csi, 0x20..=0x3f) => state,
        (State::EscIntermediate, 0x30..=0x7e) | (State::Csi, 0x40..=0x7e) => State::Ground,
        // A byte that cannot be part of the sequence ends it and is kept,
        // unless it starts another one.
        (State::EscIntermediate | State::Csi, _) => feed(State::Ground, b, out),
        (State::Str | State::StrC1, b'\n') => {
            out.push(b);
            State::Ground
        }
        (State::StrC1, 0x9c) | (State::Str | State::StrC1, BEL) => State::Ground,
        (State::Str | State::StrC1, ESC) => State::StrEsc,
        (State::Str | State::StrC1, C1_LEAD) => State::StrC1,
        (State::Str | State::StrC1, _) => State::Str,
    }
}

/// The state after `0xC2 b` outside a string. A C1 control is dropped, and
/// any other character is kept.
fn after_c1_lead(b: u8, out: &mut Vec<u8>) -> State {
    match b {
        // CSI
        0x9b => State::Csi,
        // DCS, SOS, OSC, PM, APC
        0x90 | 0x98 | 0x9d | 0x9e | 0x9f => State::Str,
        0x80..=0x9f => State::Ground,
        _ => {
            out.push(C1_LEAD);
            feed(State::Ground, b, out)
        }
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
        _ => feed(State::Ground, b, out),
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
                // A lone 0xC2 is not a sequence, so it is kept like the
                // other bytes that are not valid UTF-8.
                if self.state == State::C1 {
                    self.buf.push(C1_LEAD);
                    self.state = State::Ground;
                }
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
    fn removes_c1_controls() {
        assert_strips(b"a\xc2\x85b\xc2\x80\xc2\x9c\n", b"ab\n");
        assert_strips(b"\xc2\xc2\x84a", b"\xc2a");
    }

    #[test]
    fn removes_8_bit_csi_and_strings() {
        assert_strips(b"a\xc2\x9b31mred\n", b"ared\n");
        assert_strips(b"\xc2\x9d0;title\xc2\x9cx", b"x");
        assert_strips(b"\xc2\x9d0;title\x07x", b"x");
        assert_strips(b"\xc2\x9d0;title\x1b\\x", b"x");
        assert_strips(b"\xc2\x90q\xc2\x9cx\xc2\x9fapc\xc2\x9cy", b"xy");
        assert_strips(b"\x1b]0;t\xc2\x9cz", b"z");
        assert_strips("\u{9d}2;日本語\u{a0}\u{9c}x".as_bytes(), b"x");
        assert_strips(b"\xc2\x9d0;title\nb", b"\nb");
    }

    #[test]
    fn c1_starts_a_new_sequence() {
        assert_strips(b"\x1b[3\xc2\x9b1mb", b"b");
        assert_strips(b"\x1b(\xc2\x9b1mc", b"c");
        assert_strips(b"\x1b\xc2\x9b1md", b"d");
    }

    #[test]
    fn keeps_other_characters_starting_with_0xc2() {
        assert_strips("\u{a0}é©\n".as_bytes(), "\u{a0}é©\n".as_bytes());
        assert_strips("\x1b[3\u{a0}".as_bytes(), "\u{a0}".as_bytes());
        assert_strips(b"\xc2a", b"\xc2a");
        assert_strips(b"a\xc2", b"a\xc2");
        assert_strips(b"\xc2\x1b[1mb", b"\xc2b");
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
