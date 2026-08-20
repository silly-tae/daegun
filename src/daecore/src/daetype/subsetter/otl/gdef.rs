use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use crate::daecore::daetype::decoder::{read_u16_be, read_u32_be, write_u16_be, write_u32_be, read_i16_be, write_i16_be};
use super::generic::{self, schemas};

pub fn subset_gdef(gdef: &[u8], active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    if gdef.len() < 12 { return None; }
    let major = read_u16_be(gdef, 0)?;
    let minor = read_u16_be(gdef, 2)?;
    let glyph_class_off = read_u16_be(gdef, 4)? as usize;
    let attach_list_off = read_u16_be(gdef, 6)? as usize;
    let lig_caret_list_off = read_u16_be(gdef, 8)? as usize;
    let mark_attach_off = read_u16_be(gdef, 10)? as usize;
    let mark_glyph_sets_off = if minor >= 2 && gdef.len() >= 14 { read_u16_be(gdef, 12)? as usize } else { 0 };
    let item_var_store = if minor >= 3 && gdef.len() >= 18 {
        match read_u32_be(gdef, 14)? as usize {
            0 => None,
            off => gdef.get(off..).map(<[u8]>::to_vec),
        }
    } else {
        None
    };

    let glyph_class = if glyph_class_off != 0 { generic::subset_subtable(gdef, glyph_class_off, &schemas::gdef::glyph_class_def_schema(), active, gid_map) } else { None };
    let attach_list = if attach_list_off != 0 { generic::subset_subtable(gdef, attach_list_off, &schemas::gdef::attach_list_schema(), active, gid_map) } else { None };
    let lig_caret_list = if lig_caret_list_off != 0 { generic::subset_subtable(gdef, lig_caret_list_off, &schemas::gdef::lig_caret_list_schema(), active, gid_map) } else { None };
    let mark_attach = if mark_attach_off != 0 { generic::subset_subtable(gdef, mark_attach_off, &schemas::gdef::mark_attach_class_def_schema(), active, gid_map) } else { None };
    let mark_glyph_sets = if mark_glyph_sets_off != 0 { Some(generic::subset_subtable(gdef, mark_glyph_sets_off, &schemas::gdef::mark_glyph_sets_schema(), active, gid_map)?) } else { None };

    if glyph_class.is_none() && attach_list.is_none() && lig_caret_list.is_none()
        && mark_attach.is_none() && mark_glyph_sets.is_none() && item_var_store.is_none()
    {
        return None;
    }

    let has_mgs = mark_glyph_sets.is_some();
    let has_ivs = item_var_store.is_some();
    let header_len = if has_ivs { 18 } else if has_mgs { 14 } else { 12 };
    let mut out = vec![0u8; header_len];
    write_u16_be(&mut out, 0, major);
    write_u16_be(&mut out, 2, if has_ivs { 3 } else if has_mgs { 2 } else { 0 });
    let mut tail = Vec::new();
    if let Some(cd) = &glyph_class {
        write_u16_be(&mut out, 4, (header_len + tail.len()) as u16);
        tail.extend_from_slice(cd);
    }
    if let Some(al) = &attach_list {
        write_u16_be(&mut out, 6, (header_len + tail.len()) as u16);
        tail.extend_from_slice(al);
    }
    if let Some(lcl) = &lig_caret_list {
        write_u16_be(&mut out, 8, (header_len + tail.len()) as u16);
        tail.extend_from_slice(lcl);
    }
    if let Some(cd) = &mark_attach {
        write_u16_be(&mut out, 10, (header_len + tail.len()) as u16);
        tail.extend_from_slice(cd);
    }
    if let Some(mgs) = &mark_glyph_sets {
        write_u16_be(&mut out, 12, (header_len + tail.len()) as u16);
        tail.extend_from_slice(mgs);
    }
    if let Some(ivs) = &item_var_store {
        write_u32_be(&mut out, 14, (header_len + tail.len()) as u32);
        tail.extend_from_slice(ivs);
    }
    out.extend(tail);
    Some(out)
}

pub fn glyph_class(gdef: &[u8], glyph: u16) -> u16 {
    class_at(gdef, 4, glyph)
}

pub fn mark_attach_class(gdef: &[u8], glyph: u16) -> u16 {
    class_at(gdef, 10, glyph)
}

fn class_at(gdef: &[u8], header_offset: usize, glyph: u16) -> u16 {
    let Some(off) = read_u16_be(gdef, header_offset).map(usize::from).filter(|&o| o != 0) else {
        return 0;
    };
    let Some(format) = read_u16_be(gdef, off) else { return 0 };
    match format {
        1 => {
            let (Some(start), Some(count)) =
                (read_u16_be(gdef, off + 2), read_u16_be(gdef, off + 4)) else { return 0 };
            if glyph < start || glyph - start >= count { return 0; }
            read_u16_be(gdef, off + 6 + usize::from(glyph - start) * 2).unwrap_or(0)
        }
        2 => {
            let Some(count) = read_u16_be(gdef, off + 2) else { return 0 };
            for i in 0..usize::from(count) {
                let rec = off + 4 + i * 6;
                let (Some(lo), Some(hi), Some(class)) = (
                    read_u16_be(gdef, rec),
                    read_u16_be(gdef, rec + 2),
                    read_u16_be(gdef, rec + 4),
                ) else { return 0 };
                if glyph < lo { return 0; }
                if glyph <= hi { return class; }
            }
            0
        }
        _ => 0,
    }
}

pub fn has_mark_glyph_sets(gdef: &[u8]) -> bool {
    if gdef.len() < 14 { return false; }
    let Some(minor) = read_u16_be(gdef, 2) else { return false };
    if minor < 2 { return false; }
    read_u16_be(gdef, 12).is_some_and(|off| off != 0)
}

pub(crate) enum CaretValue {
    Coordinate(i16),
    Point(u16),
}

pub(crate) fn parse_caret_value(buf: &[u8], off: usize) -> Option<CaretValue> {
    match read_u16_be(buf, off)? {
        1 => Some(CaretValue::Coordinate(read_i16_be(buf, off + 2)?)),
        2 => Some(CaretValue::Point(read_u16_be(buf, off + 2)?)),
        3 => Some(CaretValue::Coordinate(read_i16_be(buf, off + 2)?)),
        _ => None,
    }
}

pub(crate) fn build_caret_value(cv: &CaretValue) -> Vec<u8> {
    let mut out = vec![0u8; 4];
    match cv {
        CaretValue::Coordinate(c) => { write_u16_be(&mut out, 0, 1); write_i16_be(&mut out, 2, *c); }
        CaretValue::Point(p) => { write_u16_be(&mut out, 0, 2); write_u16_be(&mut out, 2, *p); }
    }
    out
}
