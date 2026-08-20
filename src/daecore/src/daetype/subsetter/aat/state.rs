use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::super::super::decoder::{read_u16_be, read_u32_be};
use super::super::super::format::aat::Lookup;
use super::super::otl::remap_gid;
use super::lookup::build_aat_lookup;

const HEADER_LEN: usize = 16;

pub(crate) fn assemble(parts: &StateParts, header_len: usize, trailing: &[&[u8]]) -> Option<Vec<u8>> {
    let class_table = build_aat_lookup(&parts.classes)?;
    let class_off = header_len;
    let state_off = class_off + class_table.len();
    let entry_off = state_off + parts.state.len();

    let mut out = alloc::vec![0u8; header_len];
    for (i, v) in [parts.n_classes, class_off as u32, state_off as u32, entry_off as u32].iter().enumerate() {
        out.get_mut(i * 4..i * 4 + 4)?.copy_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&class_table);
    out.extend_from_slice(parts.state);
    out.extend_from_slice(parts.entry);
    for t in trailing { out.extend_from_slice(t); }
    Some(out)
}

pub(crate) struct StateParts<'a> {
    pub(crate) n_classes: u32,
    pub(crate) classes: Vec<(u16, u16)>,
    pub(crate) state: &'a [u8],
    pub(crate) entry: &'a [u8],
}

pub(crate) fn state_parts<'a>(
    data: &'a [u8], extra_starts: &[usize], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<StateParts<'a>> {
    let n_classes = read_u32_be(data, 0)?;
    let class_off = read_u32_be(data, 4)? as usize;
    let state_off = read_u32_be(data, 8)? as usize;
    let entry_off = read_u32_be(data, 12)? as usize;
    if n_classes == 0 || n_classes > 0xFFFF { return None; }

    let mut starts: Vec<usize> = alloc::vec![class_off, state_off, entry_off];
    starts.extend_from_slice(extra_starts);
    let end_of = |from: usize| starts.iter().copied().chain([data.len()])
        .filter(|&o| o > from).min().unwrap_or(data.len());

    let state = data.get(state_off..end_of(state_off))?;
    let entry = data.get(entry_off..end_of(entry_off))?;
    if state.is_empty() || entry.is_empty() { return None; }

    let lookup = Lookup::parse(data.get(class_off..end_of(class_off))?, num_glyphs)?;
    let classes: Vec<(u16, u16)> = lookup.entries().into_iter()
        .filter(|(g, _)| active.contains(g))
        .filter_map(|(g, class)| remap_gid(active, gid_map, g).map(|ng| (ng, class)))
        .collect();

    Some(StateParts { n_classes, classes, state, entry })
}

pub(crate) fn entry_table<'a>(data: &'a [u8], extra_starts: &[usize]) -> Option<&'a [u8]> {
    let entry_off = read_u32_be(data, 12)? as usize;
    let mut starts: Vec<usize> = alloc::vec![
        read_u32_be(data, 4)? as usize, read_u32_be(data, 8)? as usize, entry_off,
    ];
    starts.extend_from_slice(extra_starts);
    let end = starts.iter().copied().chain([data.len()])
        .filter(|&o| o > entry_off).min().unwrap_or(data.len());
    data.get(entry_off..end)
}

pub(crate) fn entries_of(entry: &[u8], extra_words: usize) -> Vec<(u16, u16, u16)> {
    let stride = 4 + 2 * extra_words;
    (0..entry.len() / stride)
        .filter_map(|i| {
            let at = i * stride;
            Some((
                read_u16_be(entry, at + 2)?,
                if extra_words >= 1 { read_u16_be(entry, at + 4)? } else { 0 },
                if extra_words >= 2 { read_u16_be(entry, at + 6)? } else { 0 },
            ))
        })
        .collect()
}

pub(crate) fn subset_state_table(
    data: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let parts = state_parts(data, &[], active, gid_map, num_glyphs)?;
    assemble(&parts, HEADER_LEN, &[])
}
