use alloc::string::String;
use crate::cache::FontCache;

fn char_width_pt(ch: char, fc: &FontCache, axis_values: &[(String, f64)], font_size: f64) -> f64 {
    let gid = fc.glyph_id(ch as u32).unwrap_or(0);
    fc.advance_width_rs(axis_values, gid) as f64 * font_size / 1000.0
}

pub(crate) fn string_width_pt(text: &str, fc: &FontCache, axis_values: &[(String, f64)], font_size: f64) -> f64 {
    if let Some(run) = fc.shaped_run(axis_values, text, false) {
        return run.advances.iter().sum::<f64>() * font_size / 1000.0;
    }
    text.chars().map(|c| char_width_pt(c, fc, axis_values, font_size)).sum()
}
