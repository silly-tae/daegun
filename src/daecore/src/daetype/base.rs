use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::decoder::{read_u16_be, read_i16_be, records_fit};
use crate::daecore::daetype::TableBytes;

#[derive(Debug, PartialEq)]
pub struct BaseScriptInfo {
    pub default_baseline_tag: Option<String>,
    pub baseline_coords:      BTreeMap<String, i16>,
}

// Only BaseCoord format 1. Formats 2 and 3 hang the baseline off a contour point or a device table,
// and a tag they cover reads as absent rather than as an error – a wrong baseline is worse than a
// defaulted one.
// Only BaseCoord format 1. Formats 2 and 3 hang the baseline off a contour point or a device table,
// and a tag they cover reads as absent rather than as an error – a wrong baseline is worse than a
// defaulted one.
pub fn base_script_info(
    table_map: &BTreeMap<String, TableBytes>, script_tag: &str, vertical: bool,
) -> Option<BaseScriptInfo> {
    let base = table_map.get("BASE")?;
    if base.len() < 8 { return None; }

    let axis_off_field = if vertical { 6 } else { 4 };
    let axis_base = read_u16_be(base, axis_off_field)? as usize;
    if axis_base == 0 { return None; }

    let tag_list_off    = read_u16_be(base, axis_base)? as usize;
    let script_list_off = read_u16_be(base, axis_base + 2)? as usize;
    if tag_list_off == 0 || script_list_off == 0 { return None; }

    let tag_list_base = axis_base + tag_list_off;
    let tag_count = read_u16_be(base, tag_list_base)? as usize;
    if !records_fit(tag_list_base + 2, tag_count, 4, base.len()) {
        return None;
    }
    let mut tags = Vec::with_capacity(tag_count);
    for i in 0..tag_count {
        let off = tag_list_base + 2 + i * 4;
        let tag_bytes = base.get(off..off + 4)?;
        tags.push(String::from_utf8_lossy(tag_bytes).to_string());
    }

    let script_list_base = axis_base + script_list_off;
    let script_count = read_u16_be(base, script_list_base)? as usize;
    let mut base_script_table: Option<usize> = None;
    for i in 0..script_count {
        let rec = script_list_base + 2 + i * 6;
        let tag_bytes = base.get(rec..rec + 4)?;
        if core::str::from_utf8(tag_bytes).unwrap_or("") != script_tag { continue; }
        let off = read_u16_be(base, rec + 4)? as usize;
        base_script_table = Some(script_list_base + off);
        break;
    }
    let base_script_table = base_script_table?;

    let base_values_off = read_u16_be(base, base_script_table)? as usize;
    if base_values_off == 0 { return None; }
    let base_values_table = base_script_table + base_values_off;

    let default_baseline_index = read_u16_be(base, base_values_table)? as usize;
    let base_coord_count       = read_u16_be(base, base_values_table + 2)? as usize;
    let default_baseline_tag   = tags.get(default_baseline_index).cloned();

    let mut baseline_coords = BTreeMap::new();
    for (i, tag) in tags.iter().enumerate().take(base_coord_count) {
        let coord_off = read_u16_be(base, base_values_table + 4 + i * 2)? as usize;
        if coord_off == 0 { continue; }
        let coord_table = base_values_table + coord_off;
        if read_u16_be(base, coord_table)? != 1 { continue; }
        let coordinate = read_i16_be(base, coord_table + 2)?;
        baseline_coords.insert(tag.clone(), coordinate);
    }

    Some(BaseScriptInfo { default_baseline_tag, baseline_coords })
}

pub fn base_is_glyph_free(base: &[u8]) -> bool {
    fn names_no_glyph(base: &[u8], at: usize) -> Option<bool> {
        Some(matches!(read_u16_be(base, at)?, 1 | 3))
    }

    fn min_max_ok(base: &[u8], at: usize) -> Option<bool> {
        for slot in [0usize, 2] {
            let rel = read_u16_be(base, at + slot)?;
            if rel != 0 && !names_no_glyph(base, at + rel as usize)? { return Some(false); }
        }
        let count = read_u16_be(base, at + 4)? as usize;
        if !records_fit(at + 6, count, 6, base.len()) { return None; }
        for i in 0..count {
            for slot in [4usize, 6] {
                let rel = read_u16_be(base, at + 6 + i * 6 + slot)?;
                if rel != 0 && !names_no_glyph(base, at + rel as usize)? { return Some(false); }
            }
        }
        Some(true)
    }

    let walk = || -> Option<bool> {
        for axis_slot in [4usize, 6] {
            let axis_rel = read_u16_be(base, axis_slot)?;
            if axis_rel == 0 { continue; }
            let axis = axis_rel as usize;
            let list_rel = read_u16_be(base, axis + 2)?;
            if list_rel == 0 { continue; }
            let list = axis + list_rel as usize;

            let count = read_u16_be(base, list)? as usize;
            if !records_fit(list + 2, count, 6, base.len()) { return None; }
            for i in 0..count {
                let script = list + read_u16_be(base, list + 2 + i * 6 + 4)? as usize;

                let values_rel = read_u16_be(base, script)?;
                if values_rel != 0 {
                    let values = script + values_rel as usize;
                    let n = read_u16_be(base, values + 2)? as usize;
                    if !records_fit(values + 4, n, 2, base.len()) { return None; }
                    for k in 0..n {
                        let c = read_u16_be(base, values + 4 + k * 2)?;
                        if c != 0 && !names_no_glyph(base, values + c as usize)? { return Some(false); }
                    }
                }

                let default_min_max = read_u16_be(base, script + 2)?;
                if default_min_max != 0 && !min_max_ok(base, script + default_min_max as usize)? {
                    return Some(false);
                }

                let lang_count = read_u16_be(base, script + 4)? as usize;
                if !records_fit(script + 6, lang_count, 6, base.len()) { return None; }
                for k in 0..lang_count {
                    let rel = read_u16_be(base, script + 6 + k * 6 + 4)?;
                    if rel != 0 && !min_max_ok(base, script + rel as usize)? { return Some(false); }
                }
            }
        }
        Some(true)
    };
    walk().unwrap_or(false)
}
