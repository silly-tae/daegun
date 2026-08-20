use super::super::decoder::{mac_roman_byte, mac_roman_char, read_u16_be, read_u32_be, read_u24_be, search_records};

pub fn cmap_glyph_id(cmap: &[u8], codepoint: u32) -> Option<u16> {
    if cmap.len() < 4 { return None; }
    let num_tables = read_u16_be(cmap, 2)? as usize;
    let mut f4_result: Option<u16> = None;
    let mut legacy_result: Option<u16> = None;

    for i in 0..num_tables {
        let rec = 4 + i * 8;
        if rec + 8 > cmap.len() { break; }
        let platform_id  = match read_u16_be(cmap, rec)     { Some(v) => v, None => break };
        let encoding_id   = match read_u16_be(cmap, rec + 2) { Some(v) => v, None => break };
        let subtable_off  = match read_u32_be(cmap, rec + 4) { Some(v) => v as usize, None => break };
        if subtable_off + 2 > cmap.len() { continue; }
        let format = match read_u16_be(cmap, subtable_off) { Some(v) => v, None => continue };

        match (platform_id, encoding_id, format) {
            (_, _, 12 | 13) => {
                if let Some(gid) = segmented_lookup(cmap, subtable_off, codepoint, format) {
                    return Some(gid);
                }
            }
            (_, _, 10) => {
                if let Some(gid) = format10_lookup(cmap, subtable_off, codepoint) {
                    return Some(gid);
                }
            }
            (_, _, 4) if codepoint <= 0xFFFF => {
                if f4_result.is_none() {
                    f4_result = format4_lookup(cmap, subtable_off, codepoint as u16);
                }
            }
            (_, _, 6) if codepoint <= 0xFFFF => {
                if legacy_result.is_none() {
                    legacy_result = format6_lookup(cmap, subtable_off, codepoint as u16);
                }
            }
            (1, 0, 0) if legacy_result.is_none() => {
                if let Some(byte) = mac_roman_byte(codepoint) {
                    legacy_result = format0_lookup(cmap, subtable_off, byte);
                }
            }
            (_, _, 0) if codepoint <= 0xFF
                && legacy_result.is_none() => {
                    legacy_result = format0_lookup(cmap, subtable_off, codepoint as u8);
                }
            _ => {}
        }
    }
    f4_result.or(legacy_result)
}

// Glyph 0 is `.notdef` by definition, so a lookup resolving to it means "no glyph for this", never
// a usable result. Applied at every format's exit, because the merge logic depends on "mapped to
// .notdef" and "not mapped" staying indistinguishable.
fn notdef_is_miss(gid: u16) -> Option<u16> {
    if gid == 0 { None } else { Some(gid) }
}

fn format6_lookup(cmap: &[u8], base: usize, cp: u16) -> Option<u16> {
    let first = read_u16_be(cmap, base + 6)?;
    let count = read_u16_be(cmap, base + 8)?;
    if cp < first { return None; }
    let idx = (cp - first) as usize;
    if idx >= count as usize { return None; }
    read_u16_be(cmap, base + 10 + idx * 2).and_then(notdef_is_miss)
}

fn format10_lookup(cmap: &[u8], base: usize, codepoint: u32) -> Option<u16> {
    let first = read_u32_be(cmap, base + 12)?;
    let count = read_u32_be(cmap, base + 16)?;
    let idx = codepoint.checked_sub(first)?;
    if idx >= count {
        return None;
    }
    let at = base.checked_add(20)?.checked_add((idx as usize).checked_mul(2)?)?;
    read_u16_be(cmap, at).and_then(notdef_is_miss)
}

fn format0_lookup(cmap: &[u8], base: usize, cp: u8) -> Option<u16> {
    notdef_is_miss(cmap.get(base + 6 + cp as usize).copied().unwrap_or(0) as u16)
}

fn segmented_lookup(cmap: &[u8], base: usize, codepoint: u32, format: u16) -> Option<u16> {
    if base + 16 > cmap.len() { return None; }
    let num_groups  = read_u32_be(cmap, base + 12)? as usize;
    let groups_base = base + 16;
    if groups_base + num_groups * 12 > cmap.len() { return None; }

    let cand = match search_records(num_groups, codepoint, |i| read_u32_be(cmap, groups_base + i * 12)) {
        Some(Ok(i)) => i,
        Some(Err(0)) | None => return None,
        Some(Err(i)) => i - 1,
    };
    let off = groups_base + cand * 12;
    let (Some(start), Some(end)) = (read_u32_be(cmap, off), read_u32_be(cmap, off + 4)) else { return None };
    if codepoint < start || codepoint > end { return None; }
    let glyph = read_u32_be(cmap, off + 8)? as u64;
    let gid = if format == 13 { glyph } else { glyph + (codepoint - start) as u64 };
    if gid > 0xFFFF { None } else { notdef_is_miss(gid as u16) }
}

pub enum UvsLookup {
    Explicit(u16),
    UseDefault,
}

