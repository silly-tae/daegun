use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::string::String;
use alloc::vec::Vec;
#[allow(unused_imports)]
use crate::daecore::daetype::decoder::{read_u16_be, read_i16_be, read_u32_be, write_u16_be, records_fit};
use super::{parse_coverage, build_coverage, remap_gid};
use super::lookup_list;
use super::context;
use super::generic::{self, schemas};

pub(crate) fn parse_single_subst(buf: &[u8], off: usize) -> Result<Vec<(u16, u16)>, String> {
    let format = read_u16_be(buf, off).ok_or("SingleSubst: truncated")?;
    let cov_off = read_u16_be(buf, off + 2).ok_or("SingleSubst: truncated")? as usize;
    let coverage = parse_coverage(buf, off + cov_off)?;
    match format {
        1 => {
            let delta = read_i16_be(buf, off + 4).ok_or("SingleSubst format 1: truncated")?;
            Ok(coverage.iter().map(|&g| (g, (g as i32 + delta as i32).rem_euclid(65536) as u16)).collect())
        }
        2 => {
            let count = read_u16_be(buf, off + 4).ok_or("SingleSubst format 2: truncated")? as usize;
            let mut pairs = Vec::with_capacity(count.min(coverage.len()));
            for (i, &g) in coverage.iter().enumerate().take(count) {
                let sub = read_u16_be(buf, off + 6 + i * 2).ok_or("SingleSubst format 2: substitute array truncated")?;
                pairs.push((g, sub));
            }
            Ok(pairs)
        }
        _ => Err(format!("SingleSubst: unknown format {}", format)),
    }
}

pub(crate) fn parse_coverage_indexed_glyph_arrays(buf: &[u8], off: usize) -> Result<Vec<(u16, Vec<u16>)>, String> {
    let cov_off = read_u16_be(buf, off + 2).ok_or("coverage-indexed glyph array subtable: truncated")? as usize;
    let coverage = parse_coverage(buf, off + cov_off)?;
    let count = read_u16_be(buf, off + 4).ok_or("coverage-indexed glyph array subtable: truncated")? as usize;
    let mut result = Vec::with_capacity(count.min(coverage.len()));
    for (i, &g) in coverage.iter().enumerate().take(count) {
        let arr_rel = read_u16_be(buf, off + 6 + i * 2).ok_or("coverage-indexed glyph array subtable: array offset truncated")? as usize;
        let arr_off = off + arr_rel;
        let glyph_count = read_u16_be(buf, arr_off).ok_or("coverage-indexed glyph array subtable: array truncated")? as usize;
        let mut glyphs = Vec::with_capacity(glyph_count);
        for j in 0..glyph_count {
            glyphs.push(read_u16_be(buf, arr_off + 2 + j * 2).ok_or("coverage-indexed glyph array subtable: glyph truncated")?);
        }
        result.push((g, glyphs));
    }
    Ok(result)
}

pub(crate) type LigatureSet = Vec<(u16, Vec<u16>)>;

const MAX_LIGATURE_ENTRIES: usize = 1_000_000;

pub(crate) fn parse_ligature_subst(buf: &[u8], off: usize) -> Result<Vec<(u16, LigatureSet)>, String> {
    let cov_off = read_u16_be(buf, off + 2).ok_or("LigatureSubst: truncated")? as usize;
    let coverage = parse_coverage(buf, off + cov_off)?;
    let lig_set_count = read_u16_be(buf, off + 4).ok_or("LigatureSubst: truncated")? as usize;
    if !records_fit(off + 6, lig_set_count, 2, buf.len()) {
        return Err("LigatureSubst: LigatureSet offset array does not fit".into());
    }
    let mut budget = MAX_LIGATURE_ENTRIES;
    let mut result = Vec::with_capacity(lig_set_count.min(coverage.len()).min(256));
    for (i, &first) in coverage.iter().enumerate().take(lig_set_count) {
        let ls_rel = read_u16_be(buf, off + 6 + i * 2).ok_or("LigatureSubst: LigatureSet offset truncated")? as usize;
        let ls_off = off + ls_rel;
        let lig_count = read_u16_be(buf, ls_off).ok_or("LigatureSubst: LigatureSet truncated")? as usize;
        if !records_fit(ls_off + 2, lig_count, 2, buf.len()) {
            return Err("LigatureSubst: Ligature offset array does not fit".into());
        }
        let mut ligs: LigatureSet = Vec::with_capacity(lig_count.min(256));
        for j in 0..lig_count {
            let lig_rel = read_u16_be(buf, ls_off + 2 + j * 2).ok_or("LigatureSubst: Ligature offset truncated")? as usize;
            let lig_off = ls_off + lig_rel;
            let lig_glyph = read_u16_be(buf, lig_off).ok_or("Ligature: truncated")?;
            let comp_count = read_u16_be(buf, lig_off + 2).ok_or("Ligature: truncated")? as usize;
            if comp_count == 0 { return Err("Ligature: CompCount must be at least 1".into()); }
            if !records_fit(lig_off + 4, comp_count - 1, 2, buf.len()) {
                return Err("Ligature: component array does not fit".into());
            }
            budget = budget.checked_sub(comp_count).ok_or("LigatureSubst: entry budget exhausted")?;
            let mut components = Vec::with_capacity((comp_count - 1).min(256));
            for k in 0..comp_count - 1 {
                components.push(read_u16_be(buf, lig_off + 4 + k * 2).ok_or("Ligature: component truncated")?);
            }
            ligs.push((lig_glyph, components));
        }
        result.push((first, ligs));
    }
    Ok(result)
}

