//! Helpers shared by the unit tests.

use std::io::{self, Read};

use proptest::{collection::vec, prelude::*};

/// Returns `data` in reads of the sizes in `sizes`, taken in turn and
/// repeated, so that the reader under test sees its input split at those
/// places. A size of 1 splits every sequence and character across reads.
pub struct Chunked<'a> {
    data: &'a [u8],
    sizes: Vec<usize>,
    next: usize,
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
        }
    }

    /// One byte per read.
    pub fn one_byte(data: &'a [u8]) -> Self {
        Self::new(data, vec![1])
    }
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
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
