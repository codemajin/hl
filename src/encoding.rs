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
            let n = match self.inner.read(&mut self.raw[start..]) {
                Ok(n) => n,
                Err(e) => {
                    self.raw.truncate(start);
                    return Err(e);
                }
            };
            self.raw.truncate(start + n);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns one byte per `read`, so every unit is split across reads.
    struct OneByte<'a>(&'a [u8]);

    impl Read for OneByte<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.0.len().min(1).min(buf.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|u| {
                if big_endian {
                    u.to_be_bytes()
                } else {
                    u.to_le_bytes()
                }
            })
            .collect()
    }

    fn decode(input: impl Read, big_endian: bool) -> String {
        let mut out = String::new();
        Utf16Decoder::new(input, big_endian)
            .read_to_string(&mut out)
            .unwrap();
        out
    }

    const TEXT: &str = "fn main() {} // 😀 日本語\n";

    #[test]
    fn decodes_whole_chunks() {
        for big_endian in [false, true] {
            assert_eq!(decode(&utf16(TEXT, big_endian)[..], big_endian), TEXT);
        }
    }

    #[test]
    fn decodes_units_and_surrogate_pairs_split_across_reads() {
        for big_endian in [false, true] {
            let bytes = utf16(TEXT, big_endian);
            assert_eq!(decode(OneByte(&bytes), big_endian), TEXT);
        }
    }

    #[test]
    fn replaces_a_trailing_high_surrogate() {
        let mut bytes = utf16("a", false);
        bytes.extend_from_slice(&0xD83Du16.to_le_bytes());
        assert_eq!(decode(OneByte(&bytes), false), "a\u{FFFD}");
    }

    #[test]
    fn replaces_a_trailing_odd_byte() {
        let mut bytes = utf16("a", false);
        bytes.push(b'b');
        assert_eq!(decode(OneByte(&bytes), false), "a\u{FFFD}");
    }

    #[test]
    fn replaces_an_unpaired_low_surrogate() {
        let mut bytes = 0xDE00u16.to_le_bytes().to_vec();
        bytes.extend(utf16("a", false));
        assert_eq!(decode(&bytes[..], false), "\u{FFFD}a");
    }
}
