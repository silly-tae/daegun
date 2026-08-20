use alloc::vec::Vec;
use super::super::decoder::{read_i16_be, read_offset24, read_u16_be, read_u32_be, records_fit, write_i16_be, write_u16_be, write_u32_be};
use super::super::instancer::strip_colr_var_store;
use super::super::format::ivs::{precompute_region_scalars, ItemVariationStore};
use super::super::format::round::ot_round;
use super::varfield::resolve_delta_raw;
use super::{parse_colr_v1_var_data, PaintBudget};

pub(crate) fn instance_colr_v1(colr: &[u8], location: &[f64]) -> Option<Vec<u8>> {
    if colr.len() < 34 { return None; }
    if read_u16_be(colr, 0)? != 1 { return None; }
    if read_u32_be(colr, 30)? == 0 { return None; }

    let var_data = parse_colr_v1_var_data(colr);
    let Some(store) = var_data.var_store.as_ref() else {
        return strip_colr_var_store(colr);
    };
    let region_scalars = precompute_region_scalars(store, location);
    let ctx = PatchCtx {
        var_store: Some(store),
        var_index_map: var_data.var_index_map.as_deref(),
        region_scalars: &region_scalars,
        patched: core::cell::RefCell::new(alloc::vec![0u64; colr.len().div_ceil(64)]),
    };

    let base_glyph_list_off = match read_u32_be(colr, 14) {
        Some(v) if v != 0 => v as usize,
        _ => return strip_colr_var_store(colr),
    };
    let layer_list_raw = read_u32_be(colr, 18).unwrap_or(0);
    let layer_list_off = if layer_list_raw == 0 { None } else { Some(layer_list_raw as usize) };

    let mut out = colr.to_vec();
    if patch_all_base_glyphs(&mut out, base_glyph_list_off, layer_list_off, &ctx).is_none() {
        return strip_colr_var_store(colr);
    }

    write_u32_be(&mut out, 30, 0);
    Some(out)
}

struct PatchCtx<'a> {
    var_store: Option<&'a ItemVariationStore>,
    var_index_map: Option<&'a [(u32, u32)]>,
    region_scalars: &'a [f64],
    patched: core::cell::RefCell<alloc::vec::Vec<u64>>,
}

impl PatchCtx<'_> {
    fn mark(&self, off: usize) -> bool {
        let mut bits = self.patched.borrow_mut();
        match bits.get_mut(off >> 6) {
            Some(word) => {
                let bit = 1u64 << (off & 63);
                let fresh = *word & bit == 0;
                *word |= bit;
                fresh
            }
            None => true,
        }
    }
}

const MAX_BASE_GLYPHS: usize = 65536;

fn patch_all_base_glyphs(out: &mut [u8], base_glyph_list_off: usize, layer_list_off: Option<usize>, ctx: &PatchCtx) -> Option<()> {
    let budget = &mut PaintBudget::new();
    let num_records = (read_u32_be(out, base_glyph_list_off)? as usize).min(MAX_BASE_GLYPHS);
    for i in 0..num_records {
        let rec = base_glyph_list_off + 4 + i * 6;
        let paint_rel = read_u32_be(out, rec + 2)? as usize;
        let paint_off = base_glyph_list_off + paint_rel;
        patch_paint(out, paint_off, layer_list_off, ctx, budget)?;
    }
    Some(())
}

fn patch_child(out: &mut [u8], off: usize, layer_list_off: Option<usize>, ctx: &PatchCtx, budget: &mut PaintBudget) -> Option<()> {
    if !budget.enter() { return None; }
    let result = patch_paint(out, off, layer_list_off, ctx, budget);
    budget.leave();
    result
}

