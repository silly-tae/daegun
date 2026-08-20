use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
mod cff2;
mod gpos;
mod style;
mod feature_variations;
mod name_table;
mod stat_filter;
mod axis;
mod loca;
mod coords;
mod gvar;
mod hvar;
mod mvar;
mod cvar;
mod strip_var_stores;

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use super::decoder::{build_ttf, parse_fvar_axes, read_u16_be, read_i16_be, write_u16_be, write_i16_be};
use super::format::round::ot_round;
use axis::{normalize_axis, apply_avar_all};
use loca::build_loca_table;
use gvar::apply_gvar;
use cvar::apply_cvar;
use crate::daecore::daetype::TableBytes;

pub(crate) use hvar::{apply_hvar, apply_vvar};
pub(crate) use mvar::apply_mvar;
pub use mvar::mvar_deltas;
pub(crate) use strip_var_stores::{strip_gdef_var_store, strip_colr_var_store};
pub(crate) use gpos::apply_gpos_var;
pub(crate) use style::apply_style_metadata;
pub(crate) use feature_variations::resolve_feature_variations;
pub(crate) use hvar::expand_metrics;
pub use loca::parse_loca;
pub(crate) use coords::{extract_coords, extract_coords_into};
pub use coords::GlyphCoords;

pub fn compute_location(
    table_map:   &BTreeMap<String, TableBytes>,
    axis_values: &[(String, f64)],
) -> Result<Vec<f64>, String> {
    let axes = parse_fvar_axes(table_map)?;
    let mut location = vec![0.0f64; axes.len()];
    for (i, axis) in axes.iter().enumerate() {
        if let Some(pair) = axis_values.iter().find(|p| p.0 == axis.tag) {
            location[i] = normalize_axis(pair.1, axis.min, axis.default, axis.max);
        }
    }

    apply_avar_all(table_map, &mut location);

    for loc in location.iter_mut() {
        *loc = super::format::round::quantize_f2dot14(*loc);
    }

    Ok(location)
}

pub fn instance_font_from_map(
    table_map:   &BTreeMap<String, TableBytes>,
    axis_values: &[(String, f64)],
) -> Result<Vec<u8>, String> {
    Ok(build_ttf(&instance_tables_from_map(table_map, axis_values)?))
}

