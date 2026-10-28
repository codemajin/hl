use std::io::{self, Read};

pub const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// How many bytes `Utf16Decoder` reads from the inner reader at a time.
const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bom {
    Utf8,
    Utf16 { big_endian: bool },
}

pub fn sniff_bom(head: &[u8]) -> Option<Bom> {
    if head.starts_with(UTF8_BOM) {
        Some(Bom::Utf8)
    } else if head.starts_with(b"\xFF\xFE") {
        Some(Bom::Utf16 { big_endian: false })
    } else if head.starts_with(b"\xFE\xFF") {
        Some(Bom::Utf16 { big_endian: true })
    } else {
        None
    }
}

/// True when `bytes` is valid UTF-8, allowing a character cut off at the end
/// (as happens at a buffer boundary).
pub fn is_utf8_prefix(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none(),
    }
}

/// Decodes UTF-16 from `inner` (with the BOM already skipped) into UTF-8.
/// Unpaired surrogates and a trailing odd byte become U+FFFD.
pub struct Utf16Decoder<R> {
    inner: R,
    big_endian: bool,
    /// Bytes read but not decoded yet: an odd byte or a high surrogate.
    raw: Vec<u8>,
    /// Decoded UTF-8 not returned yet, from `pos` on.
    out: Vec<u8>,
    pos: usize,
}

impl<R: Read> Utf16Decoder<R> {
    pub fn new(inner: R, big_endian: bool) -> Self {
        Self {
            inner,
            big_endian,
            raw: Vec::new(),
            out: Vec::new(),
            pos: 0,
        }
    }

    fn decode(&mut self, eof: bool) {
        let (pairs, _) = self.raw.as_chunks::<2>();
        let mut units: Vec<u16> = pairs
            .iter()
            .map(|&pair| {
                if self.big_endian {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            })
            .collect();
        // A high surrogate at the end may be paired by the next chunk.
        if !eof && units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
            units.pop();
        }
        let mut utf8 = [0; 4];
        for c in char::decode_utf16(units.iter().copied()) {
            let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
            self.out
                .extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
        }
        self.raw.drain(..units.len() * 2);
        if eof && !self.raw.is_empty() {
            self.out.extend_from_slice("\u{FFFD}".as_bytes());
            self.raw.clear();
        }
    }
}

impl<R: Read> Read for Utf16Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.out.len() {
            self.out.clear();
            self.pos = 0;
            let start = self.raw.len();
            self.raw.resize(start + CHUNK, 0);
            let read = self.inner.read(&mut self.raw[start..]);
            let n = *read.as_ref().unwrap_or(&0);
            self.raw.truncate(start + n);
            read?;
            if n == 0 && self.raw.is_empty() {
                return Ok(0);
            }
            self.decode(n == 0);
        }
        let n = buf.len().min(self.out.len() - self.pos);
        buf[..n].copy_from_slice(&self.out[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
