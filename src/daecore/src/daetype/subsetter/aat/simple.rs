use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::super::super::decoder::{read_u16_be, read_u32_be, write_u32_be};
use super::super::super::format::aat::Lookup;
use super::super::otl::remap_gid;
use super::lookup::build_aat_lookup;

const CARET_HEADER: usize = 6;

#[allow(clippy::too_many_arguments, reason = "two call sites, and `blob_len` is the only difference between them")]
fn subset_offset_lookup(
    table: &[u8], header: &[u8], lookup_at: usize, base: usize,
    active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
    blob_len: impl Fn(&[u8], usize) -> Option<usize>,
) -> Option<Vec<u8>> {
    let lookup = Lookup::parse(table.get(lookup_at..)?, num_glyphs)?;
    let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
        .filter_map(|(g, v)| remap_gid(active, gid_map, g).map(|ng| (ng, v)))
        .collect();
    if kept.is_empty() { return None; }

    let measured = build_aat_lookup(&kept)?.len();
    let blobs_at = header.len() + measured;

    let mut blobs: Vec<u8> = Vec::new();
    let mut placed: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut entries: Vec<(u16, u16)> = Vec::with_capacity(kept.len());
    for (g, value) in kept {
        let from = base.checked_add(value as usize)?;
        let len = blob_len(table, from)?;
        let bytes = table.get(from..from.checked_add(len)?)?.to_vec();
        let at = match placed.iter().find(|(b, _)| *b == bytes) {
            Some((_, at)) => *at,
            None => {
                let at = blobs_at + blobs.len();
                blobs.extend_from_slice(&bytes);
                placed.push((bytes, at));
                at
            }
        };
        entries.push((g, u16::try_from(at).ok()?));
    }

    let lookup_bytes = build_aat_lookup(&entries)?;
    if lookup_bytes.len() != measured { return None; }

    let mut out = header.to_vec();
    out.extend_from_slice(&lookup_bytes);
    out.extend_from_slice(&blobs);
    Some(out)
}

pub fn subset_lcar(
    lcar: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let header = lcar.get(..CARET_HEADER)?;
    subset_offset_lookup(lcar, header, CARET_HEADER, 0, active, gid_map, num_glyphs, |d, at| {
        Some(2 + read_u16_be(d, at)? as usize * 2)
    })
}

pub fn subset_opbd(
    opbd: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let header = opbd.get(..CARET_HEADER)?;
    subset_offset_lookup(opbd, header, CARET_HEADER, 0, active, gid_map, num_glyphs, |_, _| Some(8))
}

pub fn subset_ankr(
    ankr: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let lookup_off = read_u32_be(ankr, 4)? as usize;
    let data_off = read_u32_be(ankr, 8)? as usize;

    let header_len = 12usize;
    let lookup = Lookup::parse(ankr.get(lookup_off..)?, num_glyphs)?;
    let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
        .filter_map(|(g, v)| remap_gid(active, gid_map, g).map(|ng| (ng, v)))
        .collect();
    if kept.is_empty() { return None; }

    let mut data: Vec<u8> = Vec::new();
    let mut placed: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut entries: Vec<(u16, u16)> = Vec::with_capacity(kept.len());
    for (g, value) in kept {
        let from = data_off.checked_add(value as usize)?;
        let len = 4 + read_u32_be(ankr, from)? as usize * 4;
        let bytes = ankr.get(from..from.checked_add(len)?)?.to_vec();
        let at = match placed.iter().find(|(b, _)| *b == bytes) {
            Some((_, at)) => *at,
            None => {
                let at = data.len();
                data.extend_from_slice(&bytes);
                placed.push((bytes, at));
                at
            }
        };
        entries.push((g, u16::try_from(at).ok()?));
    }

    let lookup_bytes = build_aat_lookup(&entries)?;
    let mut out = ankr.get(..4)?.to_vec();
    out.resize(header_len, 0);
    write_u32_be(&mut out, 4, header_len as u32);
    write_u32_be(&mut out, 8, (header_len + lookup_bytes.len()) as u32);
    out.extend_from_slice(&lookup_bytes);
    out.extend_from_slice(&data);
    Some(out)
}

pub fn subset_bsln(
    bsln: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let format = read_u16_be(bsln, 4)?;
    let (fixed_len, std_glyph_at) = match format {
        0 | 1 => (6 + 2 + 32 * 2, None),
        2 | 3 => (6 + 2 + 2 + 32 * 2, Some(8usize)),
        _ => return None,
    };

    let mut out = bsln.get(..fixed_len)?.to_vec();
    if let Some(at) = std_glyph_at {
        let g = read_u16_be(bsln, at)?;
        let new = remap_gid(active, gid_map, g)?;
        out.get_mut(at..at + 2)?.copy_from_slice(&new.to_be_bytes());
    }
    if format == 0 || format == 2 { return Some(out); }

    let lookup = Lookup::parse(bsln.get(fixed_len..)?, num_glyphs)?;
    let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
        .filter_map(|(g, v)| remap_gid(active, gid_map, g).map(|ng| (ng, v)))
        .collect();
    if kept.is_empty() {
        out.get_mut(4..6)?.copy_from_slice(&(format - 1).to_be_bytes());
        return Some(out);
    }
    out.extend_from_slice(&build_aat_lookup(&kept)?);
    Some(out)
}

pub fn subset_fmtx(fmtx: &[u8], active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let glyph = read_u32_be(fmtx, 4)?;
    let new = remap_gid(active, gid_map, u16::try_from(glyph).ok()?)?;
    let mut out = fmtx.to_vec();
    write_u32_be(&mut out, 4, new as u32);
    Some(out)
}
