use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::super::decoder::{read_u16_be, read_i16_be, write_u16_be, records_fit};
use super::otl::{parse_coverage, build_coverage, copy_device_table, remap_gid};

struct SubTable {
    fixed: Vec<u8>,
    pending: Vec<(usize, Vec<u8>)>,
}

impl SubTable {
    fn new() -> Self { SubTable { fixed: Vec::new(), pending: Vec::new() } }
    fn u16(&mut self, v: u16) { self.fixed.extend_from_slice(&v.to_be_bytes()); }
    fn i16(&mut self, v: i16) { self.fixed.extend_from_slice(&v.to_be_bytes()); }
    fn slot(&mut self) -> usize { let at = self.fixed.len(); self.u16(0); at }

    fn value(&mut self, math: &[u8], src: usize, src_parent: usize) {
        self.i16(read_i16_be(math, src).unwrap_or(0));
        let slot = self.slot();
        if let Some(rel) = read_u16_be(math, src + 2).filter(|&r| r != 0)
            && let Some(dev) = copy_device_table(math, src_parent + rel as usize) {
                self.pending.push((slot, dev));
            }
    }

    fn place(&mut self, slot: usize, bytes: &[u8]) {
        let at = self.fixed.len();
        if let Ok(v) = u16::try_from(at) { write_u16_be(&mut self.fixed, slot, v); }
        self.fixed.extend_from_slice(bytes);
    }

    fn finish(mut self) -> Vec<u8> {
        let pending = core::mem::take(&mut self.pending);
        let mut placed: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
        for (slot, dev) in pending {
            let at = match placed.get(&dev) {
                Some(&at) => at,
                None => {
                    let at = self.fixed.len();
                    self.fixed.extend_from_slice(&dev);
                    placed.insert(dev, at);
                    at
                }
            };
            if let Ok(v) = u16::try_from(at) { write_u16_be(&mut self.fixed, slot, v); }
        }
        self.fixed
    }
}

fn sections(math: &[u8]) -> Option<(usize, usize, usize)> {
    Some((
        read_u16_be(math, 4)? as usize,
        read_u16_be(math, 6)? as usize,
        read_u16_be(math, 8)? as usize,
    ))
}

const MATH_CONSTANTS_LEN: usize = 8 + 51 * 4 + 2;

fn variants_direction(math: &[u8], variants: usize, vertical: bool) -> Option<(usize, usize, usize)> {
    let cov_rel = read_u16_be(math, variants + if vertical { 2 } else { 4 })? as usize;
    let count = read_u16_be(math, variants + if vertical { 6 } else { 8 })? as usize;
    let vert_count = read_u16_be(math, variants + 6)? as usize;
    let array = variants + 10 + if vertical { 0 } else { vert_count * 2 };
    Some((variants + cov_rel, count, array))
}

pub fn math_closure(math: &[u8], active: &GlyphSet) -> Vec<u16> {
    let mut found = Vec::new();
    let Some((_, _, variants)) = sections(math) else { return found };
    if variants == 0 { return found; }

    for vertical in [true, false] {
        let Some((cov_off, count, array)) = variants_direction(math, variants, vertical) else { continue };
        let Ok(covered) = parse_coverage(math, cov_off) else { continue };
        if !records_fit(array, count, 2, math.len()) { continue; }

        for (i, &gid) in covered.iter().enumerate() {
            if i >= count || !active.contains(&gid) { continue; }
            let Some(rel) = read_u16_be(math, array + i * 2) else { continue };
            let construction = variants + rel as usize;
            collect_construction_glyphs(math, construction, &mut found);
        }
    }
    found
}

fn collect_construction_glyphs(math: &[u8], construction: usize, out: &mut Vec<u16>) {
    if let Some(count) = read_u16_be(math, construction + 2)
        && records_fit(construction + 4, count as usize, 4, math.len()) {
            for i in 0..count as usize {
                if let Some(g) = read_u16_be(math, construction + 4 + i * 4) { out.push(g); }
            }
        }
    let Some(rel) = read_u16_be(math, construction).filter(|&r| r != 0) else { return };
    let assembly = construction + rel as usize;
    let Some(parts) = read_u16_be(math, assembly + 4) else { return };
    if !records_fit(assembly + 6, parts as usize, 10, math.len()) { return }
    for i in 0..parts as usize {
        if let Some(g) = read_u16_be(math, assembly + 6 + i * 10) { out.push(g); }
    }
}

