use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::decoder::{read_u16_be, records_fit};
use crate::daecore::daetype::TableBytes;

fn find_jstf_script_table(jstf: &[u8], script_tag: &str) -> Option<usize> {
    if jstf.len() < 6 { return None; }
    let script_count = read_u16_be(jstf, 4)? as usize;
    for i in 0..script_count {
        let rec = 6 + i * 6;
        let tag_bytes = jstf.get(rec..rec + 4)?;
        if core::str::from_utf8(tag_bytes).unwrap_or("") != script_tag { continue; }
        return Some(read_u16_be(jstf, rec + 4)? as usize);
    }
    None
}

pub fn jstf_extender_glyphs(table_map: &BTreeMap<String, TableBytes>, script_tag: &str) -> Option<Vec<u16>> {
    let jstf = table_map.get("JSTF")?;
    let jstf_script_table = find_jstf_script_table(jstf, script_tag)?;

    let extender_off = read_u16_be(jstf, jstf_script_table)? as usize;
    if extender_off == 0 { return None; }
    let extender_table = jstf_script_table + extender_off;

    let glyph_count = read_u16_be(jstf, extender_table)? as usize;
    if !records_fit(extender_table + 2, glyph_count, 2, jstf.len()) { return None; }
    let mut glyphs = Vec::with_capacity(glyph_count);
    for i in 0..glyph_count {
        glyphs.push(read_u16_be(jstf, extender_table + 2 + i * 2)?);
    }
    Some(glyphs)
}

#[derive(Debug, PartialEq)]
pub struct JstfModLists {
    pub shrinkage_enable_gsub:  Option<Vec<u16>>,
    pub shrinkage_disable_gsub: Option<Vec<u16>>,
    pub shrinkage_enable_gpos:  Option<Vec<u16>>,
    pub shrinkage_disable_gpos: Option<Vec<u16>>,
    pub shrinkage_jstf_max:     Option<usize>,
    pub extension_enable_gsub:  Option<Vec<u16>>,
    pub extension_disable_gsub: Option<Vec<u16>>,
    pub extension_enable_gpos:  Option<Vec<u16>>,
    pub extension_disable_gpos: Option<Vec<u16>>,
    pub extension_jstf_max:     Option<usize>,
}

fn read_mod_list(
    jstf: &[u8],
    priority_table: usize,
    field_off: usize,
    indices_left: &mut usize,
) -> Result<Option<Vec<u16>>, ()> {
    let off = read_u16_be(jstf, priority_table + field_off).ok_or(())? as usize;
    if off == 0 { return Ok(None); }
    let list_table = priority_table + off;
    let count = read_u16_be(jstf, list_table).ok_or(())? as usize;
    if !records_fit(list_table + 2, count, 2, jstf.len()) { return Err(()); }
    *indices_left = indices_left.checked_sub(count).ok_or(())?;
    let mut indices = Vec::with_capacity(count);
    for i in 0..count {
        indices.push(read_u16_be(jstf, list_table + 2 + i * 2).ok_or(())?);
    }
    Ok(Some(indices))
}

fn read_jstf_max_offset(jstf: &[u8], priority_table: usize, field_off: usize) -> Result<Option<usize>, ()> {
    let off = read_u16_be(jstf, priority_table + field_off).ok_or(())? as usize;
    if off == 0 { return Ok(None); }
    Ok(Some(priority_table + off))
}

fn read_jstf_priority(jstf: &[u8], priority_table: usize, indices_left: &mut usize) -> Option<JstfModLists> {
    Some(JstfModLists {
        shrinkage_enable_gsub:  read_mod_list(jstf, priority_table, 0, indices_left).ok()?,
        shrinkage_disable_gsub: read_mod_list(jstf, priority_table, 2, indices_left).ok()?,
        shrinkage_enable_gpos:  read_mod_list(jstf, priority_table, 4, indices_left).ok()?,
        shrinkage_disable_gpos: read_mod_list(jstf, priority_table, 6, indices_left).ok()?,
        shrinkage_jstf_max:     read_jstf_max_offset(jstf, priority_table, 8).ok()?,
        extension_enable_gsub:  read_mod_list(jstf, priority_table, 10, indices_left).ok()?,
        extension_disable_gsub: read_mod_list(jstf, priority_table, 12, indices_left).ok()?,
        extension_enable_gpos:  read_mod_list(jstf, priority_table, 14, indices_left).ok()?,
        extension_disable_gpos: read_mod_list(jstf, priority_table, 16, indices_left).ok()?,
        extension_jstf_max:     read_jstf_max_offset(jstf, priority_table, 18).ok()?,
    })
}

pub fn jstf_priorities(
    table_map: &BTreeMap<String, TableBytes>,
    script_tag: &str,
    lang_sys_tag: Option<&str>,
) -> Option<Vec<JstfModLists>> {
    let jstf = table_map.get("JSTF")?;
    let jstf_script_table = find_jstf_script_table(jstf, script_tag)?;

    let def_lang_sys_off = read_u16_be(jstf, jstf_script_table + 2)? as usize;
    let lang_sys_count   = read_u16_be(jstf, jstf_script_table + 4)? as usize;

    let jstf_lang_sys_table = match lang_sys_tag {
        None => {
            if def_lang_sys_off == 0 { return None; }
            jstf_script_table + def_lang_sys_off
        }
        Some(tag) => {
            let mut found: Option<usize> = None;
            for i in 0..lang_sys_count {
                let rec = jstf_script_table + 6 + i * 6;
                let tag_bytes = jstf.get(rec..rec + 4)?;
                if core::str::from_utf8(tag_bytes).unwrap_or("") != tag { continue; }
                let off = read_u16_be(jstf, rec + 4)? as usize;
                found = Some(jstf_script_table + off);
                break;
            }
            found?
        }
    };

    let priority_count = read_u16_be(jstf, jstf_lang_sys_table)? as usize;
    if !records_fit(jstf_lang_sys_table + 2, priority_count, 2, jstf.len()) {
        return None;
    }
    const MAX_PRIORITY_LEVELS: usize = 64;
    if priority_count > MAX_PRIORITY_LEVELS {
        return None;
    }
    let mut indices_left = jstf.len() / 2;

    let mut levels = Vec::with_capacity(priority_count);
    for i in 0..priority_count {
        let off = read_u16_be(jstf, jstf_lang_sys_table + 2 + i * 2)? as usize;
        if off == 0 { return None; }
        let priority_table = jstf_lang_sys_table + off;
        levels.push(read_jstf_priority(jstf, priority_table, &mut indices_left)?);
    }
    Some(levels)
}
