use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::decoder::{read_u16_be, read_u32_be, parse_all_name_strings, records_fit};
use crate::daecore::daetype::TableBytes;

#[derive(Debug, PartialEq)]
pub struct StatAxis { pub tag: String, pub name: Option<String>, pub ordering: u16 }

#[derive(Debug, PartialEq)]
pub enum StatAxisValue {
    Single { axis_index: u16, name: Option<String>, value: f64, elidable: bool },
    Range  { axis_index: u16, name: Option<String>, nominal: f64, min: f64, max: f64, elidable: bool },
    Linked { axis_index: u16, name: Option<String>, value: f64, linked_value: f64, elidable: bool },
    Combo  { name: Option<String>, values: Vec<(u16, f64)>, elidable: bool },
}

const ELIDABLE_AXIS_VALUE_NAME: u16 = 0x0002;

fn read_fixed(data: &[u8], off: usize) -> Option<f64> {
    read_u32_be(data, off).map(|v| v as i32 as f64 / 65536.0)
}

pub type StatInfo = (Vec<StatAxis>, Vec<StatAxisValue>, Option<String>);

pub fn parse_stat(
    table_map: &BTreeMap<String, TableBytes>,
) -> Result<StatInfo, String> {
    let stat = table_map.get("STAT").ok_or("missing STAT")?;
    if stat.len() < 18 { return Err("STAT: header truncated".into()); }

    let minor_version               = read_u16_be(stat, 2).ok_or("STAT: header truncated")?;
    let design_axis_size            = read_u16_be(stat, 4).ok_or("STAT: header truncated")? as usize;
    let design_axis_count           = read_u16_be(stat, 6).ok_or("STAT: header truncated")? as usize;
    let design_axes_offset          = read_u32_be(stat, 8).ok_or("STAT: header truncated")? as usize;
    let axis_value_count            = read_u16_be(stat, 12).ok_or("STAT: header truncated")? as usize;
    let offset_to_axis_value_offsets = read_u32_be(stat, 14).ok_or("STAT: header truncated")? as usize;

    let names = parse_all_name_strings(table_map);

    const STAT_AXIS_RECORD_SIZE: usize = 8;
    if design_axis_count > 0 && design_axis_size < STAT_AXIS_RECORD_SIZE {
        return Err("STAT: designAxisSize is narrower than an AxisRecord".into());
    }
    if !records_fit(design_axes_offset, design_axis_count, design_axis_size, stat.len()) {
        return Err("STAT: design axis array does not fit the table".into());
    }
    let mut axes = Vec::with_capacity(design_axis_count);
    for i in 0..design_axis_count {
        let rec = design_axes_offset + i * design_axis_size;
        let tag_bytes = stat.get(rec..rec + 4).ok_or("STAT: design axis truncated")?;
        let tag      = String::from_utf8_lossy(tag_bytes).to_string();
        let name_id  = read_u16_be(stat, rec + 4).ok_or("STAT: design axis truncated")?;
        let ordering = read_u16_be(stat, rec + 6).ok_or("STAT: design axis truncated")?;
        axes.push(StatAxis { tag, name: names.get(&name_id).cloned(), ordering });
    }

    if !records_fit(offset_to_axis_value_offsets, axis_value_count, 2, stat.len()) {
        return Err("STAT: axis value offset array does not fit the table".into());
    }
    let mut combo_records_left = stat.len() / 6;

    let mut values = Vec::with_capacity(axis_value_count);
    for i in 0..axis_value_count {
        let off_rec = offset_to_axis_value_offsets + i * 2;
        let rel_off = read_u16_be(stat, off_rec).ok_or("STAT: axis value offset truncated")? as usize;
        let av = offset_to_axis_value_offsets + rel_off;

        let format = match read_u16_be(stat, av) { Some(f) => f, None => continue };
        match format {
            1 => {
                let (Some(axis_index), Some(flags), Some(name_id), Some(value)) = (
                    read_u16_be(stat, av + 2), read_u16_be(stat, av + 4),
                    read_u16_be(stat, av + 6), read_fixed(stat, av + 8),
                ) else { continue };
                values.push(StatAxisValue::Single {
                    axis_index, name: names.get(&name_id).cloned(), value,
                    elidable: flags & ELIDABLE_AXIS_VALUE_NAME != 0,
                });
            }
            2 => {
                let (Some(axis_index), Some(flags), Some(name_id), Some(nominal), Some(min), Some(max)) = (
                    read_u16_be(stat, av + 2), read_u16_be(stat, av + 4), read_u16_be(stat, av + 6),
                    read_fixed(stat, av + 8), read_fixed(stat, av + 12), read_fixed(stat, av + 16),
                ) else { continue };
                values.push(StatAxisValue::Range {
                    axis_index, name: names.get(&name_id).cloned(), nominal, min, max,
                    elidable: flags & ELIDABLE_AXIS_VALUE_NAME != 0,
                });
            }
            3 => {
                let (Some(axis_index), Some(flags), Some(name_id), Some(value), Some(linked_value)) = (
                    read_u16_be(stat, av + 2), read_u16_be(stat, av + 4), read_u16_be(stat, av + 6),
                    read_fixed(stat, av + 8), read_fixed(stat, av + 12),
                ) else { continue };
                values.push(StatAxisValue::Linked {
                    axis_index, name: names.get(&name_id).cloned(), value, linked_value,
                    elidable: flags & ELIDABLE_AXIS_VALUE_NAME != 0,
                });
            }
            4 => {
                let (Some(record_count), Some(flags), Some(name_id)) = (
                    read_u16_be(stat, av + 2), read_u16_be(stat, av + 4), read_u16_be(stat, av + 6),
                ) else { continue };
                let record_count = record_count as usize;
                if !records_fit(av.saturating_add(8), record_count, 6, stat.len()) { continue; }
                let Some(left) = combo_records_left.checked_sub(record_count) else { continue };
                combo_records_left = left;

                let mut combo_values = Vec::with_capacity(record_count);
                let mut ok = true;
                for j in 0..record_count {
                    let rec = av + 8 + j * 6;
                    match (read_u16_be(stat, rec), read_fixed(stat, rec + 2)) {
                        (Some(axis_index), Some(value)) => combo_values.push((axis_index, value)),
                        _ => { ok = false; break; }
                    }
                }
                if !ok { continue; }
                values.push(StatAxisValue::Combo {
                    name: names.get(&name_id).cloned(), values: combo_values,
                    elidable: flags & ELIDABLE_AXIS_VALUE_NAME != 0,
                });
            }
            _ => continue,
        }
    }

    let elided_fallback_name = if minor_version >= 1 && stat.len() >= 20 {
        read_u16_be(stat, 18).and_then(|id| names.get(&id).cloned())
    } else {
        None
    };

    Ok((axes, values, elided_fallback_name))
}