pub fn instance_tables_from_map<'a>(
    table_map:   &'a BTreeMap<String, TableBytes>,
    axis_values: &[(String, f64)],
) -> Result<BTreeMap<String, alloc::borrow::Cow<'a, [u8]>>, String> {
    if table_map.contains_key("CFF2") {
        let instanced_map = cff2::instance_cff2_from_map(table_map, axis_values)?;
        return Ok(instanced_map);
    }

    let location   = compute_location(table_map, axis_values)?;
    let axis_count = location.len();

    let head        = table_map.get("head").ok_or("missing head")?;
    let loca_format = read_i16_be(head, 50).ok_or("head: truncated")?;

    let maxp       = table_map.get("maxp").ok_or("missing maxp")?;
    let num_glyphs = read_u16_be(maxp, 4).ok_or("maxp: truncated")? as usize;

    let glyph_offsets = parse_loca(table_map, loca_format, num_glyphs)?;

    let hmtx_src = table_map.get("hmtx").ok_or("missing hmtx")?;
    let hmtx_long_metrics = table_map
        .get("hhea")
        .and_then(|hhea| read_u16_be(hhea, 34))
        .map_or(0usize, usize::from);
    let mut os2_data  = table_map.get("OS/2").ok_or("missing OS/2")?.to_owned_vec();

    let needs_var = location.iter().any(|&v| v != 0.0);

    let mut out_loca_format = loca_format;
    let mut phantom_advance_deltas: Option<Vec<f64>> = None;
    let mut phantom_lsb_new: Option<Vec<Option<i32>>> = None;
    let mut phantom_vadvance_deltas: Option<Vec<f64>> = None;
    let mut phantom_tsb_new: Option<Vec<Option<i32>>> = None;
    let (glyf_data, new_loca_opt): (alloc::borrow::Cow<[u8]>, Option<Vec<u8>>) = if needs_var && table_map.contains_key("gvar") {
        let glyf_src = table_map.get("glyf").ok_or("missing glyf")?.as_slice();
        let result     = apply_gvar(table_map, glyf_src, &glyph_offsets, num_glyphs, &location, axis_count)?;
        if out_loca_format == 0 && result.new_loca.last().copied().unwrap_or(0) > 0xFFFF * 2 {
            out_loca_format = 1;
        }
        let new_loca = build_loca_table(&result.new_loca, out_loca_format);
        phantom_advance_deltas = Some(result.advance_deltas);
        phantom_lsb_new = Some(result.lsb_new);
        phantom_vadvance_deltas = Some(result.vadvance_deltas);
        phantom_tsb_new = Some(result.tsb_new);
        (alloc::borrow::Cow::Owned(result.glyf_data), Some(new_loca))
    } else {
        (alloc::borrow::Cow::Borrowed(table_map.get("glyf").ok_or("missing glyf")?.as_slice()), None)
    };

    let expands = needs_var && hmtx_long_metrics < num_glyphs && hmtx_long_metrics != 0;
    let (mut hmtx_data, hmtx_metrics) = if expands {
        (hvar::expand_metrics(hmtx_src, num_glyphs, hmtx_long_metrics), num_glyphs)
    } else {
        (hmtx_src.to_owned_vec(), hmtx_long_metrics)
    };

    if needs_var && table_map.contains_key("HVAR") {
        apply_hvar(table_map, &mut hmtx_data, num_glyphs, hmtx_metrics, &location)?;
    } else if needs_var
        && let Some(deltas) = &phantom_advance_deltas
    {
        for (gid, &delta) in deltas.iter().enumerate().take(hmtx_metrics.min(num_glyphs)) {
            let d = ot_round(delta);
            if d == 0 { continue; }
            let off = gid * 4;
            if off + 2 > hmtx_data.len() { break; }
            let aw = read_u16_be(&hmtx_data, off).unwrap_or(0) as i32;
            write_u16_be(&mut hmtx_data, off, aw.saturating_add(d).clamp(0, 65535) as u16);
        }
    }

    if let Some(lsb_values) = &phantom_lsb_new {
        for (gid, lsb) in lsb_values.iter().enumerate() {
            let Some(lsb) = *lsb else { continue };
            let off = if gid < hmtx_metrics {
                gid * 4 + 2
            } else {
                hmtx_metrics * 4 + (gid - hmtx_metrics) * 2
            };
            if off + 2 > hmtx_data.len() { continue; }
            write_i16_be(&mut hmtx_data, off, lsb.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
        }
    }

    let mut vmtx_data = table_map.get("vmtx").map(TableBytes::to_owned_vec);
    let vmtx_metrics = table_map
        .get("vhea")
        .and_then(|vhea| read_u16_be(vhea, 34))
        .map_or(0usize, usize::from);
    if needs_var && vmtx_metrics < num_glyphs
        && let Some(ref mut vmtx) = vmtx_data {
            *vmtx = hvar::expand_metrics(vmtx, num_glyphs, vmtx_metrics);
        }
    let vmtx_metrics = if needs_var && vmtx_metrics < num_glyphs { num_glyphs } else { vmtx_metrics };
    if needs_var && let Some(ref mut vmtx) = vmtx_data {
        if table_map.contains_key("VVAR") {
            apply_vvar(table_map, vmtx, num_glyphs, vmtx_metrics, &location)?;
        } else if let Some(deltas) = &phantom_vadvance_deltas {
            for (gid, &delta) in deltas.iter().enumerate().take(vmtx_metrics.min(num_glyphs)) {
                let d = ot_round(delta);
                if d == 0 { continue; }
                let off = gid * 4;
                if off + 2 > vmtx.len() { break; }
                let ah = read_u16_be(vmtx, off).unwrap_or(0) as i32;
                write_u16_be(vmtx, off, ah.saturating_add(d).clamp(0, 65535) as u16);
            }
        }
    }

    if let (Some(tsb_values), Some(vmtx)) = (&phantom_tsb_new, &mut vmtx_data) {
        for (gid, tsb) in tsb_values.iter().enumerate() {
            let Some(tsb) = *tsb else { continue };
            let off = if gid < vmtx_metrics {
                gid * 4 + 2
            } else {
                vmtx_metrics * 4 + (gid - vmtx_metrics) * 2
            };
            if off + 2 > vmtx.len() { continue; }
            write_i16_be(vmtx, off, tsb.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
        }
    }

    let mut hhea_data = table_map.get("hhea").map(TableBytes::to_owned_vec).unwrap_or_default();
    let mut post_data = table_map.get("post").map(TableBytes::to_owned_vec).unwrap_or_default();
    let mut cvt_data  = table_map.get("cvt ").map(TableBytes::to_owned_vec);
    if needs_var {
        apply_mvar(table_map, &mut hhea_data, &mut os2_data, &mut post_data, &location)?;
        if let Some(ref mut cvt) = cvt_data {
            apply_cvar(table_map, cvt, &location, axis_count)?;
        }
    }

    let style = style::apply_style_metadata(table_map, axis_values, &mut os2_data);

    const STRIP_AFTER_GLYF_INSTANCING: &[&str] = &["fvar", "gvar", "HVAR", "MVAR", "avar", "cvar", "VVAR"];
    let mut out_map: BTreeMap<String, Cow<[u8]>> = BTreeMap::new();
    for (tag, data) in table_map {
        if !STRIP_AFTER_GLYF_INSTANCING.contains(&tag.as_str()) {
            out_map.insert(tag.clone(), Cow::Borrowed(data.as_slice()));
        }
    }
    if needs_var
        && let Some(patched) = apply_gpos_var(table_map, &location) {
            out_map.insert("GPOS".to_string(), Cow::Owned(patched));
        }
    if let Some(gdef) = table_map.get("GDEF")
        && let Some(stripped) = strip_gdef_var_store(gdef) {
            out_map.insert("GDEF".to_string(), Cow::Owned(stripped));
        }
    if let Some(gsub) = table_map.get("GSUB")
        && let Some(resolved) = resolve_feature_variations(gsub, &location) {
            out_map.insert("GSUB".to_string(), Cow::Owned(resolved));
        }
    if let Some(colr) = table_map.get("COLR")
        && let Some(instanced) = super::colr_v1::instance_colr_v1(colr, &location) {
            out_map.insert("COLR".to_string(), Cow::Owned(instanced));
        }
    out_map.insert("glyf".to_string(), glyf_data);
    out_map.insert("hmtx".to_string(), Cow::Owned(hmtx_data));
    out_map.insert("OS/2".to_string(), Cow::Owned(os2_data));
    if let Some(head) = style.head { out_map.insert("head".to_string(), Cow::Owned(head)); }
    if let Some(name) = style.name { out_map.insert("name".to_string(), Cow::Owned(name)); }
    if let Some(stat) = style.stat { out_map.insert("STAT".to_string(), Cow::Owned(stat)); }
    if let Some(vmtx) = vmtx_data {
        out_map.insert("vmtx".to_string(), Cow::Owned(vmtx));
        if let Some(vhea) = table_map.get("vhea").filter(|v| v.len() >= 36) {
            let mut vhea = vhea.to_owned_vec();
            write_u16_be(&mut vhea, 34, vmtx_metrics.min(0xFFFF) as u16);
            out_map.insert("vhea".to_string(), Cow::Owned(vhea));
        }
    }
    if !hhea_data.is_empty() && hhea_data.len() >= 36 {
        write_u16_be(&mut hhea_data, 34, hmtx_metrics.min(0xFFFF) as u16);
    }
    if !hhea_data.is_empty() { out_map.insert("hhea".to_string(), Cow::Owned(hhea_data)); }
    if !post_data.is_empty() { out_map.insert("post".to_string(), Cow::Owned(post_data)); }
    if let Some(cvt) = cvt_data { out_map.insert("cvt ".to_string(), Cow::Owned(cvt)); }
    if let Some(new_loca) = new_loca_opt {
        out_map.insert("loca".to_string(), Cow::Owned(new_loca));
    }
    if out_loca_format != loca_format
        && let Some(head_out) = out_map.get_mut("head")
        && head_out.len() >= 52
    {
        write_i16_be(head_out.to_mut(), 50, out_loca_format);
    }

    Ok(out_map)
}
