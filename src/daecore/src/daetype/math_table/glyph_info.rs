use alloc::string::String;
use alloc::collections::BTreeMap;
use super::super::decoder::read_u16_be;
use super::{coverage_index, read_math_value};
use crate::daecore::daetype::TableBytes;

fn glyph_info_offset(table_map: &BTreeMap<String, TableBytes>) -> Option<(&[u8], usize)> {
    let math = table_map.get("MATH")?;
    let off = read_u16_be(math, 6)? as usize;
    Some((math, off))
}

// Coverage and the value array are parallel but independently sized, so a Coverage listing more
// glyphs than `count` would index past the array and hand back whatever follows as a measurement.
fn parallel_math_value(math: &[u8], sub_off: usize, gid: u16) -> Option<i16> {
    let cov_off = sub_off + read_u16_be(math, sub_off)? as usize;
    let count = read_u16_be(math, sub_off + 2)? as usize;
    let idx = coverage_index(math, cov_off, gid)?;
    if idx >= count {
        return None;
    }
    read_math_value(math, sub_off + 4 + idx * 4)
}

pub fn math_italics_correction(table_map: &BTreeMap<String, TableBytes>, gid: u16) -> Option<i16> {
    let (math, glyph_info_off) = glyph_info_offset(table_map)?;
    let sub_off = glyph_info_off + read_u16_be(math, glyph_info_off)? as usize;
    parallel_math_value(math, sub_off, gid)
}

pub fn math_top_accent_attachment(table_map: &BTreeMap<String, TableBytes>, gid: u16) -> Option<i16> {
    let (math, glyph_info_off) = glyph_info_offset(table_map)?;
    let sub_off = glyph_info_off + read_u16_be(math, glyph_info_off + 2)? as usize;
    parallel_math_value(math, sub_off, gid)
}

pub fn math_is_extended_shape(table_map: &BTreeMap<String, TableBytes>, gid: u16) -> bool {
    let Some((math, glyph_info_off)) = glyph_info_offset(table_map) else { return false };
    let Some(rel) = read_u16_be(math, glyph_info_off + 4) else { return false };
    if rel == 0 { return false; }
    coverage_index(math, glyph_info_off + rel as usize, gid).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathKernCorner { TopRight, TopLeft, BottomRight, BottomLeft }

pub fn math_kern(table_map: &BTreeMap<String, TableBytes>, gid: u16, corner: MathKernCorner, height: i16) -> i16 {
    let Some((math, glyph_info_off)) = glyph_info_offset(table_map) else { return 0 };
    let Some(rel) = read_u16_be(math, glyph_info_off + 6) else { return 0 };
    if rel == 0 { return 0; }
    let kern_info_off = glyph_info_off + rel as usize;

    let Some(cov_rel) = read_u16_be(math, kern_info_off) else { return 0 };
    let Some(idx) = coverage_index(math, kern_info_off + cov_rel as usize, gid) else { return 0 };

    let Some(kern_count) = read_u16_be(math, kern_info_off + 2) else { return 0 };
    if idx >= kern_count as usize { return 0; }

    let rec = kern_info_off + 4 + idx * 8;
    let field_off = match corner {
        MathKernCorner::TopRight    => rec,
        MathKernCorner::TopLeft     => rec + 2,
        MathKernCorner::BottomRight => rec + 4,
        MathKernCorner::BottomLeft  => rec + 6,
    };
    let Some(kern_rel) = read_u16_be(math, field_off) else { return 0 };
    if kern_rel == 0 { return 0; }
    let kern_off = kern_info_off + kern_rel as usize;

    let Some(height_count) = read_u16_be(math, kern_off) else { return 0 };
    let height_count = height_count as usize;
    if height_count == 0 {
        return read_math_value(math, kern_off + 2).unwrap_or(0);
    }

    let mut selected = height_count;
    for i in 0..height_count {
        let Some(threshold) = read_math_value(math, kern_off + 2 + i * 4) else { return 0 };
        if height < threshold {
            selected = i;
            break;
        }
    }
    let kern_values_off = kern_off + 2 + height_count * 4;
    read_math_value(math, kern_values_off + selected * 4).unwrap_or(0)
}