struct ReverseChainSingleSubst {
    backtrack: Vec<Vec<u16>>,
    lookahead: Vec<Vec<u16>>,
    pairs: Vec<(u16, u16)>,
}

fn parse_reverse_chain_single_subst(buf: &[u8], off: usize) -> Result<ReverseChainSingleSubst, String> {
    let cov_off = read_u16_be(buf, off + 2).ok_or("ReverseChainSingleSubst: truncated")? as usize;
    let bt_count = read_u16_be(buf, off + 4).ok_or("ReverseChainSingleSubst: truncated")? as usize;
    let backtrack = context::parse_coverage_array(buf, off, off + 6, bt_count)?;
    let la_off = off + 6 + bt_count * 2;
    let la_count = read_u16_be(buf, la_off).ok_or("ReverseChainSingleSubst: truncated")? as usize;
    let lookahead = context::parse_coverage_array(buf, off, la_off + 2, la_count)?;
    let gc_off = la_off + 2 + la_count * 2;
    let glyph_count = read_u16_be(buf, gc_off).ok_or("ReverseChainSingleSubst: truncated")? as usize;
    let coverage = parse_coverage(buf, off + cov_off)?;
    let mut pairs = Vec::with_capacity(glyph_count.min(coverage.len()));
    for (i, &g) in coverage.iter().enumerate().take(glyph_count) {
        let sub = read_u16_be(buf, gc_off + 2 + i * 2).ok_or("ReverseChainSingleSubst: substitute array truncated")?;
        pairs.push((g, sub));
    }
    Ok(ReverseChainSingleSubst { backtrack, lookahead, pairs })
}

fn subset_reverse_chain_single_subst(buf: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let parsed = parse_reverse_chain_single_subst(buf, off).ok()?;
    let new_backtrack = context::filter_coverage_group(&parsed.backtrack, active, gid_map)?;
    let new_lookahead = context::filter_coverage_group(&parsed.lookahead, active, gid_map)?;

    let mut remapped: Vec<(u16, u16)> = parsed.pairs.into_iter()
        .filter_map(|(g, s)| match (remap_gid(active, gid_map, g), remap_gid(active, gid_map, s)) {
            (Some(ng), Some(ns)) => Some((ng, ns)),
            _ => None,
        })
        .collect();
    if remapped.is_empty() { return None; }
    remapped.sort_unstable_by_key(|&(g, _)| g);

    let bt_blobs = context::build_coverage_array_blobs(&new_backtrack);
    let la_blobs = context::build_coverage_array_blobs(&new_lookahead);
    let cov_gids: Vec<u16> = remapped.iter().map(|&(g, _)| g).collect();
    let cov_bytes = build_coverage(&cov_gids);

    let header_len = 6 + bt_blobs.len() * 2 + 2 + la_blobs.len() * 2 + 2 + remapped.len() * 2;
    let mut out = vec![0u8; header_len];
    write_u16_be(&mut out, 0, 1);
    write_u16_be(&mut out, 4, u16::try_from(bt_blobs.len()).ok()?);
    let mut pos = header_len;
    for (i, blob) in bt_blobs.iter().enumerate() {
        write_u16_be(&mut out, 6 + i * 2, u16::try_from(pos).ok()?);
        pos = pos.checked_add(blob.len())?;
    }
    let la_count_off = 6 + bt_blobs.len() * 2;
    write_u16_be(&mut out, la_count_off, u16::try_from(la_blobs.len()).ok()?);
    for (i, blob) in la_blobs.iter().enumerate() {
        write_u16_be(&mut out, la_count_off + 2 + i * 2, u16::try_from(pos).ok()?);
        pos = pos.checked_add(blob.len())?;
    }
    let gc_off = la_count_off + 2 + la_blobs.len() * 2;
    write_u16_be(&mut out, gc_off, u16::try_from(remapped.len()).ok()?);
    for (i, &(_, sub)) in remapped.iter().enumerate() { write_u16_be(&mut out, gc_off + 2 + i * 2, sub); }
    write_u16_be(&mut out, 2, u16::try_from(pos).ok()?);
    for blob in &bt_blobs { out.extend_from_slice(blob); }
    for blob in &la_blobs { out.extend_from_slice(blob); }
    out.extend(cov_bytes);
    Some(out)
}

