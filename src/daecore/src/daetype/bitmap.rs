use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::decoder::{read_u16_be, read_u32_be, read_i16_be, records_fit};
use crate::daecore::daetype::TableBytes;

#[derive(Debug, PartialEq)]
pub struct GlyphBitmap {
    pub png:      Vec<u8>,
    pub ppem:     u16,
    pub origin_x: i16,
    pub origin_y: i16,
}

pub fn glyph_bitmap(table_map: &BTreeMap<String, TableBytes>, gid: u16, target_ppem: u16) -> Option<GlyphBitmap> {
    if let Some(b) = sbix_bitmap(table_map, gid, target_ppem) { return Some(b); }
    cbdt_bitmap(table_map, gid, target_ppem)
}

fn num_glyphs(table_map: &BTreeMap<String, TableBytes>) -> usize {
    table_map.get("maxp").and_then(|m| read_u16_be(m, 4)).unwrap_or(0) as usize
}

fn pick_strike<T: Copy>(strikes: &[(u16, T)], target_ppem: u16) -> Option<T> {
    let mut best_above: Option<(u16, T)> = None;
    let mut largest:    Option<(u16, T)> = None;
    for &(ppem, v) in strikes {
        if ppem >= target_ppem && best_above.is_none_or(|(p, _)| ppem < p) {
            best_above = Some((ppem, v));
        }
        if largest.is_none_or(|(p, _)| ppem > p) {
            largest = Some((ppem, v));
        }
    }
    best_above.or(largest).map(|(_, v)| v)
}

fn sbix_bitmap(table_map: &BTreeMap<String, TableBytes>, gid: u16, target_ppem: u16) -> Option<GlyphBitmap> {
    let sbix = table_map.get("sbix")?;
    let n_glyphs = num_glyphs(table_map);
    if gid as usize >= n_glyphs { return None; }

    let num_strikes = read_u32_be(sbix, 4)? as usize;
    if !records_fit(8, num_strikes, 4, sbix.len()) { return None; }
    let mut strikes: Vec<(u16, usize)> = Vec::with_capacity(num_strikes);
    for i in 0..num_strikes {
        let off  = read_u32_be(sbix, 8 + i * 4)? as usize;
        let ppem = read_u16_be(sbix, off)?;
        strikes.push((ppem, off));
    }
    let strike = pick_strike(&strikes, target_ppem)?;

    let ppem   = read_u16_be(sbix, strike)?;

    let g_off  = read_u32_be(sbix, strike + 4 + gid as usize * 4)? as usize;
    let g_next = read_u32_be(sbix, strike + 4 + (gid as usize + 1) * 4)? as usize;
    if g_next <= g_off { return None; }
    let data = sbix.get(strike + g_off..strike + g_next)?;
    if data.len() < 8 { return None; }
    let origin_x = read_i16_be(data, 0)?;
    let origin_y = read_i16_be(data, 2)?;
    let gtype    = &data[4..8];
    if gtype != b"png " { return None; }
    Some(GlyphBitmap { png: data[8..].to_vec(), ppem, origin_x, origin_y })
}

// `EBDT`/`EBLC` is byte-identical to `CBDT`/`CBLC` and monochrome, so one reader serves both – which
// is why this file is not called color.rs.
fn cbdt_bitmap(table_map: &BTreeMap<String, TableBytes>, gid: u16, target_ppem: u16) -> Option<GlyphBitmap> {
    let cblc = table_map.get("CBLC")?;
    let cbdt = table_map.get("CBDT")?;

    let num_sizes = read_u32_be(cblc, 4)? as usize;
    if !records_fit(8, num_sizes, 48, cblc.len()) { return None; }
    let mut strikes: Vec<(u16, usize)> = Vec::with_capacity(num_sizes);
    for i in 0..num_sizes {
        let st = 8 + i * 48;
        let ppem_x = *cblc.get(st + 44)? as u16;
        strikes.push((ppem_x, st));
    }
    let st = pick_strike(&strikes, target_ppem)?;

    let ppem = *cblc.get(st + 44)? as u16;

    let ist_array_off = read_u32_be(cblc, st)? as usize;
    let n_ist         = read_u32_be(cblc, st + 8)? as usize;

    if !records_fit(ist_array_off, n_ist, 8, cblc.len()) { return None; }
    for i in 0..n_ist {
        let rec = ist_array_off + i * 8;
        let first = read_u16_be(cblc, rec)?;
        let last  = read_u16_be(cblc, rec + 2)?;
        if gid < first || gid > last { continue; }
        let ist = ist_array_off + read_u32_be(cblc, rec + 4)? as usize;

        let index_format = read_u16_be(cblc, ist)?;
        let image_format = read_u16_be(cblc, ist + 2)?;
        let image_data_off = read_u32_be(cblc, ist + 4)? as usize;
        if !matches!(image_format, 17..=19) { return None; }

        let idx = (gid - first) as usize;
        let (g_off, g_next) = match index_format {
            1 => (
                read_u32_be(cblc, ist + 8 + idx * 4)? as usize,
                read_u32_be(cblc, ist + 8 + (idx + 1) * 4)? as usize,
            ),
            2 => {
                let size = read_u32_be(cblc, ist + 8)? as usize;
                (size * idx, size * (idx + 1))
            }
            3 => (
                read_u16_be(cblc, ist + 8 + idx * 2)? as usize,
                read_u16_be(cblc, ist + 8 + (idx + 1) * 2)? as usize,
            ),
            _ => return None,
        };
        if g_next <= g_off { return None; }
        let data = cbdt.get(image_data_off + g_off..image_data_off + g_next)?;

        let (metrics_len, ox, oy) = match image_format {
            17 => (5usize, *data.get(2)? as i8 as i16, *data.get(3)? as i8 as i16),
            18 => (8usize, *data.get(2)? as i8 as i16, *data.get(3)? as i8 as i16),
            _  => (0usize, 0, 0),
        };
        let png_len = read_u32_be(data, metrics_len)? as usize;
        let png = data.get(metrics_len + 4..metrics_len + 4 + png_len)?;
        return Some(GlyphBitmap { png: png.to_vec(), ppem, origin_x: ox, origin_y: oy });
    }
    None
}
