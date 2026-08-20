use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::super::super::decoder::{read_u16_be, read_u32_be};
use super::super::super::format::aat::Lookup;
use super::super::otl::remap_gid;
use super::lookup::build_aat_lookup;
use super::state::{assemble, state_parts};

const DIRECTION_HEADER: usize = 6;
const ACTION_HEADER: usize = 8;
mod action {
    pub(super) const DECOMPOSITION: u16 = 0;
    pub(super) const ADD_GLYPH: u16 = 1;
    pub(super) const CONDITIONAL_ADD_GLYPH: u16 = 2;
    pub(super) const STRETCH: u16 = 3;
    pub(super) const DUCTILE: u16 = 4;
    pub(super) const REPEATED_ADD_GLYPH: u16 = 5;
}

const NO_GLYPH: u16 = 0xFFFF;

fn subset_actions(data: &[u8], at: usize, active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let count = read_u32_be(data, at)?;
    let mut kept: Vec<Vec<u8>> = Vec::new();

    let mut cursor = at + 4;
    for _ in 0..count {
        let action_type = read_u16_be(data, cursor + 2)?;
        let length = read_u32_be(data, cursor + 4)? as usize;
        if length < ACTION_HEADER { return None; }
        let record = data.get(cursor..cursor.checked_add(length)?)?;
        cursor += length;

        let remap_at = |out: &mut Vec<u8>, offset: usize| -> Option<()> {
            let g = read_u16_be(record, ACTION_HEADER + offset)?;
            let new = remap_gid(active, gid_map, g)?;
            out.get_mut(ACTION_HEADER + offset..ACTION_HEADER + offset + 2)?
                .copy_from_slice(&new.to_be_bytes());
            Some(())
        };

        let mut out = record.to_vec();
        let survived = match action_type {
            action::DECOMPOSITION => {
                let n = read_u16_be(record, ACTION_HEADER + 10)? as usize;
                let glyphs_at = ACTION_HEADER + 12;
                if glyphs_at + n * 2 > record.len() { return None; }
                (0..n).try_for_each(|i| remap_at(&mut out, 12 + i * 2)).is_some()
            }
            action::ADD_GLYPH => remap_at(&mut out, 0).is_some(),
            action::CONDITIONAL_ADD_GLYPH => {
                let add = read_u16_be(record, ACTION_HEADER + 4)?;
                let new_add = if add == NO_GLYPH { NO_GLYPH } else {
                    remap_gid(active, gid_map, add).unwrap_or(NO_GLYPH)
                };
                out.get_mut(ACTION_HEADER + 4..ACTION_HEADER + 6)?.copy_from_slice(&new_add.to_be_bytes());
                remap_at(&mut out, 6).is_some()
            }
            action::REPEATED_ADD_GLYPH => remap_at(&mut out, 2).is_some(),
            action::STRETCH | action::DUCTILE => true,
            _ => false,
        };
        if survived { kept.push(out); }
    }
    if kept.is_empty() { return None; }

    let mut out = (kept.len() as u32).to_be_bytes().to_vec();
    for k in kept { out.extend_from_slice(&k); }
    Some(out)
}

fn remap_keys(data: &[u8], at: usize, active: &GlyphSet, gid_map: &[u16], num_glyphs: u16)
    -> Option<Vec<(u16, u16)>>
{
    let lookup = Lookup::parse(data.get(at..)?, num_glyphs)?;
    let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
        .filter_map(|(g, v)| remap_gid(active, gid_map, g).map(|ng| (ng, v)))
        .collect();
    (!kept.is_empty()).then_some(kept)
}

struct Direction {
    head: Vec<u8>,
    class_table: Option<Vec<u8>>,
    wdc: Vec<u8>,
    pc: Option<Vec<u8>>,
}