fn reverse_chain_single_subst_outputs(buf: &[u8], off: usize, active: &GlyphSet, out: &mut Vec<u16>) {
    if let Ok(parsed) = parse_reverse_chain_single_subst(buf, off) {
        for (orig, sub) in parsed.pairs { if active.contains(&orig) { out.push(sub); } }
    }
}

pub(crate) fn resolve_effective_type(gsub: &[u8], lookup_type: u16, sub_off: usize) -> Option<(u16, usize)> {
    lookup_list::resolve_effective_type(gsub, 7, lookup_type, sub_off)
}

pub(crate) fn each_lookup_subtable(gsub: &[u8], mut f: impl FnMut(u16, &[u8], usize)) -> Option<()> {
    let lookup_off = read_u16_be(gsub, 8)? as usize;
    let count = read_u16_be(gsub, lookup_off)?;
    for i in 0..count as usize {
        let rel = read_u16_be(gsub, lookup_off + 2 + i * 2)?;
        let lookup_start = lookup_off + rel as usize;
        let Some(lookup_type) = read_u16_be(gsub, lookup_start) else { continue };
        let Some(sub_count) = read_u16_be(gsub, lookup_start + 4) else { continue };
        for j in 0..sub_count as usize {
            let Some(srel) = read_u16_be(gsub, lookup_start + 6 + j * 2) else { break };
            if let Some((real_type, real_off)) = resolve_effective_type(gsub, lookup_type, lookup_start + srel as usize) {
                f(real_type, gsub, real_off);
            }
        }
    }
    Some(())
}

fn multiple_subst_outputs(buf: &[u8], off: usize, active: &GlyphSet, out: &mut Vec<u16>) {
    if let Ok(entries) = parse_coverage_indexed_glyph_arrays(buf, off) {
        for (orig, subs) in entries {
            if active.contains(&orig) { out.extend(subs); }
        }
    }
}

fn ligature_outputs(buf: &[u8], off: usize, active: &GlyphSet, out: &mut Vec<u16>) {
    if let Ok(entries) = parse_ligature_subst(buf, off) {
        for (first, ligs) in entries {
            if !active.contains(&first) { continue; }
            for (lig_glyph, components) in ligs {
                if components.iter().all(|c| active.contains(c)) {
                    out.push(lig_glyph);
                }
            }
        }
    }
}

pub fn gsub_closure(gsub: &[u8], active: &GlyphSet) -> Vec<u16> {
    let mut found = Vec::new();
    each_lookup_subtable(gsub, |lookup_type, buf, off| {
        match lookup_type {
            1 => {
                if let Ok(pairs) = parse_single_subst(buf, off) {
                    for (orig, sub) in pairs { if active.contains(&orig) { found.push(sub); } }
                }
            }
            2 => multiple_subst_outputs(buf, off, active, &mut found),
            3 => {
                if let Ok(entries) = parse_coverage_indexed_glyph_arrays(buf, off) {
                    for (orig, alts) in entries { if active.contains(&orig) { found.extend(alts); } }
                }
            }
            4 => ligature_outputs(buf, off, active, &mut found),
            8 => reverse_chain_single_subst_outputs(buf, off, active, &mut found),
            _ => {}
        }
    });
    found
}

pub(crate) fn subset_gsub_subtable(effective_type: u16, schema: Option<&generic::schema::Schema>, buf: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    if effective_type == 8 {
        return subset_reverse_chain_single_subst(buf, off, active, gid_map);
    }
    generic::subset_subtable(buf, off, schema?, active, gid_map)
}

pub fn subset_gsub(gsub: &[u8], active: &GlyphSet, gid_map: &[u16], mark_filter_sets_survive: bool) -> Option<Vec<u8>> {
    lookup_list::subset_lookup_table(gsub, 7, active, gid_map, mark_filter_sets_survive, &subset_gsub_subtable, &schemas::gsub_schema_for_type)
}