pub fn cmap_variation_glyph_id(cmap: &[u8], base: u32, selector: u32) -> Option<UvsLookup> {
    let base14 = find_format14_subtable(cmap)?;
    let (default_off, non_default_off) = find_var_selector_record(cmap, base14, selector)?;

    if non_default_off != 0
        && let Some(gid) = lookup_non_default_uvs(cmap, base14 + non_default_off as usize, base) {
            return Some(UvsLookup::Explicit(gid));
        }
    if default_off != 0 && lookup_default_uvs(cmap, base14 + default_off as usize, base) {
        return Some(UvsLookup::UseDefault);
    }
    None
}

fn find_format14_subtable(cmap: &[u8]) -> Option<usize> {
    if cmap.len() < 4 { return None; }
    let num_tables = read_u16_be(cmap, 2)? as usize;
    for i in 0..num_tables {
        let rec = 4 + i * 8;
        if rec + 8 > cmap.len() { break; }
        let platform_id = read_u16_be(cmap, rec)?;
        let encoding_id = read_u16_be(cmap, rec + 2)?;
        if platform_id != 0 || encoding_id != 5 { continue; }
        let off = read_u32_be(cmap, rec + 4)? as usize;
        if off + 2 <= cmap.len() && read_u16_be(cmap, off) == Some(14) {
            return Some(off);
        }
    }
    None
}

fn find_var_selector_record(cmap: &[u8], base14: usize, selector: u32) -> Option<(u32, u32)> {
    let num_records = read_u32_be(cmap, base14 + 6)? as usize;
    let records_off  = base14 + 10;
    let hit = search_records(num_records, selector, |i| read_u24_be(cmap, records_off + i * 11))?.ok()?;
    let rec = records_off + hit * 11;
    let default_off     = read_u32_be(cmap, rec + 3)?;
    let non_default_off = read_u32_be(cmap, rec + 7)?;
    Some((default_off, non_default_off))
}

fn lookup_non_default_uvs(cmap: &[u8], base: usize, codepoint: u32) -> Option<u16> {
    let num_mappings = read_u32_be(cmap, base)? as usize;
    let map_start    = base + 4;
    let hit = search_records(num_mappings, codepoint, |i| read_u24_be(cmap, map_start + i * 5))?.ok()?;
    read_u16_be(cmap, map_start + hit * 5 + 3)
}

fn lookup_default_uvs(cmap: &[u8], base: usize, codepoint: u32) -> bool {
    let num_ranges  = match read_u32_be(cmap, base) { Some(v) => v as usize, None => return false };
    let range_start = base + 4;
    let cand = match search_records(num_ranges, codepoint, |i| read_u24_be(cmap, range_start + i * 4)) {
        Some(Ok(_)) => return true,
        Some(Err(0)) | None => return false,
        Some(Err(i)) => i - 1,
    };
    let rec = range_start + cand * 4;
    let start = match read_u24_be(cmap, rec) { Some(v) => v, None => return false };
    let additional = match cmap.get(rec + 3) { Some(&b) => b as u32, None => return false };
    codepoint >= start && codepoint <= start + additional
}

