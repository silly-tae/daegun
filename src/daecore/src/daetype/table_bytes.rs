use alloc::vec::Vec;
use core::ops::Deref;

use crate::daecore::sync::Shared;

#[derive(Clone)]
// A font is one buffer and every table is a window onto it: extracting nineteen separate Vec<u8>
// copied the whole file at open, 94% of `Font::from_bytes` and running at memory bandwidth, so it
// could only be avoided rather than made faster. Cloning is a refcount.
pub struct TableBytes {
    buf: Shared<Vec<u8>>,
    start: usize,
    len: usize,
}

impl TableBytes {
    pub fn from_vec(bytes: Vec<u8>) -> TableBytes {
        let len = bytes.len();
        TableBytes { buf: Shared::new(bytes), start: 0, len }
    }

    pub fn slice(buf: &Shared<Vec<u8>>, start: usize, len: usize) -> Option<TableBytes> {
        let end = start.checked_add(len)?;
        if end > buf.len() {
            return None;
        }
        Some(TableBytes { buf: Shared::clone(buf), start, len })
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf[self.start..self.start + self.len]
    }

    pub fn to_owned_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }
}

// `Deref<Target = [u8]>` is what kept this from being a rewrite: the hundred-odd places that read a
// table still want `&[u8]`, and only the places that *build* one had to change.
impl Deref for TableBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsRef<[u8]> for TableBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl From<Vec<u8>> for TableBytes {
    fn from(bytes: Vec<u8>) -> TableBytes {
        TableBytes::from_vec(bytes)
    }
}

impl core::fmt::Debug for TableBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TableBytes({} bytes)", self.len)
    }
}

impl PartialEq for TableBytes {
    fn eq(&self, other: &TableBytes) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for TableBytes {}

impl Default for TableBytes {
    fn default() -> TableBytes {
        TableBytes::from_vec(Vec::new())
    }
}
