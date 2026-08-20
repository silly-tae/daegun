use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::super::super::decoder::{read_u16_be, read_u32_be};

const NO_INDEX: u16 = 0xFFFF;
const DELETED_GLYPH: u16 = 0xFFFF;
const HEADER_WITH_ONE_OFFSET: usize = 20;
const LIGATURE_HEADER: usize = 28;
const LIG_LAST: u32 = 0x8000_0000;
const LIG_STORE: u32 = 0x4000_0000;
const LIG_OFFSET: u32 = 0x3FFF_FFFF;
const LIG_SIGN: u32 = 0x2000_0000;
const CURRENT_INSERT_COUNT: u16 = 0x03E0;
const MARKED_INSERT_COUNT: u16 = 0x001F;
use super::super::super::format::aat::Lookup;
use super::super::otl::remap_gid;
use super::lookup::build_aat_lookup;
use super::state::{assemble, entries_of, entry_table, state_parts, subset_state_table};

fn subset_non_contextual(
    body: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let lookup = Lookup::parse(body, num_glyphs)?;
    let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
        .filter_map(|(from, to)| {
            Some((remap_gid(active, gid_map, from)?, remap_gid(active, gid_map, to)?))
        })
        .collect();
    build_aat_lookup(&kept)
}

fn subset_contextual(
    body: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let subst_off = read_u32_be(body, 16)? as usize;
    let parts = state_parts(body, &[subst_off], active, gid_map, num_glyphs)?;

    let n_lookups = entries_of(parts.entry, 2).iter()
        .flat_map(|&(_, w1, w2)| [w1, w2])
        .filter(|&w| w != NO_INDEX)
        .map(|w| w as usize + 1)
        .max()
        .unwrap_or(0);

    let substitutions = body.get(subst_off..)?;
    let mut offsets: Vec<u32> = Vec::with_capacity(n_lookups);
    let mut blob: Vec<u8> = Vec::new();
    let table_len = 4 * n_lookups;
    for i in 0..n_lookups {
        let rebuilt = read_u32_be(substitutions, 4 * i)
            .and_then(|off| Lookup::parse(substitutions.get(off as usize..)?, num_glyphs))
            .and_then(|lookup| {
                let kept: Vec<(u16, u16)> = lookup.entries().into_iter()
                    .filter_map(|(from, to)| {
                        Some((remap_gid(active, gid_map, from)?, remap_gid(active, gid_map, to)?))
                    })
                    .collect();
                build_aat_lookup(&kept)
            });
        match rebuilt {
            Some(bytes) => {
                offsets.push((table_len + blob.len()) as u32);
                blob.extend_from_slice(&bytes);
            }
            None => offsets.push(0),
        }
    }

    let mut table: Vec<u8> = Vec::with_capacity(table_len + blob.len());
    for o in &offsets { table.extend_from_slice(&o.to_be_bytes()); }
    table.extend_from_slice(&blob);

    let mut out = assemble(&parts, HEADER_WITH_ONE_OFFSET, &[&table])?;
    let subst_at = (out.len() - table.len()) as u32;
    out.get_mut(16..20)?.copy_from_slice(&subst_at.to_be_bytes());
    Some(out)
}

fn subset_insertion(
    body: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let action_off = read_u32_be(body, 16)? as usize;
    let parts = state_parts(body, &[action_off], active, gid_map, num_glyphs)?;

    let n_glyphs_in_table = entries_of(parts.entry, 2).iter()
        .flat_map(|&(flags, w1, w2)| [
            (w1, (flags & CURRENT_INSERT_COUNT) >> 5),
            (w2, flags & MARKED_INSERT_COUNT),
        ])
        .filter(|&(w, _)| w != NO_INDEX)
        .map(|(w, count)| w as usize + count as usize)
        .max()
        .unwrap_or(0);

    let actions = body.get(action_off..)?;
    let mut table: Vec<u8> = Vec::with_capacity(2 * n_glyphs_in_table);
    for i in 0..n_glyphs_in_table {
        let g = read_u16_be(actions, 2 * i)?;
        let new = remap_gid(active, gid_map, g).unwrap_or(DELETED_GLYPH);
        table.extend_from_slice(&new.to_be_bytes());
    }

    let mut out = assemble(&parts, HEADER_WITH_ONE_OFFSET, &[&table])?;
    let action_at = (out.len() - table.len()) as u32;
    out.get_mut(16..20)?.copy_from_slice(&action_at.to_be_bytes());
    Some(out)
}

