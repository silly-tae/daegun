use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::super::super::decoder::{read_u16_be, read_u32_be, read_i16_be};
use super::super::super::format::aat::Lookup;
use super::super::otl::remap_gid;
use super::lookup::build_aat_lookup;
use super::state::{assemble, state_parts};

const FORMAT0_HEADER: usize = 16;
const STATE_PLUS_ONE_OFFSET: usize = 20;
const FORMAT0_PAIR: usize = 6;

fn subset_format0(body: &[u8], active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let n_pairs = read_u32_be(body, 0)? as usize;
    let mut kept: Vec<(u16, u16, i16)> = Vec::new();
    for i in 0..n_pairs {
        let at = FORMAT0_HEADER + i * FORMAT0_PAIR;
        let (Some(left), Some(right), Some(value)) =
            (read_u16_be(body, at), read_u16_be(body, at + 2), read_i16_be(body, at + 4)) else { break };
        let (Some(l), Some(r)) = (remap_gid(active, gid_map, left), remap_gid(active, gid_map, right))
        else { continue };
        kept.push((l, r, value));
    }
    if kept.is_empty() { return None; }
    kept.sort_unstable_by_key(|&(l, r, _)| (l, r));

    let n = kept.len();
    let selector = usize::BITS - 1 - n.leading_zeros();
    let search_range = FORMAT0_PAIR * (1usize << selector);
    let mut out = Vec::with_capacity(FORMAT0_HEADER + n * FORMAT0_PAIR);
    for v in [n, search_range, selector as usize, FORMAT0_PAIR * n - search_range] {
        out.extend_from_slice(&(v as u32).to_be_bytes());
    }
    for (l, r, value) in kept {
        out.extend_from_slice(&l.to_be_bytes());
        out.extend_from_slice(&r.to_be_bytes());
        out.extend_from_slice(&value.to_be_bytes());
    }
    Some(out)
}

fn subset_format2(
    body: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let row_width = read_u32_be(body, 0)?;
    let left_off = read_u32_be(body, 4)? as usize;
    let right_off = read_u32_be(body, 8)? as usize;
    let array_off = read_u32_be(body, 12)? as usize;

    let rebuild = |at: usize| -> Option<Vec<u8>> {
        let lookup = Lookup::parse(body.get(at..)?, num_glyphs)?;
        let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
            .filter_map(|(g, v)| remap_gid(active, gid_map, g).map(|ng| (ng, v)))
            .collect();
        build_aat_lookup(&kept)
    };
    let left = rebuild(left_off)?;
    let right = rebuild(right_off)?;
    let array = body.get(array_off..)?;

    let new_left = 16usize;
    let new_right = new_left + left.len();
    let new_array = new_right + right.len();
    let mut out = Vec::with_capacity(new_array + array.len());
    for v in [row_width, new_left as u32, new_right as u32, new_array as u32] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&left);
    out.extend_from_slice(&right);
    out.extend_from_slice(array);
    Some(out)
}

fn subset_format1(
    body: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let value_off = read_u32_be(body, 16)? as usize;
    let parts = state_parts(body, &[value_off], active, gid_map, num_glyphs)?;
    let values = body.get(value_off..)?;

    let mut out = assemble(&parts, STATE_PLUS_ONE_OFFSET, &[values])?;
    let at = (out.len() - values.len()) as u32;
    out.get_mut(16..20)?.copy_from_slice(&at.to_be_bytes());
    Some(out)
}

fn subset_format4(
    body: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    const ACTION_OFFSET: u32 = 0x00FF_FFFF;
    let flags = read_u32_be(body, 16)?;
    let table_off = (flags & ACTION_OFFSET) as usize;
    let parts = state_parts(body, &[table_off], active, gid_map, num_glyphs)?;
    let table = body.get(table_off..)?;

    let mut out = assemble(&parts, STATE_PLUS_ONE_OFFSET, &[table])?;
    let at = (out.len() - table.len()) as u32;
    if at & !ACTION_OFFSET != 0 { return None; }
    out.get_mut(16..20)?.copy_from_slice(&((flags & !ACTION_OFFSET) | at).to_be_bytes());
    Some(out)
}

