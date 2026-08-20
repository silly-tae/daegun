use super::aat::Lookup;
use crate::daecore::daetype::decoder::{read_i16_be, read_u16_be, read_u32_be, window};

#[derive(Clone, Copy)]
pub struct Ankr<'a> {
    data: &'a [u8],
    lookup: Lookup<'a>,
    glyph_data: usize,
}

impl<'a> Ankr<'a> {
    pub fn parse(data: &'a [u8], num_glyphs: u16) -> Option<Self> {
        let h = window::<8>(data, 4)?;
        let lookup_at = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as usize;
        let glyph_data = u32::from_be_bytes([h[4], h[5], h[6], h[7]]) as usize;
        let lookup = Lookup::parse(data.get(lookup_at..)?, num_glyphs)?;
        Some(Ankr { data, lookup, glyph_data })
    }

    pub fn anchor_point(&self, glyph: u16, index: u16) -> Option<(i16, i16)> {
        let entry = self.lookup.value(glyph)? as usize;
        let at = self.glyph_data.checked_add(entry)?;
        let count = read_u32_be(self.data, at)?;
        if u32::from(index) >= count {
            return None;
        }
        let point = at.checked_add(4)?.checked_add(usize::from(index).checked_mul(4)?)?;
        let p = window::<4>(self.data, point)?;
        Some((i16::from_be_bytes([p[0], p[1]]), i16::from_be_bytes([p[2], p[3]])))
    }

    pub fn point_count(&self, glyph: u16) -> u32 {
        let Some(entry) = self.lookup.value(glyph) else { return 0 };
        let Some(at) = self.glyph_data.checked_add(entry as usize) else { return 0 };
        read_u32_be(self.data, at).unwrap_or(0)
    }
}

pub fn control_point(data: &[u8], at: usize) -> Option<(i16, i16)> {
    Some((read_i16_be(data, at)?, read_i16_be(data, at + 2)?))
}

pub fn version(data: &[u8]) -> Option<u16> {
    read_u16_be(data, 0)
}
