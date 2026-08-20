use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use alloc::collections::{BTreeMap, BTreeSet};
use super::super::super::decoder::{read_u16_be, read_u32_be, write_u32_be};
use super::super::otl::remap_gid;

const HEADER: usize = 8;
const NONE: u32 = 0xFFFF_FFFF;
const GROUP_HAS_FLAGS: u16 = 0x8000;
const GROUP_IS_ARRAY: u16 = 0x4000;
const GROUP_COUNT: u16 = 0x3FFF;

const MAX_GROUP_DEPTH: u8 = 8;

#[allow(clippy::too_many_arguments, reason = "recursive: three of these are accumulators it threads through itself")]
fn place_group(
    zapf: &[u8], extra_info: usize, value: u32, active: &GlyphSet, gid_map: &[u16],
    extra: &mut Vec<u8>, placed: &mut BTreeMap<u32, u32>, open: &mut BTreeSet<u32>, depth: u8,
) -> Option<u32> {
    if let Some(&at) = placed.get(&value) { return Some(at); }
    if depth >= MAX_GROUP_DEPTH { return None; }
    open.insert(value);

    let at = extra_info.checked_add(value as usize)?;
    let num_groups = read_u16_be(zapf, at)?;
    let count = (num_groups & GROUP_COUNT) as usize;

    let bytes = if num_groups & GROUP_IS_ARRAY != 0 {
        let mut children = Vec::with_capacity(count);
        for i in 0..count {
            let child = read_u32_be(zapf, at + 4 + i * 4)?;
            children.push(if child == NONE || open.contains(&child) {
                NONE
            } else {
                place_group(zapf, extra_info, child, active, gid_map, extra, placed, open, depth + 1)?
            });
        }
        let mut out = num_groups.to_be_bytes().to_vec();
        out.extend_from_slice(&read_u16_be(zapf, at + 2)?.to_be_bytes());
        for c in children { out.extend_from_slice(&c.to_be_bytes()); }
        out
    } else {
        let mut out = num_groups.to_be_bytes().to_vec();
        let mut p = at + 2;
        if num_groups & GROUP_HAS_FLAGS != 0 {
            out.extend_from_slice(zapf.get(p..p + 4)?);
            p += 4;
        }
        for _ in 0..count {
            let name_index = read_u16_be(zapf, p)?;
            let n_glyphs = read_u16_be(zapf, p + 2)? as usize;
            let kept: Vec<u16> = (0..n_glyphs)
                .filter_map(|k| read_u16_be(zapf, p + 4 + k * 2))
                .filter_map(|g| remap_gid(active, gid_map, g))
                .collect();
            out.extend_from_slice(&name_index.to_be_bytes());
            out.extend_from_slice(&(kept.len() as u16).to_be_bytes());
            for g in kept { out.extend_from_slice(&g.to_be_bytes()); }
            p += 4 + n_glyphs * 2;
        }
        out
    };

    let mut bytes = bytes;
    while bytes.len() % 4 != 0 { bytes.push(0); }

    let landed = extra.len() as u32;
    extra.extend_from_slice(&bytes);
    placed.insert(value, landed);
    open.remove(&value);
    Some(landed)
}

pub fn subset_zapf(
    zapf: &[u8], num_glyphs: usize, active_sorted: &[u16], active: &GlyphSet, gid_map: &[u16],
) -> Option<Vec<u8>> {
    let extra_info = read_u32_be(zapf, 4)? as usize;
    if !super::super::super::decoder::records_fit(HEADER, num_glyphs, 4, zapf.len()) { return None; }

    let offset_of = |g: u16| read_u32_be(zapf, HEADER + g as usize * 4);
    let mut bounds: Vec<usize> = (0..num_glyphs)
        .filter_map(|g| offset_of(g as u16))
        .map(|o| o as usize)
        .collect();
    bounds.push(extra_info.min(zapf.len()));
    bounds.sort_unstable();
    bounds.dedup();
    let record_end = |from: usize| bounds.iter().copied().find(|&o| o > from).unwrap_or(zapf.len());

    let mut extra: Vec<u8> = Vec::new();
    let mut group_at: BTreeMap<u32, u32> = BTreeMap::new();
    let mut open: BTreeSet<u32> = BTreeSet::new();
    let mut feat_at: BTreeMap<u32, u32> = BTreeMap::new();

    let mut feat_bounds: Vec<usize> = active_sorted.iter()
        .filter_map(|&g| offset_of(g))
        .filter_map(|o| read_u32_be(zapf, o as usize + 4))
        .filter(|&f| f != NONE)
        .map(|f| extra_info + f as usize)
        .collect();
    feat_bounds.push(zapf.len());
    feat_bounds.sort_unstable();
    feat_bounds.dedup();

    let mut records: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
    for &orig in active_sorted {
        let Some(new_gid) = remap_gid(active, gid_map, orig) else { continue };
        let at = offset_of(orig)? as usize;
        let mut record = zapf.get(at..record_end(at))?.to_vec();
        if record.len() < 8 { return None; }

        let group = read_u32_be(&record, 0)?;
        if group != NONE {
            let at = place_group(zapf, extra_info, group, active, gid_map, &mut extra, &mut group_at, &mut open, 0)?;
            write_u32_be(&mut record, 0, at);
        }
        let feature = read_u32_be(&record, 4)?;
        if feature != NONE {
            let at = match feat_at.get(&feature) {
                Some(&p) => p,
                None => {
                    let p = extra.len() as u32;
                    let from = extra_info + feature as usize;
                    let end = feat_bounds.iter().copied().find(|&o| o > from).unwrap_or(zapf.len());
                    extra.extend_from_slice(zapf.get(from..end)?);
                    feat_at.insert(feature, p);
                    p
                }
            };
            write_u32_be(&mut record, 4, at);
        }
        records.insert(new_gid, record);
    }
    if records.is_empty() { return None; }

    let offsets_at = HEADER;
    let new_count = *records.keys().next_back()? as usize + 1;
    let records_at = offsets_at + new_count * 4;
    let mut out = zapf.get(..4)?.to_vec();
    out.resize(records_at, 0);

    for i in 0..new_count { write_u32_be(&mut out, offsets_at + i * 4, NONE); }

    let mut cursor = records_at;
    for (&new_gid, r) in &records {
        write_u32_be(&mut out, offsets_at + new_gid as usize * 4, cursor as u32);
        cursor += r.len();
    }
    write_u32_be(&mut out, 4, cursor as u32);
    for r in records.values() { out.extend_from_slice(r); }
    out.extend_from_slice(&extra);
    Some(out)
}
