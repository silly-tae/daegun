use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::string::String;
use alloc::vec::Vec;
use crate::daecore::daetype::decoder::read_u16_be;
use super::{parse_coverage, build_coverage, remap_gid};

pub(crate) fn parse_coverage_array(buf: &[u8], base: usize, array_off: usize, count: usize) -> Result<Vec<Vec<u16>>, String> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let rel = read_u16_be(buf, array_off + i * 2).ok_or("Coverage array: offset truncated")? as usize;
        out.push(parse_coverage(buf, base + rel)?);
    }
    Ok(out)
}

pub(crate) fn filter_coverage_group(group: &[Vec<u16>], active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<Vec<u16>>> {
    group.iter().map(|pos| {
        let surviving: Vec<u16> = pos.iter().filter_map(|&g| remap_gid(active, gid_map, g)).collect();
        if surviving.is_empty() { None } else { Some(surviving) }
    }).collect()
}

pub(crate) fn build_coverage_array_blobs(positions: &[Vec<u16>]) -> Vec<Vec<u8>> {
    positions.iter().map(|p| build_coverage(p)).collect()
}
