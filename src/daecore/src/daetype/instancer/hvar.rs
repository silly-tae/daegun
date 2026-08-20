use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::super::decoder::{read_u16_be, read_u32_be, write_u16_be};
use super::super::format::ivs::{parse_item_variation_store, parse_delta_set_index_map, delta_set_index_map_lookup, compute_ivs_delta_f64, precompute_region_scalars};
use super::super::format::round::ot_round;
use crate::daecore::daetype::TableBytes;

pub fn expand_metrics(mtx_data: &[u8], num_glyphs: usize, long_metrics: usize) -> Vec<u8> {
    if long_metrics >= num_glyphs || long_metrics == 0 {
        return mtx_data.to_vec();
    }
    let last_advance = read_u16_be(mtx_data, (long_metrics - 1) * 4).unwrap_or(0);
    let mut out = Vec::with_capacity(num_glyphs * 4);
    for gid in 0..num_glyphs {
        let (advance, lsb) = if gid < long_metrics {
            (
                read_u16_be(mtx_data, gid * 4).unwrap_or(0),
                read_u16_be(mtx_data, gid * 4 + 2).unwrap_or(0),
            )
        } else {
            let at = long_metrics * 4 + (gid - long_metrics) * 2;
            (last_advance, read_u16_be(mtx_data, at).unwrap_or(0))
        };
        out.extend_from_slice(&advance.to_be_bytes());
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    out
}

pub fn apply_hvar(
    table_map:  &BTreeMap<String, TableBytes>,
    hmtx_data:  &mut [u8],
    num_glyphs: usize,
    long_metrics: usize,
    location:   &[f64],
) -> Result<(), String> {
    apply_metric_var(table_map, "HVAR", hmtx_data, num_glyphs, long_metrics, location)
}

pub fn apply_vvar(
    table_map:  &BTreeMap<String, TableBytes>,
    vmtx_data:  &mut [u8],
    num_glyphs: usize,
    long_metrics: usize,
    location:   &[f64],
) -> Result<(), String> {
    apply_metric_var(table_map, "VVAR", vmtx_data, num_glyphs, long_metrics, location)
}

fn apply_metric_var(
    table_map:  &BTreeMap<String, TableBytes>,
    var_tag:    &str,
    mtx_data:   &mut [u8],
    num_glyphs: usize,
    long_metrics: usize,
    location:   &[f64],
) -> Result<(), String> {
    let var     = table_map.get(var_tag).ok_or_else(|| format!("missing {}", var_tag))?;
    let ivs_off = read_u32_be(var, 4).ok_or_else(|| format!("{}: header truncated", var_tag))? as usize;
    let map_off = read_u32_be(var, 8).ok_or_else(|| format!("{}: header truncated", var_tag))? as usize;

    let store = parse_item_variation_store(var, ivs_off)?;
    let map: Option<Vec<(u32, u32)>> = if map_off != 0 {
        Some(parse_delta_set_index_map(var, map_off)?)
    } else {
        None
    };

    if long_metrics == 0 {
        return Err(format!("{}: long-metrics count is zero", var_tag));
    }

    let region_scalars = precompute_region_scalars(&store, location);

    for gid in 0..long_metrics.min(num_glyphs) {
        let (outer, inner) = match &map {
            Some(m) => delta_set_index_map_lookup(m, gid),
            None => (0, gid),
        };
        let delta  = ot_round(compute_ivs_delta_f64(&store, outer, inner, &region_scalars));
        let aw_off = gid * 4;
        if aw_off + 2 > mtx_data.len() { continue; }
        let aw = read_u16_be(mtx_data, aw_off).unwrap_or(0) as i32;
        write_u16_be(mtx_data, aw_off, aw.saturating_add(delta).clamp(0, 65535) as u16);
    }
    Ok(())
}
