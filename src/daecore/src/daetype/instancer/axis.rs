use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be};
use crate::daecore::daetype::TableBytes;

pub fn normalize_axis(value: f64, min: f64, def: f64, max: f64) -> f64 {
    let value = value.clamp(min.min(max), max.max(min));
    if value < def && def > min      { ((value - def) / (def - min)).max(-1.0) }
    else if value > def && max > def { ((value - def) / (max - def)).min(1.0) }
    else                             { 0.0 }
}

pub fn apply_avar_all(table_map: &BTreeMap<String, TableBytes>, location: &mut [f64]) {
    let avar = match table_map.get("avar") { Some(v) => v, None => return };
    let major = match read_u16_be(avar, 0) { Some(v @ (1 | 2)) => v, _ => return };
    let axis_count = match read_u16_be(avar, 6) { Some(v) => v as usize, None => return };
    let mut pos = 8usize;

    for i in 0..axis_count {
        let count = match read_u16_be(avar, pos) { Some(v) => v as usize, None => return };
        pos += 2;
        if i < location.len() {
            location[i] = segment_map(avar, pos, count, location[i]);
        }
        pos += count * 4;
    }

    if major != 2 { return; }
    let idx_map_off  = match read_u32_be(avar, pos)     { Some(v) => v as usize, None => return };
    let var_store_off = match read_u32_be(avar, pos + 4) { Some(v) => v as usize, None => return };
    if var_store_off == 0 { return; }
    let store = match super::super::format::ivs::parse_item_variation_store(avar, var_store_off) { Ok(s) => s, Err(_) => return };
    let map: Option<Vec<(u32, u32)>> = if idx_map_off != 0 {
        match super::super::format::ivs::parse_delta_set_index_map(avar, idx_map_off) {
            Ok(m)  => Some(m),
            Err(_) => return,
        }
    } else {
        None
    };
    let snapshot: Vec<f64> = location.to_vec();
    let region_scalars = super::super::format::ivs::precompute_region_scalars(&store, &snapshot);
    for (i, loc) in location.iter_mut().enumerate() {
        let (outer, inner) = match &map {
            Some(m) => super::super::format::ivs::delta_set_index_map_lookup(m, i),
            None => (0, i),
        };
        let delta = super::super::format::ivs::compute_ivs_delta_f64(&store, outer, inner, &region_scalars);
        *loc = (*loc + delta / 16384.0).clamp(-1.0, 1.0);
    }
}

fn segment_map(avar: &[u8], pos: usize, count: usize, norm_value: f64) -> f64 {
    for j in 0..count.saturating_sub(1) {
        let from0 = match read_i16_be(avar, pos + j * 4)           { Some(v) => v as f64 / 16384.0, None => return norm_value };
        let to0   = match read_i16_be(avar, pos + j * 4 + 2)       { Some(v) => v as f64 / 16384.0, None => return norm_value };
        let from1 = match read_i16_be(avar, pos + (j + 1) * 4)     { Some(v) => v as f64 / 16384.0, None => return norm_value };
        let to1   = match read_i16_be(avar, pos + (j + 1) * 4 + 2) { Some(v) => v as f64 / 16384.0, None => return norm_value };
        if norm_value >= from0 && norm_value <= from1 {
            return if from1 == from0 {
                to0
            } else {
                to0 + (norm_value - from0) * (to1 - to0) / (from1 - from0)
            };
        }
    }
    norm_value
}
