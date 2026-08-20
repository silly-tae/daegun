use alloc::string::String;
use alloc::vec::Vec;
mod format;
mod paint;
mod color_line;
mod varfield;
mod instance;

use alloc::collections::BTreeMap;
use super::decoder::{read_u16_be, read_u32_be, search_records};
use super::format::ivs::{parse_delta_set_index_map, parse_item_variation_store, precompute_region_scalars, ItemVariationStore};
use crate::daecore::daetype::TableBytes;

pub use paint::{ColorStop, Paint};
pub(crate) use format::paint_layout;
pub(crate) use instance::instance_colr_v1;

pub(crate) const MAX_PAINT_DEPTH: usize = 64;

pub(crate) const MAX_PAINT_VISITS: usize = 100_000;

pub(crate) const MAX_PAINT_STOPS: usize = 1_000_000;

pub(crate) struct PaintBudget {
    depth: usize,
    visits: usize,
    stops: usize,
}

impl PaintBudget {
    pub fn new() -> Self {
        PaintBudget { depth: 0, visits: MAX_PAINT_VISITS, stops: MAX_PAINT_STOPS }
    }

    #[must_use]
    pub(crate) fn spend_stops(&mut self, n: usize) -> bool {
        match self.stops.checked_sub(n) {
            Some(left) => { self.stops = left; true }
            None => false,
        }
    }

    #[must_use]
    pub(crate) fn enter(&mut self) -> bool {
        if self.depth >= MAX_PAINT_DEPTH || self.visits == 0 {
            return false;
        }
        self.depth += 1;
        self.visits -= 1;
        true
    }

    pub(crate) fn leave(&mut self) {
        self.depth -= 1;
    }

}

impl Default for PaintBudget {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ColrV1VarData {
    pub var_store:     Option<ItemVariationStore>,
    pub var_index_map: Option<Vec<(u32, u32)>>,
}

pub fn parse_colr_v1_var_data(colr: &[u8]) -> ColrV1VarData {
    let var_index_map_off = read_u32_be(colr, 26).unwrap_or(0);
    let ivs_off            = read_u32_be(colr, 30).unwrap_or(0);
    let var_store = if ivs_off != 0 {
        parse_item_variation_store(colr, ivs_off as usize).ok()
    } else {
        None
    };
    let var_index_map = if var_index_map_off != 0 && var_store.is_some() {
        parse_delta_set_index_map(colr, var_index_map_off as usize).ok()
    } else {
        None
    };
    ColrV1VarData { var_store, var_index_map }
}

pub(crate) struct Colrv1Ctx<'a> {
    pub colr:            &'a [u8],
    pub cpal:             Option<&'a [u8]>,
    pub palette:          Option<super::colr_v0::CpalPalette>,
    pub var_store:        Option<&'a ItemVariationStore>,
    pub region_scalars:   &'a [f64],
    pub var_index_map:    Option<&'a [(u32, u32)]>,
    pub base_glyph_list_off: usize,
    pub layer_list_off:   Option<usize>,
}

#[allow(dead_code, reason = "the uncached entry point, for a caller holding raw COLR bytes rather than a FontCache; no in-tree caller since the fuzz harness was removed, and FontCache::colr_v1_paint is what production uses")]
pub fn colr_v1_paint_graph(
    table_map:     &BTreeMap<String, TableBytes>,
    gid:           u16,
    location:      &[f64],
    palette_index: u16,
) -> Option<Paint> {
    let colr = table_map.get("COLR")?;
    let var_data = parse_colr_v1_var_data(colr);
    colr_v1_paint_graph_impl(table_map, gid, location, palette_index, &var_data)
}

pub fn colr_v1_paint_graph_cached(
    table_map:     &BTreeMap<String, TableBytes>,
    gid:           u16,
    location:      &[f64],
    palette_index: u16,
    var_data:      &ColrV1VarData,
) -> Option<Paint> {
    colr_v1_paint_graph_impl(table_map, gid, location, palette_index, var_data)
}

pub fn colr_v1_region_scalars(var_data: &ColrV1VarData, location: &[f64]) -> Vec<f64> {
    match &var_data.var_store {
        Some(store) => precompute_region_scalars(store, location),
        None => Vec::new(),
    }
}

pub fn colr_v1_paint_graph_with_scalars(
    table_map:      &BTreeMap<String, TableBytes>,
    gid:            u16,
    palette_index:  u16,
    var_data:       &ColrV1VarData,
    region_scalars: &[f64],
) -> Option<Paint> {
    build(table_map, gid, palette_index, var_data, region_scalars)
}

fn colr_v1_paint_graph_impl(
    table_map:     &BTreeMap<String, TableBytes>,
    gid:           u16,
    location:      &[f64],
    palette_index: u16,
    var_data:      &ColrV1VarData,
) -> Option<Paint> {
    let scalars = colr_v1_region_scalars(var_data, location);
    build(table_map, gid, palette_index, var_data, &scalars)
}

fn build(
    table_map:      &BTreeMap<String, TableBytes>,
    gid:            u16,
    palette_index:  u16,
    var_data:       &ColrV1VarData,
    region_scalars: &[f64],
) -> Option<Paint> {
    let colr = table_map.get("COLR")?;
    if colr.len() < 34 { return None; }
    if read_u16_be(colr, 0)? != 1 { return None; }

    let base_glyph_list_off = read_u32_be(colr, 14)? as usize;
    if base_glyph_list_off == 0 { return None; }

    let layer_list_raw = read_u32_be(colr, 18)?;
    let layer_list_off = if layer_list_raw == 0 { None } else { Some(layer_list_raw as usize) };

    let cpal = table_map.get("CPAL").map(|v| v.as_slice());
    let palette = cpal.and_then(|c| super::colr_v0::CpalPalette::new(c, palette_index));
    let ctx = Colrv1Ctx {
        colr, cpal, palette,
        var_store: var_data.var_store.as_ref(),
        region_scalars,
        var_index_map: var_data.var_index_map.as_deref(),
        base_glyph_list_off, layer_list_off,
    };

    let mut budget = PaintBudget::new();
    resolve_glyph_paint(&ctx, gid, &mut budget)
}

pub(super) fn resolve_glyph_paint(ctx: &Colrv1Ctx, gid: u16, budget: &mut PaintBudget) -> Option<Paint> {
    let paint_off = lookup_base_glyph_paint_offset(ctx.colr, ctx.base_glyph_list_off, gid)?;
    paint::parse_paint(ctx, paint_off, budget)
}

fn lookup_base_glyph_paint_offset(colr: &[u8], base_glyph_list_off: usize, gid: u16) -> Option<usize> {
    let num_records = read_u32_be(colr, base_glyph_list_off)? as usize;
    let records_off = base_glyph_list_off + 4;
    let hit = search_records(num_records, gid as u32, |i| read_u16_be(colr, records_off + i * 6).map(u32::from))?.ok()?;
    let rel_off = read_u32_be(colr, records_off + hit * 6 + 2)? as usize;
    Some(base_glyph_list_off + rel_off)
}
