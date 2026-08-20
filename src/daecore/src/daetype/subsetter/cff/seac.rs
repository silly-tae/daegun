use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use crate::daecore::daetype::decoder::read_u16_be;
use super::parse::{walk_charset, CharsetFlow};

pub(crate) fn standard_encoding_sid(code: u8) -> u16 {
    match code {
        32..=126 => (code - 31) as u16,
        161 => 96, 162 => 97, 163 => 98, 164 => 99, 165 => 100, 166 => 101,
        167 => 102, 168 => 103, 169 => 104, 170 => 105, 171 => 106, 172 => 107,
        173 => 108, 174 => 109, 175 => 110, 177 => 111, 178 => 112, 179 => 113,
        180 => 114, 182 => 115, 183 => 116, 184 => 117, 185 => 118, 186 => 119,
        187 => 120, 188 => 121, 189 => 122, 191 => 123, 193 => 124, 194 => 125,
        195 => 126, 196 => 127, 197 => 128, 198 => 129, 199 => 130, 200 => 131,
        202 => 132, 203 => 133, 205 => 134, 206 => 135, 207 => 136, 208 => 137,
        225 => 138, 227 => 139, 232 => 140, 233 => 141, 234 => 142, 235 => 143,
        241 => 144, 245 => 145, 248 => 146, 249 => 147, 250 => 148, 251 => 149,
        _ => 0,
    }
}

struct Ring<'a> { last: &'a mut [i32; 4], n: &'a mut usize }

impl Ring<'_> {
    fn push(&mut self, v: i32) {
        self.last[*self.n & 3] = v;
        *self.n += 1;
    }
    fn len(&self) -> usize { *self.n }
    fn nth_from_end(&self, k: usize) -> i32 { self.last[(*self.n - 1 - k) & 3] }
}

fn seac_operands(cs: &[u8]) -> Option<(i32, i32, u8, u8)> {
    let mut last = [0i32; 4];
    let mut n = 0usize;
    let mut stack = Ring { last: &mut last, n: &mut n };
    let mut pos = 0usize;
    let mut steps = 0usize;
    while pos < cs.len() {
        steps += 1;
        if steps > cs.len() { break; }
        let b0 = cs[pos];
        match b0 {
            32..=246 => { stack.push(b0 as i32 - 139); pos += 1; }
            247..=250 => {
                let b1 = *cs.get(pos + 1)? as i32;
                stack.push((b0 as i32 - 247) * 256 + b1 + 108);
                pos += 2;
            }
            251..=254 => {
                let b1 = *cs.get(pos + 1)? as i32;
                stack.push(-(b0 as i32 - 251) * 256 - b1 - 108);
                pos += 2;
            }
            28 => {
                let v = read_u16_be(cs, pos + 1)? as i16 as i32;
                stack.push(v);
                pos += 3;
            }
            255 => {
                let v = read_u16_be(cs, pos + 1)? as i16 as i32;
                stack.push(v);
                pos += 5;
            }
            14 => {
                if stack.len() >= 4 {
                    let achar = stack.nth_from_end(0);
                    let bchar = stack.nth_from_end(1);
                    let ady = stack.nth_from_end(2);
                    let adx = stack.nth_from_end(3);
                    if (0..=255).contains(&bchar) && (0..=255).contains(&achar) {
                        return Some((adx, ady, bchar as u8, achar as u8));
                    }
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

pub(super) fn seac_components(cs: &[u8]) -> Option<(u8, u8)> {
    seac_operands(cs).map(|(_, _, bchar, achar)| (bchar, achar))
}

pub(crate) fn seac_offsets(cs: &[u8]) -> Option<(f64, f64, u8, u8)> {
    seac_operands(cs).map(|(adx, ady, bchar, achar)| (adx as f64, ady as f64, bchar, achar))
}

pub(super) fn build_format0_sid_to_gid_map(cff: &[u8], charset_off: Option<usize>, n_glyphs: usize) -> Option<BTreeMap<u16, u16>> {
    let off = charset_off?;
    if *cff.get(off)? != 0 { return None; }
    let mut map = BTreeMap::new();
    walk_charset(cff, off, n_glyphs, |gid, sid| {
        map.entry(sid).or_insert(gid);
        CharsetFlow::Continue
    }).ok()?;
    Some(map)
}

pub(crate) fn sid_to_gid(cff: &[u8], charset_off: Option<usize>, n_glyphs: usize, target: u16) -> Option<u16> {
    if target == 0 { return Some(0) }
    let off = match charset_off {
        None => return if (target as usize) < n_glyphs { Some(target) } else { None },
        Some(o) => o,
    };
    let mut hit = None;
    walk_charset(cff, off, n_glyphs, |gid, sid| {
        if sid == target { hit = Some(gid); CharsetFlow::Stop } else { CharsetFlow::Continue }
    }).ok()?;
    hit
}

pub fn seac_component_gids(
    charstrings: &[&[u8]],
    gids: &[u16],
    cff: &[u8],
    charset_off: Option<usize>,
    format0_map: Option<&BTreeMap<u16, u16>>,
) -> Vec<u16> {
    let n_glyphs = charstrings.len();
    let mut found = Vec::new();
    for &gid in gids {
        let cs = match charstrings.get(gid as usize) { Some(c) => *c, None => continue };
        if let Some((bchar, achar)) = seac_components(cs) {
            for code in [bchar, achar] {
                let sid = standard_encoding_sid(code);
                let comp = match format0_map {
                    Some(map) => if sid == 0 { Some(0) } else { map.get(&sid).copied() },
                    None => sid_to_gid(cff, charset_off, n_glyphs, sid),
                };
                if let Some(comp) = comp {
                    found.push(comp);
                }
            }
        }
    }
    found
}
