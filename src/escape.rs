//! Removing the escape sequences and control characters of the input, so
//! that they neither mix with the colors of `hl` nor control the terminal
//! (window title, hyperlinks, clipboard, character sets).

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
/// 7-bit forms. So are the C0 controls (BEL, BS, SO, ...) other than NUL,
/// tab, newline and carriage return, which `cat` users rely on to overwrite
/// a line; NUL and DEL do nothing on a terminal and are kept, so binary
/// input keeps its shape. A sequence split across reads is still removed.
///
/// Bytes in `0x80..=0x9F` that are not part of a C1 control in UTF-8 are
/// kept: they are the lead bytes of `Shift_JIS` and the like, and do nothing
/// on a UTF-8 terminal. A terminal that does not read UTF-8 (Latin-1, for
/// example) takes them for C1 controls.
///
/// A string also ends at a newline, which is kept, so that an unterminated
/// one does not swallow the rest of the input. A sequence cut off by the end
/// of the input is dropped. Only ASCII bytes and whole C1 characters are
/// removed, apart from the whole content of a string, so valid UTF-8 stays
/// valid.
///
/// A lone `0xC2`, which is not valid UTF-8, is kept. When a sequence removed
/// after it would leave it next to a byte in `0x80..=0x9F`, the two would make
/// a C1 control, so that byte is taken as one: it is removed, or starts a
/// sequence.
pub struct StripEscapes<R> {
    inner: R,
    state: State,
    /// Filtered bytes not returned yet, from `pos` on.
    out: Output,
    pos: usize,
}

/// The filtered bytes, and whether the last byte kept is a lone `0xC2`.
#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    /// The last byte kept, which may already have been returned, is a
    /// `0xC2` that did not start a C1 control.
    lone_lead: bool,
}

impl Output {
    fn push(&mut self, b: u8) {
        self.bytes.push(b);
        self.lone_lead = false;
    }

    fn push_lone_lead(&mut self) {
        self.bytes.push(C1_LEAD);
        self.lone_lead = true;
    }
}

impl<R: BufRead> StripEscapes<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            state: State::Ground,
            out: Output::default(),
            pos: 0,
        }
    }
}

