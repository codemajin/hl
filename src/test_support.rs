//! Helpers shared by the unit tests.

use std::{
    io::{self, Read},
    sync::{Arc, LazyLock},
};

use proptest::{collection::vec, prelude::*};
use syntect::parsing::{SyntaxDefinition, SyntaxSet, SyntaxSetBuilder};

use crate::{
    encoding::Endian,
    highlight::{Syntax, find_language},
};

/// Returns `data` in reads of the sizes in `sizes`, taken in turn and
/// repeated, so that the reader under test sees its input split at those
/// places. A size of 1 splits every sequence and character across reads.
/// With `fail_at`, chosen reads return an error instead.
pub struct Chunked<'a> {
    data: &'a [u8],
    sizes: Vec<usize>,
    next: usize,
    /// Calls to `read` so far, failed ones included.
    calls: usize,
    /// The calls to `read` that fail, and with what.
    errors: Vec<(usize, io::ErrorKind)>,
}

impl<'a> Chunked<'a> {
    /// `sizes` must not be empty, and a size of 0 is taken as 1, so that a
    /// read returns 0 only at the end of `data`.
    pub fn new(data: &'a [u8], sizes: Vec<usize>) -> Self {
        assert!(!sizes.is_empty(), "no chunk sizes");
        Self {
            data,
            sizes,
            next: 0,
            calls: 0,
            errors: Vec::new(),
        }
    }

    /// Makes call `call` to `read` (counted from 0, failed ones included)
    /// return an error of `kind`. That call reads nothing and does not use
    /// up a chunk size, so the input is split at the same places as without
    /// it. Can be given more than once.
    pub fn fail_at(mut self, call: usize, kind: io::ErrorKind) -> Self {
        self.errors.push((call, kind));
        self
    }

    /// One byte per read.
    pub fn one_byte(data: &'a [u8]) -> Self {
        Self::new(data, vec![1])
    }
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let call = self.calls;
        self.calls += 1;
        if let Some(&(_, kind)) = self.errors.iter().find(|&&(c, _)| c == call) {
            return Err(kind.into());
        }
        let size = self.sizes[self.next % self.sizes.len()].max(1);
        self.next += 1;
        let n = self.data.len().min(size).min(buf.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

/// Chunk sizes for `Chunked`: mostly small, so that the input is split in
/// many places, and sometimes large.
pub fn chunk_sizes() -> impl Strategy<Value = Vec<usize>> {
    vec(prop_oneof![9 => 1..=16usize, 1 => 17..=4096usize], 1..8)
}

/// A syntax whose two contexts hand `x` back and forth without consuming
/// it, which makes syntect loop forever.
///
/// Every input highlighted with it leaves a thread spinning until the test
/// process exits, which slows down the other tests and can make the ones
/// that wait for a deadline fail on a CI runner with few cores. So only the
/// tests of the stall itself use it, each with as few stuck inputs as it
/// can.
pub const LOOPING_SYNTAX: &str = r"%YAML 1.2
---
name: Loop
scope: source.loop
contexts:
  main:
    - match: (?=x)
      set: other
  other:
    - match: (?=x)
      set: main
";

/// The syntax defined by `yaml`, in a syntax set of its own.
pub fn custom_syntax(yaml: &str) -> Syntax {
    let definition = SyntaxDefinition::load_from_str(yaml, true, None).unwrap();
    let name = definition.name.clone();
    let mut builder = SyntaxSetBuilder::new();
    builder.add(definition);
    let set: Arc<SyntaxSet> = Arc::new(builder.build());
    find_language(&set, &name).unwrap()
}

/// The bundled syntaxes, as `hl` uses them without custom ones, so that the
/// user's custom syntaxes stay out. They are loaded once for all the tests,
/// which is much faster.
pub fn bundled() -> Arc<SyntaxSet> {
    static SYNTAXES: LazyLock<Arc<SyntaxSet>> =
        LazyLock::new(|| Arc::new(crate::assets::bundled_syntaxes()));
    Arc::clone(&SYNTAXES)
}

/// Removes SGR escape sequences (`ESC [ ... m`). Panics on one without its
/// `m`, which `hl` never writes, rather than dropping the rest. Any other
/// escape is kept, so that a test comparing the result sees it.
pub fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
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

/// Whether `bytes` contain the start of an escape sequence (`ESC [`).
pub fn has_escape(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|w| w == b"\x1b[")
}

/// `text` in UTF-16 with the byte order of `endian`. No BOM is added: start
/// `text` with U+FEFF for one.
pub fn utf16(text: &str, endian: Endian) -> Vec<u8> {
    utf16_units(text.encode_utf16(), endian)
}

/// The UTF-16 code `units` in the byte order of `endian`, which can hold
/// lone surrogates that no `&str` can.
pub fn utf16_units(units: impl IntoIterator<Item = u16>, endian: Endian) -> Vec<u8> {
    units
        .into_iter()
        .flat_map(|u| match endian {
            Endian::Little => u.to_le_bytes(),
            Endian::Big => u.to_be_bytes(),
        })
        .collect()
}
