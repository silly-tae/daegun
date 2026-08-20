use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::super::decoder::{read_u16_be, read_u32_be, write_u16_be, write_u32_be};

fn read_fixed(data: &[u8], off: usize) -> Option<f64> {
    read_u32_be(data, off).map(|v| v as i32 as f64 / 65536.0)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0 / 65536.0
}

fn axis_value_len(stat: &[u8], at: usize) -> Option<usize> {
    Some(match read_u16_be(stat, at)? {
        1 => 12,
        2 => 20,
        3 => 16,
        4 => 8 + read_u16_be(stat, at + 2)? as usize * 6,
        _ => return None,
    })
}

fn describes(
    stat: &[u8], at: usize, location: &BTreeMap<u16, f64>, combo_left: &mut usize,
) -> Option<bool> {
    let matches_axis = |axis_index: u16, value: f64| match location.get(&axis_index) {
        Some(&coord) => close(coord, value),
        None => true,
    };
    Some(match read_u16_be(stat, at)? {
        1 => matches_axis(read_u16_be(stat, at + 2)?, read_fixed(stat, at + 8)?),
        2 => {
            let axis_index = read_u16_be(stat, at + 2)?;
            let (min, max) = (read_fixed(stat, at + 12)?, read_fixed(stat, at + 16)?);
            match location.get(&axis_index) {
                Some(&coord) => coord >= min - 1.0 / 65536.0 && coord <= max + 1.0 / 65536.0,
                None => true,
            }
        }
        3 => matches_axis(read_u16_be(stat, at + 2)?, read_fixed(stat, at + 8)?),
        4 => {
            let count = read_u16_be(stat, at + 2)? as usize;
            *combo_left = combo_left.checked_sub(count)?;
            let mut all = count > 0;
            for j in 0..count {
                let rec = at + 8 + j * 6;
                if !matches_axis(read_u16_be(stat, rec)?, read_fixed(stat, rec + 2)?) {
                    all = false;
                    break;
                }
            }
            all
        }
        _ => false,
    })
}

pub(crate) fn filter_stat_to_instance(stat: &[u8], axis_coords: &BTreeMap<String, f64>) -> Option<Vec<u8>> {
    if stat.len() < 18 {
        return None;
    }
    let major = read_u16_be(stat, 0)?;
    let minor = read_u16_be(stat, 2)?;
    let design_axis_size = read_u16_be(stat, 4)? as usize;
    let design_axis_count = read_u16_be(stat, 6)? as usize;
    let design_axes_offset = read_u32_be(stat, 8)? as usize;
    let axis_value_count = read_u16_be(stat, 12)? as usize;
    let offsets_array = read_u32_be(stat, 14)? as usize;

    if design_axis_size < 8 {
        return None;
    }
    let design_axes = stat.get(design_axes_offset..)?.get(..design_axis_count.checked_mul(design_axis_size)?)?;

    let mut location: BTreeMap<u16, f64> = BTreeMap::new();
    for i in 0..design_axis_count {
        let rec = i * design_axis_size;
        let tag = String::from_utf8_lossy(design_axes.get(rec..rec + 4)?).to_string();
        if let Some(&coord) = axis_coords.get(&tag) {
            location.insert(u16::try_from(i).ok()?, coord);
        }
    }

    let mut kept: Vec<&[u8]> = Vec::new();
    let mut combo_left = stat.len() / 6;
    for i in 0..axis_value_count {
        let rel = read_u16_be(stat, offsets_array + i * 2)? as usize;
        let at = offsets_array.checked_add(rel)?;
        let Some(len) = axis_value_len(stat, at) else { continue };
        let Some(bytes) = stat.get(at..).and_then(|s| s.get(..len)) else { continue };
        if describes(stat, at, &location, &mut combo_left).unwrap_or(false) {
            kept.push(bytes);
        }
    }

    let elided_fallback = if minor >= 1 && stat.len() >= 20 { read_u16_be(stat, 18)? } else { 2 };

    let header_len = 20usize;
    let axes_at = header_len;
    let offsets_at = axes_at + design_axes.len();
    let values_at = offsets_at + kept.len() * 2;

    let mut out = vec![0u8; values_at];
    write_u16_be(&mut out, 0, major);
    write_u16_be(&mut out, 2, minor.max(1));
    write_u16_be(&mut out, 4, u16::try_from(design_axis_size).ok()?);
    write_u16_be(&mut out, 6, u16::try_from(design_axis_count).ok()?);
    write_u32_be(&mut out, 8, u32::try_from(axes_at).ok()?);
    write_u16_be(&mut out, 12, u16::try_from(kept.len()).ok()?);
    write_u32_be(&mut out, 14, u32::try_from(offsets_at).ok()?);
    write_u16_be(&mut out, 18, elided_fallback);
    out[axes_at..offsets_at].copy_from_slice(design_axes);

    for (i, bytes) in kept.iter().enumerate() {
        let target = out.len() - offsets_at;
        write_u16_be(&mut out, offsets_at + i * 2, u16::try_from(target).ok()?);
        out.extend_from_slice(bytes);
    }

    Some(out)
}