fn subset_parallel_values(
    math: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16],
) -> Option<Vec<u8>> {
    let cov_off = off + read_u16_be(math, off)? as usize;
    let count = read_u16_be(math, off + 2)? as usize;
    let covered = parse_coverage(math, cov_off).ok()?;
    if !records_fit(off + 4, count, 4, math.len()) { return None; }

    let kept: Vec<(u16, usize)> = covered.iter().enumerate()
        .filter(|&(i, g)| i < count && active.contains(g))
        .filter_map(|(i, g)| remap_gid(active, gid_map, *g).map(|ng| (ng, i)))
        .collect();
    if kept.is_empty() { return None; }

    let mut sub = SubTable::new();
    let cov_slot = sub.slot();
    sub.u16(kept.len() as u16);
    for &(_, i) in &kept { sub.value(math, off + 4 + i * 4, off); }
    let mut out = SubTable { fixed: sub.finish(), pending: Vec::new() };
    let gids: Vec<u16> = kept.iter().map(|&(g, _)| g).collect();
    out.place(cov_slot, &build_coverage(&gids));
    Some(out.fixed)
}

fn subset_math_kern(math: &[u8], off: usize) -> Option<Vec<u8>> {
    let heights = read_u16_be(math, off)? as usize;
    let records = heights.checked_mul(2)?.checked_add(1)?;
    if !records_fit(off + 2, records, 4, math.len()) { return None; }
    let mut sub = SubTable::new();
    sub.u16(heights as u16);
    for i in 0..records { sub.value(math, off + 2 + i * 4, off); }
    Some(sub.finish())
}

fn subset_kern_info(
    math: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16],
) -> Option<Vec<u8>> {
    let cov_off = off + read_u16_be(math, off)? as usize;
    let count = read_u16_be(math, off + 2)? as usize;
    let covered = parse_coverage(math, cov_off).ok()?;
    if !records_fit(off + 4, count, 8, math.len()) { return None; }

    let kept: Vec<(u16, usize)> = covered.iter().enumerate()
        .filter(|&(i, g)| i < count && active.contains(g))
        .filter_map(|(i, g)| remap_gid(active, gid_map, *g).map(|ng| (ng, i)))
        .collect();
    if kept.is_empty() { return None; }

    let mut sub = SubTable::new();
    let cov_slot = sub.slot();
    sub.u16(kept.len() as u16);
    let mut corner_slots: Vec<(usize, Option<usize>)> = Vec::new();
    for &(_, i) in &kept {
        for c in 0..4 {
            let src = read_u16_be(math, off + 4 + i * 8 + c * 2).filter(|&r| r != 0);
            corner_slots.push((sub.slot(), src.map(|r| off + r as usize)));
        }
    }
    let mut out = SubTable { fixed: sub.finish(), pending: Vec::new() };
    for (slot, src) in corner_slots {
        let Some(src) = src else { continue };
        if let Some(kern) = subset_math_kern(math, src) { out.place(slot, &kern); }
    }
    out.place(cov_slot, &build_coverage(&kept.iter().map(|&(g, _)| g).collect::<Vec<_>>()));
    Some(out.fixed)
}

fn subset_construction(
    math: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16],
) -> Option<Vec<u8>> {
    let variant_count = read_u16_be(math, off + 2)? as usize;
    if !records_fit(off + 4, variant_count, 4, math.len()) { return None; }

    let mut variants: Vec<(u16, u16)> = Vec::new();
    for i in 0..variant_count {
        let rec = off + 4 + i * 4;
        let Some(g) = read_u16_be(math, rec) else { continue };
        let Some(ng) = remap_gid(active, gid_map, g) else { continue };
        variants.push((ng, read_u16_be(math, rec + 2).unwrap_or(0)));
    }

    let assembly = read_u16_be(math, off).filter(|&r| r != 0).and_then(|rel| {
        let at = off + rel as usize;
        let parts = read_u16_be(math, at + 4)? as usize;
        if !records_fit(at + 6, parts, 10, math.len()) { return None; }
        let mut mapped: Vec<(u16, [u16; 4])> = Vec::with_capacity(parts);
        for i in 0..parts {
            let rec = at + 6 + i * 10;
            let ng = remap_gid(active, gid_map, read_u16_be(math, rec)?)?;
            let mut rest = [0u16; 4];
            for (k, slot) in rest.iter_mut().enumerate() { *slot = read_u16_be(math, rec + 2 + k * 2)?; }
            mapped.push((ng, rest));
        }
        let mut sub = SubTable::new();
        sub.value(math, at, at);
        sub.u16(mapped.len() as u16);
        for (g, rest) in &mapped {
            sub.u16(*g);
            for v in rest { sub.u16(*v); }
        }
        Some(sub.finish())
    });

    if variants.is_empty() && assembly.is_none() { return None; }

    let mut sub = SubTable::new();
    let assembly_slot = sub.slot();
    sub.u16(variants.len() as u16);
    for (g, adv) in &variants { sub.u16(*g); sub.u16(*adv); }
    let mut out = SubTable { fixed: sub.finish(), pending: Vec::new() };
    if let Some(a) = assembly { out.place(assembly_slot, &a); }
    Some(out.fixed)
}