fn subset_direction(
    just: &[u8], at: usize, active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Direction> {
    let class_off = read_u16_be(just, at)? as usize;
    let wdc_off = read_u16_be(just, at + 2)? as usize;
    let pc_off = read_u16_be(just, at + 4)? as usize;

    let lookup = build_aat_lookup(&remap_keys(just, at + DIRECTION_HEADER, active, gid_map, num_glyphs)?)?;
    let mut head = alloc::vec![0u8; DIRECTION_HEADER];
    head.extend_from_slice(&lookup);

    let class_table = (class_off != 0).then(|| {
        assemble(&state_parts(just.get(class_off..)?, &[], active, gid_map, num_glyphs)?, 16, &[])
    }).flatten();

    let wdc_end = [class_off, pc_off, just.len()].into_iter()
        .filter(|&o| o > wdc_off).min().unwrap_or(just.len());
    let wdc = just.get(wdc_off..wdc_end)?.to_vec();

    let pc = (pc_off != 0).then(|| -> Option<Vec<u8>> {
            let off = pc_off;
            let entries = remap_keys(just, off, active, gid_map, num_glyphs)?;
            let rebuilt: Vec<(u16, Vec<u8>)> = entries.into_iter()
                .filter_map(|(g, value)| {
                    subset_actions(just, off + value as usize, active, gid_map).map(|a| (g, a))
                })
                .collect();
            if rebuilt.is_empty() { return None; }

            let lookup_len = 12 + rebuilt.len() * 4;
            let mut actions: Vec<u8> = Vec::new();
            let mut placed: Vec<(u16, u16)> = Vec::with_capacity(rebuilt.len());
            for (g, bytes) in rebuilt {
                placed.push((g, (lookup_len + actions.len()) as u16));
                actions.extend_from_slice(&bytes);
            }
            let mut table = build_aat_lookup(&placed)?;
            debug_assert_eq!(table.len(), lookup_len, "the lookup must be the length its offsets assumed");
            table.extend_from_slice(&actions);
            Some(table)
    }).flatten();

    Some(Direction { head, class_table, wdc, pc })
}

pub fn subset_just(
    just: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let version = read_u32_be(just, 0)?;
    let format = read_u16_be(just, 4)?;

    let mut built: [Option<Direction>; 2] = [None, None];
    for (i, slot) in [6usize, 8].into_iter().enumerate() {
        let off = read_u16_be(just, slot)? as usize;
        if off == 0 { continue; }
        built[i] = subset_direction(just, off, active, gid_map, num_glyphs);
    }
    if built.iter().all(Option::is_none) { return None; }

    let mut at = 10usize;
    let mut dir_offsets = [0u16; 2];
    for (i, d) in built.iter().enumerate() {
        let Some(d) = d else { continue };
        dir_offsets[i] = at as u16;
        at += d.head.len();
    }
    let mut table_offsets: [[u16; 3]; 2] = [[0; 3]; 2];
    for (i, d) in built.iter().enumerate() {
        let Some(d) = d else { continue };
        for (k, part) in [d.class_table.as_ref(), Some(&d.wdc), d.pc.as_ref()].into_iter().enumerate() {
            if let Some(p) = part {
                table_offsets[i][k] = at as u16;
                at += p.len();
            }
        }
    }

    let mut out = Vec::with_capacity(at);
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&format.to_be_bytes());
    out.extend_from_slice(&dir_offsets[0].to_be_bytes());
    out.extend_from_slice(&dir_offsets[1].to_be_bytes());
    for (i, d) in built.iter().enumerate() {
        let Some(d) = d else { continue };
        let mut head = d.head.clone();
        for (k, v) in table_offsets[i].iter().enumerate() {
            head.get_mut(k * 2..k * 2 + 2)?.copy_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&head);
    }
    for d in built.iter().flatten() {
        for part in [d.class_table.as_ref(), Some(&d.wdc), d.pc.as_ref()].into_iter().flatten() {
            out.extend_from_slice(part);
        }
    }
    Some(out)
}