fn patch_paint(out: &mut [u8], off: usize, layer_list_off: Option<usize>, ctx: &PatchCtx, budget: &mut PaintBudget) -> Option<()> {
    let format = *out.get(off)?;

    match format {
        1 => {
            let num_layers = *out.get(off + 1)? as usize;
            let first_layer_index = read_u32_be(out, off + 2)? as usize;
            let layer_list_off = layer_list_off?;
            let num_layers_total = read_u32_be(out, layer_list_off)? as usize;
            for i in 0..num_layers {
                let idx = first_layer_index + i;
                if idx >= num_layers_total { return None; }
                let entry_off = layer_list_off + 4 + idx * 4;
                let rel = read_u32_be(out, entry_off)? as usize;
                patch_child(out, layer_list_off + rel, Some(layer_list_off), ctx, budget)?;
            }
            Some(())
        }
        2 => Some(()),
        3 => {
            let vib = read_u32_be(out, off + 5)?;
            patch_i16_field(out, off + 3, ctx, vib, 0)
        }
        4 => {
            let color_line_off = read_offset24(out, off + 1)?;
            patch_color_line(out, off + color_line_off, false, ctx, budget)
        }
        5 => {
            let color_line_off = read_offset24(out, off + 1)?;
            patch_color_line(out, off + color_line_off, true, ctx, budget)?;
            let coord_off = off + 4;
            let vib = read_u32_be(out, coord_off + 12)?;
            for i in 0..6u32 { patch_i16_field(out, coord_off + i as usize * 2, ctx, vib, i)?; }
            Some(())
        }
        6 => {
            let color_line_off = read_offset24(out, off + 1)?;
            patch_color_line(out, off + color_line_off, false, ctx, budget)
        }
        7 => {
            let color_line_off = read_offset24(out, off + 1)?;
            patch_color_line(out, off + color_line_off, true, ctx, budget)?;
            let c = off + 4;
            let vib = read_u32_be(out, c + 12)?;
            patch_i16_field(out, c, ctx, vib, 0)?;
            patch_i16_field(out, c + 2, ctx, vib, 1)?;
            patch_u16_field(out, c + 4, ctx, vib, 2)?;
            patch_i16_field(out, c + 6, ctx, vib, 3)?;
            patch_i16_field(out, c + 8, ctx, vib, 4)?;
            patch_u16_field(out, c + 10, ctx, vib, 5)
        }
        8 => {
            let color_line_off = read_offset24(out, off + 1)?;
            patch_color_line(out, off + color_line_off, false, ctx, budget)
        }
        9 => {
            let color_line_off = read_offset24(out, off + 1)?;
            patch_color_line(out, off + color_line_off, true, ctx, budget)?;
            let c = off + 4;
            let vib = read_u32_be(out, c + 8)?;
            for i in 0..4u32 { patch_i16_field(out, c + i as usize * 2, ctx, vib, i)?; }
            Some(())
        }
        10 => {
            let child_off = read_offset24(out, off + 1)?;
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        11 => Some(()),
        12 => {
            let child_off = read_offset24(out, off + 1)?;
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        13 => {
            let child_off = read_offset24(out, off + 1)?;
            let transform_off = read_offset24(out, off + 4)?;
            let t = off + transform_off;
            let vib = read_u32_be(out, t + 24)?;
            for i in 0..6u32 { patch_fixed32_field(out, t + i as usize * 4, ctx, vib, i)?; }
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        14 => {
            let child_off = read_offset24(out, off + 1)?;
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        15 => {
            let child_off = read_offset24(out, off + 1)?;
            let vib = read_u32_be(out, off + 8)?;
            patch_i16_field(out, off + 4, ctx, vib, 0)?;
            patch_i16_field(out, off + 6, ctx, vib, 1)?;
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        16..=23 => {
            let child_off = read_offset24(out, off + 1)?;
            let uniform = matches!(format, 20..=23);
            let around_center = matches!(format, 18 | 19 | 22 | 23);
            let is_var = format % 2 == 1;
            let after_scale_off = if uniform { off + 6 } else { off + 8 };
            if is_var {
                let (center_raw_off, vib_off) = if around_center { (after_scale_off, after_scale_off + 4) } else { (after_scale_off, after_scale_off) };
                let vib = read_u32_be(out, vib_off)?;
                patch_i16_field(out, off + 4, ctx, vib, 0)?;
                if !uniform { patch_i16_field(out, off + 6, ctx, vib, 1)?; }
                if around_center {
                    let base_pos = if uniform { 1 } else { 2 };
                    patch_i16_field(out, center_raw_off, ctx, vib, base_pos)?;
                    patch_i16_field(out, center_raw_off + 2, ctx, vib, base_pos + 1)?;
                }
            }
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        24..=27 => {
            let child_off = read_offset24(out, off + 1)?;
            let around_center = matches!(format, 26 | 27);
            let is_var = format % 2 == 1;
            if is_var {
                let center_off = off + 6;
                let vib_off = if around_center { center_off + 4 } else { center_off };
                let vib = read_u32_be(out, vib_off)?;
                patch_i16_field(out, off + 4, ctx, vib, 0)?;
                if around_center {
                    patch_i16_field(out, center_off, ctx, vib, 1)?;
                    patch_i16_field(out, center_off + 2, ctx, vib, 2)?;
                }
            }
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        28..=31 => {
            let child_off = read_offset24(out, off + 1)?;
            let around_center = matches!(format, 30 | 31);
            let is_var = format % 2 == 1;
            if is_var {
                let center_off = off + 8;
                let vib_off = if around_center { center_off + 4 } else { center_off };
                let vib = read_u32_be(out, vib_off)?;
                patch_i16_field(out, off + 4, ctx, vib, 0)?;
                patch_i16_field(out, off + 6, ctx, vib, 1)?;
                if around_center {
                    patch_i16_field(out, center_off, ctx, vib, 2)?;
                    patch_i16_field(out, center_off + 2, ctx, vib, 3)?;
                }
            }
            patch_child(out, off + child_off, layer_list_off, ctx, budget)
        }
        32 => {
            let source_off = read_offset24(out, off + 1)?;
            let backdrop_off = read_offset24(out, off + 5)?;
            patch_child(out, off + source_off, layer_list_off, ctx, budget)?;
            patch_child(out, off + backdrop_off, layer_list_off, ctx, budget)
        }
        _ => None,
    }
}

fn patch_color_line(
    out: &mut [u8], off: usize, is_var: bool, ctx: &PatchCtx, budget: &mut PaintBudget,
) -> Option<()> {
    if !is_var { return Some(()); }
    let num_stops = read_u16_be(out, off + 1)? as usize;
    let stop_size = 10usize;
    if !records_fit(off + 3, num_stops, stop_size, out.len()) { return None; }
    if !budget.spend_stops(num_stops) { return None; }
    for i in 0..num_stops {
        let s = off + 3 + i * stop_size;
        let vib = read_u32_be(out, s + 6)?;
        patch_i16_field(out, s, ctx, vib, 0)?;
        patch_i16_field(out, s + 4, ctx, vib, 1)?;
    }
    Some(())
}

fn patch_i16_field(out: &mut [u8], off: usize, ctx: &PatchCtx, var_index_base: u32, field_position: u32) -> Option<()> {
    if !ctx.mark(off) {
        return Some(());
    }
    let raw = read_i16_be(out, off)?;
    let delta = resolve_delta_raw(ctx.var_store, ctx.var_index_map, ctx.region_scalars, var_index_base, field_position);
    let new_val = ot_round(raw as f64 + delta).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    write_i16_be(out, off, new_val);
    Some(())
}

fn patch_u16_field(out: &mut [u8], off: usize, ctx: &PatchCtx, var_index_base: u32, field_position: u32) -> Option<()> {
    if !ctx.mark(off) {
        return Some(());
    }
    let raw = read_u16_be(out, off)?;
    let delta = resolve_delta_raw(ctx.var_store, ctx.var_index_map, ctx.region_scalars, var_index_base, field_position);
    let new_val = ot_round(raw as f64 + delta).clamp(0, u16::MAX as i32) as u16;
    write_u16_be(out, off, new_val);
    Some(())
}

fn patch_fixed32_field(out: &mut [u8], off: usize, ctx: &PatchCtx, var_index_base: u32, field_position: u32) -> Option<()> {
    if !ctx.mark(off) {
        return Some(());
    }
    let raw = read_u32_be(out, off)? as i32;
    let delta = resolve_delta_raw(ctx.var_store, ctx.var_index_map, ctx.region_scalars, var_index_base, field_position);
    let new_val = ot_round(raw as f64 + delta);
    write_u32_be(out, off, new_val as u32);
    Some(())
}
