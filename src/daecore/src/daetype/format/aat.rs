use alloc::vec::Vec;
use crate::daecore::daetype::decoder::{read_u16_be, read_u32_be, window};

pub mod class {
    pub const END_OF_TEXT: u16 = 0;
    pub(crate) const OUT_OF_BOUNDS: u16 = 1;
    pub const DELETED_GLYPH: u16 = 2;
}

pub mod state {
    pub const START_OF_TEXT: u16 = 0;
}

#[derive(Clone, Copy)]
pub struct Lookup<'a> {
    data: &'a [u8],
    format: u16,
    num_glyphs: u16,
    bin_srch: Option<(usize, usize)>,
}

impl<'a> Lookup<'a> {
    pub fn parse(data: &'a [u8], num_glyphs: u16) -> Option<Self> {
        let format = read_u16_be(data, 0)?;
        if !matches!(format, 0 | 2 | 4 | 6 | 8 | 10) {
            return None;
        }
        let bin_srch = parse_bin_srch(data);
        Some(Lookup { data, format, num_glyphs, bin_srch })
    }

    pub fn value(&self, glyph: u16) -> Option<u16> {
        match self.format {
            // Format 0 is an array with no length of its own – it runs to the end of the font's
            // glyph range, so without this bound a glyph past that reads whatever bytes follow.
            0 if glyph < self.num_glyphs => read_u16_be(self.data, 2 + 2 * usize::from(glyph)),
            0 => None,
            2 => self.segment_single(glyph),
            4 => self.segment_array(glyph),
            6 => self.single_table(glyph),
            8 => self.trimmed_array(glyph),
            10 => self.trimmed_array_wide(glyph),
            _ => None,
        }
    }

    pub fn entries(&self) -> Vec<(u16, u16)> {
        let mut out = Vec::new();
        let push = |g: u16, out: &mut Vec<(u16, u16)>| {
            if let Some(v) = self.value(g) { out.push((g, v)); }
        };
        match self.format {
            0 => for g in 0..self.num_glyphs { push(g, &mut out); },
            2 | 4 | 6 => {
                let Some((unit, n)) = self.bin_srch() else { return out };
                for i in 0..n {
                    let rec = 12 + i * unit;
                    let (first, last) = if self.format == 6 {
                        let Some(g) = read_u16_be(self.data, rec) else { break };
                        (g, g)
                    } else {
                        let (Some(last), Some(first)) =
                            (read_u16_be(self.data, rec), read_u16_be(self.data, rec + 2))
                        else { break };
                        (first, last)
                    };
                    if last == 0xFFFF || first > last { continue; }
                    for g in first..=last { push(g, &mut out); }
                }
            }
            8 | 10 => {
                let at = if self.format == 8 { 2 } else { 4 };
                let (Some(first), Some(count)) =
                    (read_u16_be(self.data, at), read_u16_be(self.data, at + 2)) else { return out };
                for i in 0..count {
                    let Some(g) = first.checked_add(i) else { break };
                    push(g, &mut out);
                }
            }
            _ => {}
        }
        out
    }

    fn bin_srch(&self) -> Option<(usize, usize)> {
        self.bin_srch
    }

