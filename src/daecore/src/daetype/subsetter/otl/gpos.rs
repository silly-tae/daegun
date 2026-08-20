use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
#[allow(unused_imports)]
use crate::daecore::daetype::decoder::{read_u16_be, read_u32_be, write_u16_be};
use super::lookup_list;
use super::generic::{self, schemas};

pub(crate) fn subset_gpos_subtable(_effective_type: u16, schema: Option<&generic::schema::Schema>, buf: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    generic::subset_subtable(buf, off, schema?, active, gid_map)
}

fn subset_gpos_subtable_stripping_devices(_effective_type: u16, schema: Option<&generic::schema::Schema>, buf: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    generic::subset_subtable_stripping_devices(buf, off, schema?, active, gid_map)
}

pub fn subset_gpos(gpos: &[u8], active: &GlyphSet, gid_map: &[u16], mark_filter_sets_survive: bool) -> Option<Vec<u8>> {
    lookup_list::subset_lookup_table(gpos, 9, active, gid_map, mark_filter_sets_survive, &subset_gpos_subtable, &schemas::gpos_schema_for_type)
        .or_else(|| lookup_list::subset_lookup_table(
            gpos, 9, active, gid_map, mark_filter_sets_survive, &subset_gpos_subtable_stripping_devices, &schemas::gpos_schema_for_type,
        ))
}
