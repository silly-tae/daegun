use alloc::string::String;
use alloc::vec::Vec;
use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be, records_fit};

pub struct ItemVariationStore {
    pub regions:    Vec<Vec<RegionAxis>>,
    pub ivd_data:   Vec<Ivd>,
    pub axis_count: usize,
}

pub struct RegionAxis { pub start: f64, pub peak: f64, pub end: f64 }

#[derive(Default)]
pub struct Ivd {
    pub region_indices: Vec<usize>,
    deltas: Vec<i32>,
}

impl Ivd {
    pub fn row(&self, inner: usize) -> Option<&[i32]> {
        let width = self.region_indices.len();
        if width == 0 { return None; }
        let start = inner.checked_mul(width)?;
        self.deltas.get(start..start.checked_add(width)?)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn rows(&self) -> usize {
        self.deltas.len().checked_div(self.region_indices.len()).unwrap_or(0)
    }
}

pub fn parse_item_variation_store(buf: &[u8], base: usize) -> Result<ItemVariationStore, String> {
    let region_list_off = read_u32_be(buf, base + 2).ok_or("IVS: header truncated")? as usize;
    let ivd_count       = read_u16_be(buf, base + 6).ok_or("IVS: header truncated")? as usize;

    let region_list_base = base + region_list_off;
    let axis_count       = read_u16_be(buf, region_list_base).ok_or("IVS: region list truncated")? as usize;
    let region_count     = read_u16_be(buf, region_list_base + 2).ok_or("IVS: region list truncated")? as usize;

    let stride = axis_count.checked_mul(6).ok_or("IVS: region stride overflows")?;
    let regions_at = region_list_base.checked_add(4).ok_or("IVS: region list base overflows")?;
    if !records_fit(regions_at, region_count, stride, buf.len()) {
        return Err("IVS: region list does not fit the table".into());
    }

    let mut regions: Vec<Vec<RegionAxis>> = Vec::with_capacity(region_count);
    for i in 0..region_count {
        let r_off = regions_at + i * stride;
        let mut axes = Vec::with_capacity(axis_count);
        for j in 0..axis_count {
            let start = read_i16_be(buf, r_off + j * 6).ok_or("IVS: region axis truncated")?;
            let peak  = read_i16_be(buf, r_off + j * 6 + 2).ok_or("IVS: region axis truncated")?;
            let end   = read_i16_be(buf, r_off + j * 6 + 4).ok_or("IVS: region axis truncated")?;
            axes.push(RegionAxis {
                start: start as f64 / 16384.0,
                peak:  peak  as f64 / 16384.0,
                end:   end   as f64 / 16384.0,
            });
        }
        regions.push(axes);
    }

    if !records_fit(base + 8, ivd_count, 4, buf.len()) {
        return Err("IVS: IVD offset table does not fit".into());
    }
    let mut ivd_offsets: Vec<usize> = Vec::with_capacity(ivd_count);
    for i in 0..ivd_count {
        let off = read_u32_be(buf, base + 8 + i * 4).ok_or("IVS: IVD offset table truncated")?;
        ivd_offsets.push(off as usize);
    }

    // Two runaways, one budget. The IVD offsets need not be distinct, so every `ivdCount` may name
    // the same IVD; and `row_bytes` is zero when `regionIndexCount` is, so the bounds checks that
    // would stop an over-large `itemCount` never fail. 65,535 x 65,535 is 4.29e9 heap `Vec<i32>`
    // out of under 350 KB of font.
    const MAX_DELTA_ROWS: usize = 1_000_000;
    let mut rows_left = MAX_DELTA_ROWS;

    let mut ivd_data: Vec<Ivd> = Vec::with_capacity(ivd_count.min(256));
    for off in ivd_offsets {
        let ivd      = base + off;
        let items    = read_u16_be(buf, ivd).ok_or("IVS: IVD header truncated")? as usize;
        let word_cnt = read_u16_be(buf, ivd + 2).ok_or("IVS: IVD header truncated")? as usize;
        let reg_cnt  = read_u16_be(buf, ivd + 4).ok_or("IVS: IVD header truncated")? as usize;
        if !records_fit(ivd + 6, reg_cnt, 2, buf.len()) {
            return Err("IVS: IVD region index array does not fit".into());
        }
        let mut reg_idxs = Vec::with_capacity(reg_cnt);
        for j in 0..reg_cnt {
            let idx = read_u16_be(buf, ivd + 6 + j * 2).ok_or("IVS: IVD region index truncated")?;
            reg_idxs.push(idx as usize);
        }
        let long_words  = (word_cnt & 0x8000) != 0;
        let wc          = word_cnt & 0x7FFF;
        if wc > reg_cnt {
            return Err("IVS: IVD wordDeltaCount exceeds regionIndexCount".into());
        }
        let wide_size   = if long_words { 4 } else { 2 };
        let narrow_size = if long_words { 2 } else { 1 };
        let row_bytes   = wc * wide_size + (reg_cnt - wc) * narrow_size;
        let data_start  = ivd + 6 + reg_cnt * 2;

        rows_left = rows_left
            .checked_sub(items)
            .ok_or("IVS: delta row budget exhausted")?;
        let mut deltas: Vec<i32> = Vec::with_capacity(
            items.min(256).saturating_mul(reg_cnt).min(buf.len()),
        );
        for r in 0..items {
            let row_off = r.checked_mul(row_bytes).and_then(|d| data_start.checked_add(d))
                .ok_or("IVS: delta row offset overflows")?;
            let row = buf.get(row_off..row_off.checked_add(row_bytes).ok_or("IVS: delta row truncated")?)
                .ok_or("IVS: delta row truncated")?;
            let (wide, narrow) = row.split_at_checked(wc * wide_size).ok_or("IVS: delta row truncated")?;
            if long_words {
                deltas.extend(wide.chunks_exact(4).map(|c| i32::from_be_bytes([c[0], c[1], c[2], c[3]])));
                deltas.extend(narrow.chunks_exact(2).map(|c| i16::from_be_bytes([c[0], c[1]]) as i32));
            } else {
                deltas.extend(wide.chunks_exact(2).map(|c| i16::from_be_bytes([c[0], c[1]]) as i32));
                deltas.extend(narrow.iter().map(|&b| b as i8 as i32));
            }
        }
        ivd_data.push(Ivd { region_indices: reg_idxs, deltas });
    }

    Ok(ItemVariationStore { regions, ivd_data, axis_count })
}

pub fn parse_delta_set_index_map(buf: &[u8], base: usize) -> Result<Vec<(u32, u32)>, String> {
    let fmt       = *buf.get(base).ok_or("IVS: delta set index map truncated")?;
    let entry_fmt = *buf.get(base + 1).ok_or("IVS: delta set index map truncated")?;
    let map_count: usize = if fmt == 0 {
        read_u16_be(buf, base + 2).ok_or("IVS: delta set index map truncated")? as usize
    } else {
        read_u32_be(buf, base + 2).ok_or("IVS: delta set index map truncated")? as usize
    };
    let inner_bits = (entry_fmt & 0x0F) as usize + 1;
    let entry_size = ((entry_fmt >> 4) & 0x3) as usize + 1;
    let inner_mask = (1usize << inner_bits) - 1;
    let data_off   = if fmt == 0 { 4 } else { 6 };

    let need = map_count.checked_mul(entry_size)
        .and_then(|n| n.checked_add(base + data_off))
        .ok_or("IVS: delta set index map too large")?;
    if need > buf.len() { return Err("IVS: delta set index map truncated".into()); }

    let entries = buf.get(base + data_off..need).ok_or("IVS: delta set index map truncated")?;
    let map: Vec<(u32, u32)> = entries
        .chunks_exact(entry_size)
        .map(|e| {
            let val = e.iter().fold(0usize, |acc, &b| (acc << 8) | b as usize);
            ((val >> inner_bits) as u32, (val & inner_mask) as u32)
        })
        .collect();

    Ok(map)
}

pub fn delta_set_index_map_lookup(map: &[(u32, u32)], idx: usize) -> (usize, usize) {
    let (o, i) = if idx < map.len() { map[idx] } else { *map.last().unwrap_or(&(0, 0)) };
    (o as usize, i as usize)
}

pub fn precompute_region_scalars(store: &ItemVariationStore, location: &[f64]) -> Vec<f64> {
    (0..store.regions.len()).map(|reg_idx| region_scalar(store, reg_idx, location)).collect()
}

pub fn compute_ivs_delta_f64(store: &ItemVariationStore, outer_idx: usize, inner_idx: usize, region_scalars: &[f64]) -> f64 {
    let ivd       = match store.ivd_data.get(outer_idx) { Some(v) => v, None => return 0.0 };
    let delta_set = match ivd.row(inner_idx) { Some(v) => v, None => return 0.0 };
    let mut delta = 0.0f64;
    for (i, &reg_idx) in ivd.region_indices.iter().enumerate() {
        let scalar = region_scalars.get(reg_idx).copied().unwrap_or(0.0);
        delta += scalar * delta_set.get(i).copied().unwrap_or(0) as f64;
    }
    delta
}

fn region_scalar(store: &ItemVariationStore, reg_idx: usize, location: &[f64]) -> f64 {
    let region = match store.regions.get(reg_idx) { Some(r) => r, None => return 0.0 };
    let mut scalar = 1.0f64;
    for (j, &RegionAxis { start, peak, end }) in region.iter().enumerate().take(store.axis_count) {
        let loc = *location.get(j).unwrap_or(&0.0);
        if peak == 0.0 { continue; }
        if loc == peak { continue; }
        if loc < start || loc > end { scalar = 0.0; break; }
        scalar *= if loc < peak {
            (loc - start) / (peak - start)
        } else {
            (end - loc) / (end - peak)
        };
    }
    scalar
}

pub(crate) fn region_scalars(store: &ItemVariationStore, outer_idx: usize, location: &[f64]) -> Option<Vec<f64>> {
    let ivd = store.ivd_data.get(outer_idx)?;
    Some(ivd.region_indices.iter().map(|&reg_idx| region_scalar(store, reg_idx, location)).collect())
}