fn subset_ligature(
    body: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let class_off = read_u32_be(body, 4)? as usize;
    let action_off = read_u32_be(body, 16)? as usize;
    let component_off = read_u32_be(body, 20)? as usize;
    let ligature_off = read_u32_be(body, 24)? as usize;

    let extras = [action_off, component_off, ligature_off];
    let parts = state_parts(body, &extras, active, gid_map, num_glyphs)?;

    let end_of = |from: usize| {
        [read_u32_be(body, 4).unwrap_or(0) as usize, read_u32_be(body, 8).unwrap_or(0) as usize,
         read_u32_be(body, 12).unwrap_or(0) as usize, action_off, component_off, ligature_off, body.len()]
            .into_iter().filter(|&o| o > from).min().unwrap_or(body.len())
    };
    let n_actions = end_of(action_off).saturating_sub(action_off) / 4;
    let n_components = end_of(component_off).saturating_sub(component_off) / 2;
    let n_ligatures = end_of(ligature_off).saturating_sub(ligature_off) / 2;
    if n_actions == 0 || n_components == 0 { return None; }

    let classed: Vec<(u16, u16)> = Lookup::parse(body.get(class_off..end_of(class_off))?, num_glyphs)?
        .entries().into_iter()
        .filter_map(|(g, _)| remap_gid(active, gid_map, g).map(|ng| (g, ng)))
        .collect();

    let mut components: Vec<u16> = Vec::new();
    let mut actions: Vec<u32> = Vec::with_capacity(n_actions);
    for i in 0..n_actions {
        let word = read_u32_be(body, action_off + i * 4)?;
        let mut offset = (word & LIG_OFFSET) as i32;
        if word & LIG_SIGN != 0 { offset -= (LIG_OFFSET + 1) as i32; }

        let reaching: Vec<(u16, u16)> = classed.iter().copied()
            .filter(|&(g, _)| {
                let at = g as i32 + offset;
                at >= 0 && (at as usize) < n_components
            })
            .collect();

        let new_offset = match (reaching.iter().map(|&(_, ng)| ng).min(), reaching.iter().map(|&(_, ng)| ng).max()) {
            (Some(lo), Some(hi)) => {
                let base = components.len();
                components.resize(base + (hi - lo) as usize + 1, 0);
                for (g, ng) in reaching {
                    let from = (g as i32 + offset) as usize;
                    components[base + (ng - lo) as usize] = read_u16_be(body, component_off + from * 2)?;
                }
                base as i32 - lo as i32
            }
            _ => 0,
        };
        if !(-(1 << 29)..(1 << 29)).contains(&new_offset) { return None; }
        actions.push((word & (LIG_LAST | LIG_STORE)) | (new_offset as u32 & LIG_OFFSET));
    }

    let mut ligatures: Vec<u16> = Vec::with_capacity(n_ligatures);
    for i in 0..n_ligatures {
        let g = read_u16_be(body, ligature_off + i * 2)?;
        ligatures.push(remap_gid(active, gid_map, g)?);
    }

    let class_table = build_aat_lookup(&parts.classes)?;
    let new_class = LIGATURE_HEADER;
    let new_state = new_class + class_table.len();
    let new_entry = new_state + parts.state.len();
    let new_action = new_entry + parts.entry.len();
    let new_component = new_action + actions.len() * 4;
    let new_ligature = new_component + components.len() * 2;

    let mut out = alloc::vec![0u8; LIGATURE_HEADER];
    for (i, v) in [parts.n_classes, new_class as u32, new_state as u32, new_entry as u32,
                   new_action as u32, new_component as u32, new_ligature as u32].iter().enumerate() {
        out.get_mut(i * 4..i * 4 + 4)?.copy_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&class_table);
    out.extend_from_slice(parts.state);
    out.extend_from_slice(parts.entry);
    for a in &actions { out.extend_from_slice(&a.to_be_bytes()); }
    for c in &components { out.extend_from_slice(&c.to_be_bytes()); }
    for l in &ligatures { out.extend_from_slice(&l.to_be_bytes()); }
    Some(out)
}

fn subset_subtable(
    body: &[u8], coverage: u32, active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    match coverage & 0xFF {
        0 => subset_state_table(body, active, gid_map, num_glyphs),
        1 => subset_contextual(body, active, gid_map, num_glyphs),
        4 => subset_non_contextual(body, active, gid_map, num_glyphs),
        2 => subset_ligature(body, active, gid_map, num_glyphs),
        5 => subset_insertion(body, active, gid_map, num_glyphs),
        _ => None,
    }
}

fn each_subtable(morx: &[u8], mut visit: impl FnMut(&[u8], u32)) -> Option<()> {
    let n_chains = read_u32_be(morx, 4)?;
    let mut at = 8usize;
    for _ in 0..n_chains {
        let length = read_u32_be(morx, at + 4)? as usize;
        let n_features = read_u32_be(morx, at + 8)? as usize;
        let n_subtables = read_u32_be(morx, at + 12)?;
        let chain = morx.get(at..at.checked_add(length)?)?;

        let mut sub_at = 16 + n_features.checked_mul(12)?;
        for _ in 0..n_subtables {
            let sub_len = read_u32_be(chain, sub_at)? as usize;
            let coverage = read_u32_be(chain, sub_at + 4)?;
            visit(chain.get(sub_at + 12..sub_at.checked_add(sub_len)?)?, coverage);
            let next = sub_at.checked_add(sub_len)?;
            if sub_len == 0 || next > chain.len() { break; }
            sub_at = next;
        }
        let next = at.checked_add(length)?;
        if length == 0 || next > morx.len() { break; }
        at = next;
    }
    Some(())
}