pub fn cmap_entries(cmap: &[u8], cap: usize) -> Option<alloc::vec::Vec<(u32, u16)>> {
    let num_tables = read_u16_be(cmap, 2)? as usize;
    let mut subtables: alloc::vec::Vec<(u16, u16, u16, usize)> = alloc::vec::Vec::new();
    for i in 0..num_tables {
        let rec = 4 + i * 8;
        if rec + 8 > cmap.len() {
            break;
        }
        let (Some(platform), Some(encoding), Some(off)) =
            (read_u16_be(cmap, rec), read_u16_be(cmap, rec + 2), read_u32_be(cmap, rec + 4))
        else {
            break;
        };
        let off = off as usize;
        if off + 2 > cmap.len() {
            continue;
        }
        let Some(format) = read_u16_be(cmap, off) else { continue };
        subtables.push((platform, encoding, format, off));
    }

    let mut map: alloc::vec::Vec<(u32, u16)> = alloc::vec::Vec::new();
    let compact_at = cap.saturating_add(cap / 4);
    let emit = |map: &mut alloc::vec::Vec<(u32, u16)>, cp: u32, gid: u16| -> bool {
        if gid != 0 {
            map.push((cp, gid));
            if map.len() > compact_at {
                map.sort_by_key(|&(cp, _)| cp);
                map.dedup_by_key(|&mut (cp, _)| cp);
            }
        }
        map.len() <= cap
    };

    let mut work = cap.saturating_mul(4);

    for &(_platform, _encoding, format, off) in &subtables {
        if !matches!(format, 12 | 13) {
            continue;
        }
        let groups = read_u32_be(cmap, off + 12)? as usize;
        for g in 0..groups {
            let rec = off + 16 + g * 12;
            let (Some(start), Some(end), Some(first_gid)) =
                (read_u32_be(cmap, rec), read_u32_be(cmap, rec + 4), read_u32_be(cmap, rec + 8))
            else {
                return None;
            };
            if end < start || end.saturating_sub(start) as usize > cap {
                return None;
            }
            for cp in start..=end {
                work = work.checked_sub(1)?;
                let gid = if format == 13 { first_gid as u64 } else { first_gid as u64 + (cp - start) as u64 };
                if gid <= 0xFFFF && !emit(&mut map, cp, gid as u16) {
                    return None;
                }
            }
        }
    }

    for &(_platform, _encoding, format, off) in &subtables {
        if format != 10 {
            continue;
        }
        let (Some(first), Some(count)) = (read_u32_be(cmap, off + 12), read_u32_be(cmap, off + 16))
        else {
            return None;
        };
        if count as usize > cap {
            return None;
        }
        for i in 0..count {
            work = work.checked_sub(1)?;
            let Some(cp) = first.checked_add(i) else { break };
            let gid = read_u16_be(cmap, off + 20 + i as usize * 2)?;
            if !emit(&mut map, cp, gid) {
                return None;
            }
        }
    }

    for &(_platform, _encoding, format, off) in &subtables {
        if format != 4 {
            continue;
        }
        let seg_count = read_u16_be(cmap, off + 6)? as usize / 2;
        let ends = off + 14;
        let starts = ends + seg_count * 2 + 2;
        let deltas = starts + seg_count * 2;
        let range_offsets = deltas + seg_count * 2;

        for i in 0..seg_count {
            let (Some(end), Some(start), Some(delta), Some(range_offset)) = (
                read_u16_be(cmap, ends + i * 2),
                read_u16_be(cmap, starts + i * 2),
                read_u16_be(cmap, deltas + i * 2),
                read_u16_be(cmap, range_offsets + i * 2),
            ) else {
                return None;
            };
            if start > end {
                continue;
            }
            for cp in start..=end {
                work = work.checked_sub(1)?;
                let gid = if range_offset == 0 {
                    (cp as u32 + delta as u32) & 0xFFFF
                } else {
                    let at = range_offsets + i * 2 + range_offset as usize + (cp - start) as usize * 2;
                    match read_u16_be(cmap, at) {
                        Some(0) | None => continue,
                        Some(g) => (g as u32 + delta as u32) & 0xFFFF,
                    }
                };
                if !emit(&mut map, cp as u32, gid as u16) {
                    return None;
                }
                if cp == u16::MAX {
                    break;
                }
            }
        }
    }

    for &(platform, _, format, off) in &subtables {
        match format {
            6 => {
                let first = read_u16_be(cmap, off + 6)? as u32;
                let count = read_u16_be(cmap, off + 8)? as u32;
                for i in 0..count {
                    work = work.checked_sub(1)?;
                    let Some(gid) = read_u16_be(cmap, off + 10 + i as usize * 2) else { break };
                    if !emit(&mut map, first + i, gid) {
                        return None;
                    }
                }
            }
            0 => {
                for byte in 0..=0xFFu32 {
                    work = work.checked_sub(1)?;
                    let Some(gid) = format0_lookup(cmap, off, byte as u8) else { continue };
                    let cp = if platform == 1 { mac_roman_char(byte as u8) as u32 } else { byte };
                    if !emit(&mut map, cp, gid) {
                        return None;
                    }
                }
            }
            _ => {}
        }
    }

    map.sort_by_key(|&(cp, _)| cp);
    map.dedup_by_key(|&mut (cp, _)| cp);
    Some(map)
}

fn format4_lookup(cmap: &[u8], base: usize, cp: u16) -> Option<u16> {
    if base + 14 > cmap.len() { return None; }
    let seg_count = read_u16_be(cmap, base + 6)? as usize / 2;

    let end_off   = base + 14;
    let start_off = end_off + seg_count * 2 + 2;
    let delta_off = start_off + seg_count * 2;
    let range_off = delta_off + seg_count * 2;

    let i = match search_records(seg_count, cp as u32, |k| read_u16_be(cmap, end_off + k * 2).map(u32::from)) {
        Some(Ok(k)) | Some(Err(k)) => k,
        None => return None,
    };
    if i >= seg_count { return None; }

    let start = read_u16_be(cmap, start_off + i * 2)?;
    if cp < start { return None; }

    let delta        = read_u16_be(cmap, delta_off + i * 2)? as u32;
    let range_offset = read_u16_be(cmap, range_off + i * 2)? as usize;

    if range_offset == 0 {
        notdef_is_miss(((cp as u32 + delta) & 0xFFFF) as u16)
    } else {
        let gid_off = range_off + i * 2 + range_offset + (cp - start) as usize * 2;
        if gid_off + 2 > cmap.len() { return None; }
        let g = read_u16_be(cmap, gid_off).unwrap_or(0);
        if g == 0 { None } else { notdef_is_miss(((g as u32 + delta) & 0xFFFF) as u16) }
    }
}
