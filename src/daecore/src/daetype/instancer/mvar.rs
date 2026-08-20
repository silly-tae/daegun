use alloc::string::String;
use alloc::collections::BTreeMap;
use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be, write_u16_be, write_i16_be};
use super::super::format::ivs::{parse_item_variation_store, compute_ivs_delta_f64, precompute_region_scalars};
use super::super::format::round::ot_round;
use crate::daecore::daetype::TableBytes;

pub fn mvar_deltas(
    table_map: &BTreeMap<String, TableBytes>,
    location:  &[f64],
) -> Result<BTreeMap<[u8; 4], i32>, String> {
    let mut out = BTreeMap::new();
    let mvar = match table_map.get("MVAR") { Some(m) => m, None => return Ok(out) };

    let value_record_size  = read_u16_be(mvar, 6).ok_or("MVAR: header truncated")? as usize;
    let value_record_count = read_u16_be(mvar, 8).ok_or("MVAR: header truncated")? as usize;
    let ivs_off            = read_u16_be(mvar, 10).ok_or("MVAR: header truncated")? as usize;
    if ivs_off == 0 || value_record_count == 0 { return Ok(out); }
    if value_record_size < 8 { return Err("MVAR: valueRecordSize too small".into()); }

    let store = parse_item_variation_store(mvar, ivs_off)?;
    let region_scalars = precompute_region_scalars(&store, location);

    let records_base = 12usize;
    for r in 0..value_record_count {
        let rec = records_base + r * value_record_size;
        let tag   = read_u32_be(mvar, rec).ok_or("MVAR: value record truncated")?;
        let outer = read_u16_be(mvar, rec + 4).ok_or("MVAR: value record truncated")? as usize;
        let inner = read_u16_be(mvar, rec + 6).ok_or("MVAR: value record truncated")? as usize;
        let delta = ot_round(compute_ivs_delta_f64(&store, outer, inner, &region_scalars));
        if delta != 0 {
            out.insert(tag.to_be_bytes(), delta);
        }
    }
    Ok(out)
}

pub fn apply_mvar(
    table_map: &BTreeMap<String, TableBytes>,
    hhea:      &mut [u8],
    os2:       &mut [u8],
    post:      &mut [u8],
    location:  &[f64],
) -> Result<(), String> {
    for (tag, delta) in mvar_deltas(table_map, location)? {
        let target: Option<(&mut [u8], usize, bool)> = match &tag {
            b"hasc" => Some((os2,  68, false)),
            b"hdsc" => Some((os2,  70, false)),
            b"hlgp" => Some((os2,  72, false)),
            b"hcla" => Some((os2,  74, true)),
            b"hcld" => Some((os2,  76, true)),
            b"xhgt" => Some((os2,  86, false)),
            b"cpht" => Some((os2,  88, false)),
            b"sbxs" => Some((os2,  10, false)),
            b"sbys" => Some((os2,  12, false)),
            b"sbxo" => Some((os2,  14, false)),
            b"sbyo" => Some((os2,  16, false)),
            b"spxs" => Some((os2,  18, false)),
            b"spys" => Some((os2,  20, false)),
            b"spxo" => Some((os2,  22, false)),
            b"spyo" => Some((os2,  24, false)),
            b"strs" => Some((os2,  26, false)),
            b"stro" => Some((os2,  28, false)),
            b"undo" => Some((post,  8, false)),
            b"unds" => Some((post, 10, false)),
            _       => None,
        };
        if let Some((buf, off, unsigned)) = target {
            bump(buf, off, delta, unsigned);
        }
        match &tag {
            b"hasc" => bump(hhea, 4, delta, false),
            b"hdsc" => bump(hhea, 6, delta, false),
            b"hlgp" => bump(hhea, 8, delta, false),
            _ => {}
        }
    }
    Ok(())
}

fn bump(buf: &mut [u8], off: usize, delta: i32, unsigned: bool) {
    if off + 2 > buf.len() { return; }
    if unsigned {
        let v = read_u16_be(buf, off).unwrap_or(0) as i32;
        write_u16_be(buf, off, v.saturating_add(delta).clamp(0, 65535) as u16);
    } else {
        let v = read_i16_be(buf, off).unwrap_or(0) as i32;
        write_i16_be(buf, off, v.saturating_add(delta).clamp(-32768, 32767) as i16);
    }
}
