use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::decoder::{read_u16_be, read_u32_be, records_fit, search_records};
use crate::daecore::daetype::TableBytes;

pub type ColrLayer = (u16, u8, u8, u8, u8, bool);

fn colr_base_glyph_layers(colr: &[u8], gid: u16) -> Option<(usize, usize, usize)> {
    let n_base     = read_u16_be(colr, 2)? as usize;
    let base_off   = read_u32_be(colr, 4)? as usize;
    let layers_off = read_u32_be(colr, 8)? as usize;

    let hit = search_records(n_base, gid as u32, |i| read_u16_be(colr, base_off + i * 6).map(u32::from))?.ok()?;
    let rec = base_off + hit * 6;
    let first_layer = read_u16_be(colr, rec + 2)? as usize;
    let n_layers    = read_u16_be(colr, rec + 4)? as usize;
    Some((first_layer, n_layers, layers_off))
}

pub(crate) fn colr_v0_header(colr: &[u8]) -> Option<(usize, usize, usize, usize)> {
    let n_base     = read_u16_be(colr, 2)? as usize;
    let base_off   = read_u32_be(colr, 4)? as usize;
    let layers_off = read_u32_be(colr, 8)? as usize;
    let n_layers   = read_u16_be(colr, 12)? as usize;
    Some((n_base, base_off, layers_off, n_layers))
}

pub(crate) fn colr_v0_base_glyphs(colr: &[u8]) -> Vec<(u16, usize, usize)> {
    let Some((n_base, base_off, _, _)) = colr_v0_header(colr) else { return vec![] };
    if !records_fit(base_off, n_base, 6, colr.len()) { return vec![] }
    let mut out = Vec::with_capacity(n_base);
    for i in 0..n_base {
        let rec = base_off + i * 6;
        let (Some(gid), Some(first), Some(n)) = (
            read_u16_be(colr, rec), read_u16_be(colr, rec + 2), read_u16_be(colr, rec + 4),
        ) else { break };
        out.push((gid, first as usize, n as usize));
    }
    out
}

pub fn cpal_palette_count(table_map: &BTreeMap<String, TableBytes>) -> u16 {
    table_map.get("CPAL").and_then(|cpal| read_u16_be(cpal, 4)).unwrap_or(0)
}

#[derive(Debug, PartialEq)]
pub struct PaletteInfo {
    pub index:      u16,
    pub light_safe: bool,
    pub dark_safe:  bool,
    pub name_id:    Option<u16>,
}

pub fn cpal_palette_info(table_map: &BTreeMap<String, TableBytes>) -> Vec<PaletteInfo> {
    let cpal = match table_map.get("CPAL") { Some(c) => c, None => return vec![] };
    let version      = read_u16_be(cpal, 0).unwrap_or(0);
    let num_palettes = match read_u16_be(cpal, 4) { Some(v) => v, None => return vec![] };

    if version == 0 {
        return (0..num_palettes)
            .map(|i| PaletteInfo { index: i, light_safe: false, dark_safe: false, name_id: None })
            .collect();
    }

    let v1_off     = 12 + num_palettes as usize * 2;
    let types_off  = read_u32_be(cpal, v1_off).map(|v| v as usize);
    let labels_off = read_u32_be(cpal, v1_off + 4).map(|v| v as usize);

    (0..num_palettes).map(|i| {
        let (light_safe, dark_safe) = match types_off {
            Some(off) if off != 0 => {
                let flags = read_u32_be(cpal, off + i as usize * 4).unwrap_or(0);
                (flags & 0x0001 != 0, flags & 0x0002 != 0)
            }
            _ => (false, false),
        };
        let name_id = match labels_off {
            Some(off) if off != 0 => read_u16_be(cpal, off + i as usize * 2).filter(|&v| v != 0xFFFF),
            _ => None,
        };
        PaletteInfo { index: i, light_safe, dark_safe, name_id }
    }).collect()
}

#[derive(Clone, Copy)]
pub(crate) struct CpalPalette {
    records_off: usize,
    pal_start:   usize,
    n_entries:   usize,
}

impl CpalPalette {
    pub(crate) fn new(cpal: &[u8], palette_index: u16) -> Option<CpalPalette> {
        let n_entries    = read_u16_be(cpal, 2)? as usize;
        let num_palettes = read_u16_be(cpal, 4)? as usize;
        if palette_index as usize >= num_palettes { return None; }
        let records_off = read_u32_be(cpal, 8)? as usize;
        let pal_start   = read_u16_be(cpal, 12 + palette_index as usize * 2)? as usize;
        Some(CpalPalette { records_off, pal_start, n_entries })
    }

    pub(crate) fn entry(&self, cpal: &[u8], entry_index: u16) -> Option<(u8, u8, u8, u8)> {
        if entry_index as usize >= self.n_entries { return None; }
        let c = self.records_off + (self.pal_start + entry_index as usize) * 4;
        let b = *cpal.get(c)?;
        let g = *cpal.get(c + 1)?;
        let r = *cpal.get(c + 2)?;
        let a = *cpal.get(c + 3)?;
        Some((r, g, b, a))
    }
}

pub fn colr_layers_for_palette(
    table_map: &BTreeMap<String, TableBytes>, gid: u16, palette_index: u16,
) -> Option<Vec<ColrLayer>> {
    let colr = table_map.get("COLR")?;
    let cpal = table_map.get("CPAL")?;

    let (first_layer, n_layers, layers_off) = colr_base_glyph_layers(colr, gid)?;
    let num_palettes = read_u16_be(cpal, 4)?;
    if palette_index >= num_palettes { return None; }

    if !records_fit(layers_off + first_layer * 4, n_layers, 4, colr.len()) { return None; }
    let palette = CpalPalette::new(cpal, palette_index)?;
    let mut out = Vec::with_capacity(n_layers);
    for l in 0..n_layers {
        let rec = layers_off + (first_layer + l) * 4;
        let layer_gid = read_u16_be(colr, rec)?;
        let pal_idx   = read_u16_be(colr, rec + 2)?;
        // The one index CPAL cannot resolve: it means "whatever color the text is", which is the
        // caller's to supply – hence a flag on the layer rather than a color.
        if pal_idx == 0xFFFF {
            out.push((layer_gid, 0, 0, 0, 255, true));
            continue;
        }
        let (r, g, b, a) = palette.entry(cpal, pal_idx)?;
        out.push((layer_gid, r, g, b, a, false));
    }
    Some(out)
}

pub fn colr_layers(table_map: &BTreeMap<String, TableBytes>, gid: u16) -> Option<Vec<ColrLayer>> {
    colr_layers_for_palette(table_map, gid, 0)
}
