#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use alloc::vec::Vec;
use super::super::decoder::{read_i16_be, read_u16_be, read_u32_be, records_fit};
use super::paint::ColorStop;
use super::varfield::{resolve_var_field, VarField};
use super::Colrv1Ctx;

pub(super) fn parse_color_line(
    ctx: &Colrv1Ctx, off: usize, is_var: bool, budget: &mut super::PaintBudget,
) -> Option<(u8, Vec<ColorStop>)> {
    let colr = ctx.colr;
    let extend = *colr.get(off)?;
    let num_stops = read_u16_be(colr, off + 1)? as usize;
    let stop_size = if is_var { 10 } else { 6 };

    if !records_fit(off + 3, num_stops, stop_size, colr.len()) { return None; }
    if !budget.spend_stops(num_stops) { return None; }
    let mut stops = Vec::with_capacity(num_stops);
    for i in 0..num_stops {
        let s = off + 3 + i * stop_size;
        let stop_offset_raw = read_i16_be(colr, s)?;
        let palette_index   = read_u16_be(colr, s + 2)?;
        let alpha_raw        = read_i16_be(colr, s + 4)?;

        let (stop_offset, alpha) = if is_var {
            let var_index_base = read_u32_be(colr, s + 6)?;
            (
                resolve_var_field(stop_offset_raw as i32, VarField::F2Dot14, var_index_base, 0, ctx),
                resolve_var_field(alpha_raw as i32, VarField::F2Dot14, var_index_base, 1, ctx),
            )
        } else {
            (stop_offset_raw as f64 / 16384.0, alpha_raw as f64 / 16384.0)
        };

        let stop_alpha = alpha.clamp(0.0, 1.0);
        let (is_foreground, r, g, b, alpha_byte) = if palette_index == 0xFFFF {
            (true, 0, 0, 0, (stop_alpha * 255.0).round() as u8)
        } else {
            let (cpal, palette) = (ctx.cpal?, ctx.palette.as_ref()?);
            let (r, g, b, cpal_a) = palette.entry(cpal, palette_index)?;
            let combined = stop_alpha * (cpal_a as f64 / 255.0);
            (false, r, g, b, (combined * 255.0).round() as u8)
        };

        stops.push(ColorStop { offset: stop_offset, is_foreground, r, g, b, alpha: alpha_byte });
    }

    Some((extend, stops))
}