/// Feeds `b` to the state machine in `state`, keeping it in `out` if it is
/// not part of a sequence, and returns the next state.
fn feed(state: State, b: u8, out: &mut Output) -> State {
    match (state, b) {
        (State::Ground | State::EscIntermediate | State::Csi, ESC) => State::Esc,
        (State::Ground, C1_LEAD) => State::C1,
        // With the lone 0xC2 kept before it, this byte makes a C1 control.
        (State::Ground, 0x80..=0x9f) if out.lone_lead => after_c1_lead(b, out),
        (State::Ground, 0x01..=0x1f) if !matches!(b, b'\t' | b'\n' | b'\r') => State::Ground,
        #[expect(
            clippy::match_same_arms,
            reason = "the ground state keeps every other byte; a string keeps only the newline that ends it"
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
        // unless it starts another one or is a control removed.
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
fn after_c1_lead(b: u8, out: &mut Output) -> State {
    match b {
        // CSI
        0x9b => State::Csi,
        // DCS, SOS, OSC, PM, APC
        0x90 | 0x98 | 0x9d | 0x9e | 0x9f => State::Str,
        0x80..=0x9f => State::Ground,
        _ => {
            out.push_lone_lead();
            feed(State::Ground, b, out)
        }
    }
}

/// The state after `ESC b`. Inside a string, anything but `\` ends the
/// string and starts a new sequence, as with xterm.
fn after_esc(state: State, b: u8, out: &mut Output) -> State {
    match b {
        b'\\' if state == State::StrEsc => State::Ground,
        b'[' => State::Csi,
        b']' | b'P' | b'X' | b'^' | b'_' => State::Str,
        0x20..=0x2f => State::EscIntermediate,
        0x30..=0x7e => State::Ground,
        ESC => State::Esc,
        // A lone ESC is dropped and the byte after it is fed again.
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
        while self.pos == self.out.bytes.len() {
            self.out.bytes.clear();
            self.pos = 0;
            let chunk = self.inner.fill_buf()?;
            if chunk.is_empty() {
                // A lone 0xC2 is not a sequence, so it is kept like the
                // other bytes that are not valid UTF-8.
                if self.state == State::C1 {
                    self.out.push_lone_lead();
                    self.state = State::Ground;
                }
                break;
            }
            for &b in chunk {
                self.state = feed(self.state, b, &mut self.out);
            }
            let n = chunk.len();
            self.inner.consume(n);
        }
        Ok(&self.out.bytes[self.pos..])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = (self.pos + amt).min(self.out.bytes.len());
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use proptest::{collection::vec, prelude::*};

    use super::*;
    use crate::test_support::{Chunked, chunk_sizes};

    fn strip(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        StripEscapes::new(input).read_to_end(&mut out).unwrap();
        out
    }

    /// Checks `input` both in one read and one byte at a time.
    fn assert_strips(input: &[u8], expected: &[u8]) {
        assert_eq!(strip(input), expected, "{input:?}");
        let mut out = Vec::new();
        StripEscapes::new(BufReader::new(Chunked::one_byte(input)))
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, expected, "{input:?} one byte at a time");
    }

    #[test]
    fn keeps_text_without_escapes() {
        assert_strips(
            "a\tb\r\n日本語\x00\x7f\n".as_bytes(),
            "a\tb\r\n日本語\x00\x7f\n".as_bytes(),
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
    fn removes_c1_controls_joined_by_a_removed_sequence() {
        assert_strips(b"\xc2\x1b[m\x9b31mX", b"\xc2X");
        assert_strips(b"\xc2\x1b\x85X", b"\xc2X");
        assert_strips(b"\xc2\x1b]t\x07\x9d0;title\x07X", b"\xc2X");
        assert_strips(b"\xc2\x1b(B\x85\x85X", b"\xc2X");
        assert_strips(b"\xc2\xc2\x85\x1b[m\x85X", b"\xc2X");
        // Not a C1 control, so it is kept.
        assert_strips(b"\xc2\x1b[m\xa0X", b"\xc2\xa0X");
        assert_strips(b"\xc2\x1b[mY\x85", b"\xc2Y\x85");
    }

    #[test]
    fn removes_c0_controls() {
        assert_strips(b"a\x07b\x0e\x0f\x08\x0b\x0c\x05\x01\x1fc\n", b"abc\n");
        assert_strips(b"a\x1b[3\x0e1mx", b"a1mx");
        assert_strips(b"\x1b\x08y\x1b(\x07z", b"yz");
    }

    #[test]
    fn keeps_raw_bytes_in_the_c1_range() {
        // Not UTF-8: "日本" in Shift_JIS, and a raw 8-bit CSI.
        assert_strips(b"\x93\xfa\x96\x7b\n", b"\x93\xfa\x96\x7b\n");
        assert_strips(b"a\x9b31mb\x9d0;t\x07c", b"a\x9b31mb\x9d0;tc");
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
        let mut reader =
            StripEscapes::new(BufReader::new(Chunked::one_byte(b"\x1b[1mab\n\x1b[0mc\n")));
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line).unwrap();
        assert_eq!(line, b"ab\n");
        line.clear();
        reader.read_until(b'\n', &mut line).unwrap();
        assert_eq!(line, b"c\n");
    }

    /// Reads `input` through `StripEscapes`, split into reads of `sizes`.
    fn strip_chunked(input: &[u8], sizes: Vec<usize>) -> Vec<u8> {
        let mut out = Vec::new();
        StripEscapes::new(BufReader::new(Chunked::new(input, sizes)))
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    /// Bytes weighted toward those that start, continue or end a sequence.
    fn interesting_byte() -> impl Strategy<Value = u8> {
        prop_oneof![
            3 => Just(ESC),
            2 => Just(C1_LEAD),
            2 => 0x80..=0x9fu8,
            1 => Just(BEL),
            1 => Just(b'\n'),
            2 => prop::sample::select(&b"[]\\PX^_(#;0m"[..]),
            2 => 0x20..=0x7eu8,
            1 => 0x00..=0x1fu8,
            2 => any::<u8>(),
        ]
    }

    /// Text weighted toward the characters that start, continue or end a
    /// sequence, including the C1 controls.
    fn interesting_text() -> impl Strategy<Value = String> {
        const CHARS: &[char] = &[
            '\x1b', '\u{9b}', '\u{9c}', '\u{9d}', '\u{85}', '\x07', '\n', '[', ']', '\\', 'P', ';',
            'm', 'a', '\u{a0}', 'é', '日', '😀', '\x0e', '\x00', '\r',
        ];
        let c = prop_oneof![4 => prop::sample::select(CHARS), 1 => any::<char>()];
        vec(c, 0..256).prop_map(String::from_iter)
    }

    /// Whether `bytes` holds a C0 control that the filter removes.
    fn has_c0(bytes: &[u8]) -> bool {
        bytes
            .iter()
            .any(|&b| b <= 0x1f && !matches!(b, 0 | b'\t' | b'\n' | b'\r'))
    }

    /// Whether `bytes` holds a C1 control in UTF-8.
    fn has_c1(bytes: &[u8]) -> bool {
        bytes
            .windows(2)
            .any(|w| w[0] == C1_LEAD && (0x80..=0x9f).contains(&w[1]))
    }

    proptest! {
        #[test]
        fn chunk_boundaries_do_not_change_the_output(
            input in vec(interesting_byte(), 0..256),
            sizes in chunk_sizes(),
        ) {
            prop_assert_eq!(strip_chunked(&input, sizes), strip(&input));
        }

        #[test]
        fn leaves_no_control_characters(input in vec(interesting_byte(), 0..256)) {
            let out = strip(&input);
            prop_assert!(!has_c0(&out), "{:?} -> {:?}", input, out);
            prop_assert!(!has_c1(&out), "{:?} -> {:?}", input, out);
        }

        #[test]
        fn valid_utf8_stays_valid(text in interesting_text(), sizes in chunk_sizes()) {
            let out = strip_chunked(text.as_bytes(), sizes);
            prop_assert!(std::str::from_utf8(&out).is_ok(), "{:?} -> {:?}", text, out);
        }
    }
}