pub fn morx_closure(morx: &[u8], active: &GlyphSet, num_glyphs: u16) -> Vec<u16> {
    let mut found = Vec::new();
    each_subtable(morx, |body, coverage| {
        match coverage & 0xFF {
            1 => {
                let Some(subst_off) = read_u32_be(body, 16).map(|o| o as usize) else { return };
                let Some(entry) = entry_table(body, &[subst_off]) else { return };
                let n = entries_of(entry, 2).iter()
                    .flat_map(|&(_, w1, w2)| [w1, w2])
                    .filter(|&w| w != NO_INDEX)
                    .map(|w| w as usize + 1)
                    .max().unwrap_or(0);
                let Some(table) = body.get(subst_off..) else { return };
                for i in 0..n {
                    let Some(off) = read_u32_be(table, 4 * i) else { break };
                    let Some(lookup) = table.get(off as usize..).and_then(|d| Lookup::parse(d, num_glyphs))
                    else { continue };
                    found.extend(lookup.entries().into_iter().map(|(_, to)| to));
                }
            }
            2 => {
                let Some(lig_off) = read_u32_be(body, 24).map(|o| o as usize) else { return };
                let mut at = lig_off;
                while let Some(g) = read_u16_be(body, at) {
                    found.push(g);
                    at += 2;
                }
            }
            4 => {
                let Some(lookup) = Lookup::parse(body, num_glyphs) else { return };
                found.extend(lookup.entries().into_iter()
                    .filter(|(from, _)| active.contains(from))
                    .map(|(_, to)| to));
            }
            5 => {
                let Some(action_off) = read_u32_be(body, 16).map(|o| o as usize) else { return };
                let Some(entry) = entry_table(body, &[action_off]) else { return };
                let n = entries_of(entry, 2).iter()
                    .flat_map(|&(flags, w1, w2)| [
                        (w1, (flags & CURRENT_INSERT_COUNT) >> 5),
                        (w2, flags & MARKED_INSERT_COUNT),
                    ])
                    .filter(|&(w, _)| w != NO_INDEX)
                    .map(|(w, c)| w as usize + c as usize)
                    .max().unwrap_or(0);
                for i in 0..n {
                    if let Some(g) = read_u16_be(body, action_off + i * 2) { found.push(g); }
                }
            }
            _ => {}
        }
    });
    found.retain(|g| *g != DELETED_GLYPH);
    found
}

pub fn subset_morx(
    morx: &[u8], active: &GlyphSet, gid_map: &[u16], num_glyphs: u16,
) -> Option<Vec<u8>> {
    let n_chains = read_u32_be(morx, 4)?;
    let mut chains: Vec<Vec<u8>> = Vec::new();

    let mut at = 8usize;
    for _ in 0..n_chains {
        let length = read_u32_be(morx, at + 4)? as usize;
        let n_features = read_u32_be(morx, at + 8)? as usize;
        let n_subtables = read_u32_be(morx, at + 12)?;
        let chain = morx.get(at..at.checked_add(length)?)?;

        let prefix_len = 16 + n_features.checked_mul(12)?;
        let mut rebuilt = chain.get(..prefix_len)?.to_vec();

        let mut sub_at = prefix_len;
        for _ in 0..n_subtables {
            let sub_len = read_u32_be(chain, sub_at)? as usize;
            let coverage = read_u32_be(chain, sub_at + 4)?;
            let body = chain.get(sub_at + 12..sub_at.checked_add(sub_len)?)?;

            let new_body = subset_subtable(body, coverage, active, gid_map, num_glyphs)?;
            let new_len = 12 + new_body.len();
            rebuilt.extend_from_slice(&(new_len as u32).to_be_bytes());
            rebuilt.extend_from_slice(chain.get(sub_at + 4..sub_at + 12)?);
            rebuilt.extend_from_slice(&new_body);

            let next = sub_at.checked_add(sub_len)?;
            if sub_len == 0 || next > chain.len() { break; }
            sub_at = next;
        }

        let rebuilt_len = rebuilt.len() as u32;
        rebuilt.get_mut(4..8)?.copy_from_slice(&rebuilt_len.to_be_bytes());
        chains.push(rebuilt);

        let next = at.checked_add(length)?;
        if length == 0 || next > morx.len() { break; }
        at = next;
    }
    if chains.is_empty() { return None; }

    let mut out = morx.get(..4)?.to_vec();
    out.extend_from_slice(&(chains.len() as u32).to_be_bytes());
    for c in &chains { out.extend_from_slice(c); }
    Some(out)
}
