use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::super::decoder::{write_u16_be, write_u32_be};
use crate::daecore::daetype::TableBytes;

pub fn parse_loca(table_map: &BTreeMap<String, TableBytes>, loca_format: i16, num_glyphs: usize) -> Result<Vec<usize>, String> {
    let loca = table_map.get("loca").ok_or("missing loca")?;
    Ok(super::super::subsetter::parse_loca(loca, loca_format, num_glyphs))
}

pub fn build_loca_table(new_loca: &[usize], loca_format: i16) -> Vec<u8> {
    let n = new_loca.len();
    if loca_format == 0 {
        let mut out = vec![0u8; n * 2];
        for (i, &v) in new_loca.iter().enumerate() { write_u16_be(&mut out, i * 2, (v / 2) as u16); }
        out
    } else {
        let mut out = vec![0u8; n * 4];
        for (i, &v) in new_loca.iter().enumerate() { write_u32_be(&mut out, i * 4, v as u32); }
        out
    }
}