fn subset_format6(
    body: &[u8], tuple_count: u32, active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    const VALUES_ARE_LONG: u32 = 1;
    let flags = read_u32_be(body, 0)?;
    if flags & VALUES_ARE_LONG != 0 { return None; }

    let row_off = read_u32_be(body, 8)? as usize;
    let col_off = read_u32_be(body, 12)? as usize;
    let array_off = read_u32_be(body, 16)? as usize;
    let vector_off = if tuple_count >= 1 { Some(read_u32_be(body, 20)? as usize) } else { None };

    let rebuild = |at: usize| -> Option<Vec<u8>> {
        let lookup = Lookup::parse(body.get(at..)?, num_glyphs)?;
        let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
            .filter_map(|(g, v)| remap_gid(active, gid_map, g).map(|ng| (ng, v)))
            .collect();
        build_aat_lookup(&kept)
    };
    let rows = rebuild(row_off)?;
    let cols = rebuild(col_off)?;

    let array_end = vector_off.filter(|&v| v > array_off).unwrap_or(body.len());
    let array = body.get(array_off..array_end)?;
    let vectors = match vector_off {
        Some(v) => Some(body.get(v..)?),
        None => None,
    };

    let header = if vector_off.is_some() { 24usize } else { 20 };
    let new_rows = header;
    let new_cols = new_rows + rows.len();
    let new_array = new_cols + cols.len();
    let new_vectors = new_array + array.len();

    let mut out = alloc::vec![0u8; header];
    out.get_mut(..8)?.copy_from_slice(body.get(..8)?);
    for (i, v) in [new_rows as u32, new_cols as u32, new_array as u32].iter().enumerate() {
        out.get_mut(8 + i * 4..12 + i * 4)?.copy_from_slice(&v.to_be_bytes());
    }
    if vectors.is_some() {
        out.get_mut(20..24)?.copy_from_slice(&(new_vectors as u32).to_be_bytes());
    }
    out.extend_from_slice(&rows);
    out.extend_from_slice(&cols);
    out.extend_from_slice(array);
    if let Some(v) = vectors { out.extend_from_slice(v); }
    Some(out)
}

pub fn subset_kerx(
    kerx: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let n_tables = read_u32_be(kerx, 4)?;
    let mut kept: Vec<Vec<u8>> = Vec::new();

    let mut at = 8usize;
    for _ in 0..n_tables {
        let length = read_u32_be(kerx, at)? as usize;
        let coverage = read_u32_be(kerx, at + 4)?;
        let tuple_count = read_u32_be(kerx, at + 8)?;
        let body = kerx.get(at + 12..at.checked_add(length)?)?;

        let rebuilt = match coverage & 0xFF {
            0 if tuple_count == 0 => subset_format0(body, active, gid_map),
            1 => subset_format1(body, active, gid_map, num_glyphs),
            2 if tuple_count == 0 => subset_format2(body, active, gid_map, num_glyphs),
            4 => subset_format4(body, active, gid_map, num_glyphs),
            6 => subset_format6(body, tuple_count, active, gid_map, num_glyphs),
            _ => None,
        };
        if let Some(new_body) = rebuilt {
            let mut sub = Vec::with_capacity(12 + new_body.len());
            sub.extend_from_slice(&((12 + new_body.len()) as u32).to_be_bytes());
            sub.extend_from_slice(&coverage.to_be_bytes());
            sub.extend_from_slice(&tuple_count.to_be_bytes());
            sub.extend_from_slice(&new_body);
            kept.push(sub);
        }

        let next = at.checked_add(length)?;
        if length == 0 || next > kerx.len() { break; }
        at = next;
    }
    if kept.is_empty() { return None; }

    let mut out = kerx.get(..4)?.to_vec();
    out.extend_from_slice(&(kept.len() as u32).to_be_bytes());
    for s in &kept { out.extend_from_slice(s); }
    Some(out)
}