    fn lower_bound(&self, glyph: u16, unit: usize, n: usize) -> Option<usize> {
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if read_u16_be(self.data, 12 + mid * unit)? < glyph {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        (lo < n).then(|| 12 + lo * unit)
    }

    fn segment_single(&self, glyph: u16) -> Option<u16> {
        let (unit, n) = self.bin_srch()?;
        if unit < 6 {
            return None;
        }
        let at = self.lower_bound(glyph, unit, n)?;
        let seg = window::<4>(self.data, at)?;
        let last = u16::from_be_bytes([seg[0], seg[1]]);
        let first = u16::from_be_bytes([seg[2], seg[3]]);
        if first == 0xFFFF && last == 0xFFFF {
            return None;
        }
        (first <= glyph).then(|| read_u16_be(self.data, at + 4))?
    }

    fn segment_array(&self, glyph: u16) -> Option<u16> {
        let (unit, n) = self.bin_srch()?;
        if unit < 6 {
            return None;
        }
        let at = self.lower_bound(glyph, unit, n)?;
        let seg = window::<4>(self.data, at)?;
        let last = u16::from_be_bytes([seg[0], seg[1]]);
        let first = u16::from_be_bytes([seg[2], seg[3]]);
        if first == 0xFFFF && last == 0xFFFF {
            return None;
        }
        if first > glyph {
            return None;
        }
        let values = usize::from(read_u16_be(self.data, at + 4)?);
        read_u16_be(self.data, values + 2 * usize::from(glyph - first))
    }

    fn single_table(&self, glyph: u16) -> Option<u16> {
        let (unit, n) = self.bin_srch()?;
        if unit < 4 {
            return None;
        }
        let at = self.lower_bound(glyph, unit, n)?;
        let g = read_u16_be(self.data, at)?;
        if g != glyph || g == 0xFFFF {
            return None;
        }
        read_u16_be(self.data, at + 2)
    }

    fn trimmed_array(&self, glyph: u16) -> Option<u16> {
        let h = window::<4>(self.data, 2)?;
        let first = u16::from_be_bytes([h[0], h[1]]);
        let count = u16::from_be_bytes([h[2], h[3]]);
        let i = glyph.checked_sub(first)?;
        (i < count).then(|| read_u16_be(self.data, 6 + 2 * usize::from(i)))?
    }

    fn trimmed_array_wide(&self, glyph: u16) -> Option<u16> {
        let h = window::<6>(self.data, 2)?;
        let unit = u16::from_be_bytes([h[0], h[1]]);
        let first = u16::from_be_bytes([h[2], h[3]]);
        let count = u16::from_be_bytes([h[4], h[5]]);
        let i = glyph.checked_sub(first)?;
        if i >= count {
            return None;
        }
        let at = 8 + usize::from(i) * usize::from(unit);
        match unit {
            1 => self.data.get(at).map(|b| u16::from(*b)),
            2 => read_u16_be(self.data, at),
            4 => read_u32_be(self.data, at).map(|v| v as u16),
            _ => None,
        }
    }
}

fn parse_bin_srch(data: &[u8]) -> Option<(usize, usize)> {
    let h = window::<4>(data, 2)?;
    let unit = usize::from(u16::from_be_bytes([h[0], h[1]]));
    let declared = usize::from(u16::from_be_bytes([h[2], h[3]]));
    if unit == 0 {
        return None;
    }
    let fits = data.len().saturating_sub(12) / unit;
    Some((unit, declared.min(fits)))
}

#[derive(Clone, Copy, Default)]
pub struct Entry {
    pub new_state: u16,
    pub flags: u16,
    pub word1: u16,
    pub word2: u16,
}

pub struct StateTable<'a> {
    class_lookup: Lookup<'a>,
    state_array: &'a [u8],
    entry_table: &'a [u8],
    n_classes: usize,
    extra_words: usize,
}

impl<'a> StateTable<'a> {
    pub fn parse(data: &'a [u8], extra_words: usize, num_glyphs: u16) -> Option<Self> {
        let h = window::<16>(data, 0)?;
        let word = |i: usize| u32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]) as usize;
        let (n_classes, class_off, state_off, entry_off) = (word(0), word(4), word(8), word(12));
        if n_classes == 0 || n_classes > 0xFFFF {
            return None;
        }
        Some(StateTable {
            class_lookup: Lookup::parse(data.get(class_off..)?, num_glyphs)?,
            state_array: data.get(state_off..)?,
            entry_table: data.get(entry_off..)?,
            n_classes,
            extra_words,
        })
    }

    pub fn class(&self, glyph: u16) -> u16 {
        if glyph == 0xFFFF {
            return class::DELETED_GLYPH;
        }
        self.class_lookup.value(glyph).unwrap_or(class::OUT_OF_BOUNDS)
    }

    pub fn entry(&self, state: u16, klass: u16) -> Option<Entry> {
        let klass = usize::from(klass);
        if klass >= self.n_classes {
            return None;
        }
        let cell = usize::from(state).checked_mul(self.n_classes)?.checked_add(klass)?.checked_mul(2)?;
        let index = read_u16_be(self.state_array, cell)?;
        let at = usize::from(index).checked_mul(4 + 2 * self.extra_words)?;
        let e = window::<4>(self.entry_table, at)?;
        Some(Entry {
            new_state: u16::from_be_bytes([e[0], e[1]]),
            flags: u16::from_be_bytes([e[2], e[3]]),
            word1: if self.extra_words >= 1 { read_u16_be(self.entry_table, at + 4)? } else { 0 },
            word2: if self.extra_words >= 2 { read_u16_be(self.entry_table, at + 6)? } else { 0 },
        })
    }
}