fn subset_variants(
    math: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16],
) -> Option<Vec<u8>> {
    let mut built: [Vec<(u16, Vec<u8>)>; 2] = [Vec::new(), Vec::new()];
    for (slot, vertical) in [(0usize, true), (1, false)] {
        let Some((cov_off, count, array)) = variants_direction(math, off, vertical) else { continue };
        let Ok(covered) = parse_coverage(math, cov_off) else { continue };
        if !records_fit(array, count, 2, math.len()) { continue; }
        for (i, &gid) in covered.iter().enumerate() {
            if i >= count || !active.contains(&gid) { continue; }
            let (Some(ng), Some(rel)) = (remap_gid(active, gid_map, gid), read_u16_be(math, array + i * 2))
            else { continue };
            if let Some(c) = subset_construction(math, off + rel as usize, active, gid_map) {
                built[slot].push((ng, c));
            }
        }
    }
    if built[0].is_empty() && built[1].is_empty() { return None; }

    let mut sub = SubTable::new();
    sub.u16(read_u16_be(math, off).unwrap_or(0));
    let vert_cov = sub.slot();
    let horiz_cov = sub.slot();
    sub.u16(built[0].len() as u16);
    sub.u16(built[1].len() as u16);
    let mut construction_slots: Vec<usize> = Vec::new();
    for dir in &built {
        for _ in dir { construction_slots.push(sub.slot()); }
    }

    let mut out = SubTable { fixed: sub.finish(), pending: Vec::new() };
    let mut slots = construction_slots.into_iter();
    for dir in &built {
        for (_, bytes) in dir {
            if let Some(slot) = slots.next() { out.place(slot, bytes); }
        }
    }
    for (slot, dir) in [(vert_cov, &built[0]), (horiz_cov, &built[1])] {
        if dir.is_empty() { continue; }
        out.place(slot, &build_coverage(&dir.iter().map(|(g, _)| *g).collect::<Vec<_>>()));
    }
    Some(out.fixed)
}

fn subset_glyph_info(
    math: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16],
) -> Option<Vec<u8>> {
    let italics = read_u16_be(math, off).filter(|&r| r != 0)
        .and_then(|r| subset_parallel_values(math, off + r as usize, active, gid_map));
    let top_accent = read_u16_be(math, off + 2).filter(|&r| r != 0)
        .and_then(|r| subset_parallel_values(math, off + r as usize, active, gid_map));
    let extended = read_u16_be(math, off + 4).filter(|&r| r != 0).and_then(|r| {
        let covered = parse_coverage(math, off + r as usize).ok()?;
        let kept: Vec<u16> = covered.iter().filter_map(|&g| remap_gid(active, gid_map, g)).collect();
        (!kept.is_empty()).then(|| build_coverage(&kept))
    });
    let kern = read_u16_be(math, off + 6).filter(|&r| r != 0)
        .and_then(|r| subset_kern_info(math, off + r as usize, active, gid_map));

    if italics.is_none() && top_accent.is_none() && extended.is_none() && kern.is_none() { return None; }

    let mut sub = SubTable::new();
    let slots: Vec<usize> = (0..4).map(|_| sub.slot()).collect();
    let mut out = SubTable { fixed: sub.finish(), pending: Vec::new() };
    for (slot, bytes) in slots.iter().zip([&italics, &top_accent, &extended, &kern]) {
        if let Some(b) = bytes { out.place(*slot, b); }
    }
    Some(out.fixed)
}

pub fn subset_math(math: &[u8], active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let (constants, glyph_info, variants) = sections(math)?;

    let new_glyph_info = (glyph_info != 0)
        .then(|| subset_glyph_info(math, glyph_info, active, gid_map)).flatten();
    let new_variants = (variants != 0)
        .then(|| subset_variants(math, variants, active, gid_map)).flatten();
    if new_glyph_info.is_none() && new_variants.is_none() { return None; }

    let new_constants = (constants != 0).then(|| {
        let mut sub = SubTable::new();
        for k in 0..2 { sub.i16(read_i16_be(math, constants + k * 2).unwrap_or(0)); }
        for k in 0..2 { sub.u16(read_u16_be(math, constants + 4 + k * 2).unwrap_or(0)); }
        for k in 0..51 { sub.value(math, constants + 8 + k * 4, constants); }
        sub.u16(read_u16_be(math, constants + MATH_CONSTANTS_LEN - 2).unwrap_or(0));
        sub.finish()
    });

    let mut sub = SubTable::new();
    sub.u16(1);
    sub.u16(0);
    let slots: Vec<usize> = (0..3).map(|_| sub.slot()).collect();
    let mut out = SubTable { fixed: sub.finish(), pending: Vec::new() };
    for (slot, bytes) in slots.iter().zip([&new_constants, &new_glyph_info, &new_variants]) {
        if let Some(b) = bytes { out.place(*slot, b); }
    }
    Some(out.fixed)
}
