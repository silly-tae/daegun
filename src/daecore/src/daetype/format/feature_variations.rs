use super::super::decoder::{read_i16_be, read_u16_be, read_u32_be};

pub struct FeatureVariations<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> FeatureVariations<'a> {
    pub fn parse(layout: &'a [u8]) -> Option<Self> {
        if layout.len() < 14 || read_u16_be(layout, 2)? < 1 {
            return None;
        }
        match read_u32_be(layout, 10)? {
            0 => None,
            off => Some(Self { data: layout, at: off as usize }),
        }
    }

    pub fn at(layout: &'a [u8], at: usize) -> Self {
        Self { data: layout, at }
    }

    pub fn find(&self, coords: &[i32]) -> Option<u16> {
        let count = read_u32_be(self.data, self.at + 4)?;
        for i in 0..count {
            let rec = self.at + 8 + i as usize * 8;
            let cond_off = read_u32_be(self.data, rec)? as usize;
            if cond_off != 0 && self.condition_set_matches(self.at + cond_off, coords) {
                return u16::try_from(i).ok();
            }
        }
        None
    }

    fn condition_set_matches(&self, at: usize, coords: &[i32]) -> bool {
        let Some(count) = read_u16_be(self.data, at) else { return false };
        (0..count).all(|i| {
            read_u32_be(self.data, at + 2 + i as usize * 4)
                .is_some_and(|rel| self.condition_matches(at + rel as usize, coords))
        })
    }

    fn condition_matches(&self, at: usize, coords: &[i32]) -> bool {
        if read_u16_be(self.data, at) != Some(1) {
            return false;
        }
        let (Some(axis), Some(min), Some(max)) = (
            read_u16_be(self.data, at + 2),
            read_i16_be(self.data, at + 4),
            read_i16_be(self.data, at + 6),
        ) else {
            return false;
        };
        let v = coords.get(axis as usize).copied().unwrap_or(0);
        v >= min as i32 && v <= max as i32
    }

    pub fn substitute(&self, variation: u16, feature: u16) -> Option<usize> {
        let rec = self.at + 8 + variation as usize * 8;
        let subst_off = read_u32_be(self.data, rec + 4)? as usize;
        if subst_off == 0 {
            return None;
        }
        let at = self.at + subst_off;
        let count = read_u16_be(self.data, at + 4)?;
        for i in 0..count {
            let r = at + 6 + i as usize * 6;
            if read_u16_be(self.data, r)? == feature {
                let rel = read_u32_be(self.data, r + 2)? as usize;
                return (rel != 0).then_some(at + rel);
            }
        }
        None
    }
}
