use alloc::string::String;
use alloc::collections::BTreeMap;
use super::decoder::{read_i16_be, read_u16_be, search_records};
use crate::daecore::daetype::TableBytes;

pub fn vorg_default_origin_y(table_map: &BTreeMap<String, TableBytes>) -> Option<i16> {
    let vorg = table_map.get("VORG")?;
    read_i16_be(vorg, 4)
}

pub fn vorg_origin_y(table_map: &BTreeMap<String, TableBytes>, gid: u16) -> Option<i16> {
    let vorg = table_map.get("VORG")?;
    let default = read_i16_be(vorg, 4)?;
    let count = read_u16_be(vorg, 6)? as usize;

    match search_records(count, gid as u32, |i| read_u16_be(vorg, 8 + i * 4).map(u32::from))? {
        Ok(i) => read_i16_be(vorg, 8 + i * 4 + 2),
        Err(_) => Some(default),
    }
}
