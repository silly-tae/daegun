use alloc::vec::Vec;

use super::format::coverage::coverage_index;
use super::decoder::{read_i16_be, read_u16_be};
use super::instancer::extract_coords;
use super::format::ivs::{compute_ivs_delta_f64, parse_item_variation_store, precompute_region_scalars};

const LIG_CARET_LIST_OFF: usize = 8;
const ITEM_VAR_STORE_OFF: usize = 14;

// Not shared with the subsetter's reader, which drops format 3's device table – right for the
// static instance it is producing, wrong here.
pub fn ligature_carets(
    gdef: &[u8],
    gid: u16,
    outline: Option<(&[u8], &[usize])>,
    location: &[f64],
) -> Vec<f64> {
    let mut out = Vec::new();
    let Some(list) = lig_caret_list(gdef) else { return out };

    let Some(cov_off) = read_u16_be(gdef, list).map(usize::from) else { return out };
    if cov_off == 0 { return out; }
    let Some(cov) = gdef.get(list + cov_off..) else { return out };
    let Some(index) = coverage_index(cov, gid) else { return out };

    let Some(count) = read_u16_be(gdef, list + 2) else { return out };
    if index >= count { return out; }

    let Some(lig_off) = read_u16_be(gdef, list + 4 + index as usize * 2).map(usize::from) else { return out };
    if lig_off == 0 { return out; }
    let lig = list + lig_off;

    let Some(caret_count) = read_u16_be(gdef, lig) else { return out };
    let vars = VarResolver::new(gdef, location);
    let mut points = LazyOutline::new(outline);

    for i in 0..caret_count as usize {
        let Some(off) = read_u16_be(gdef, lig + 2 + i * 2).map(usize::from) else { break };
        if off == 0 { continue; }
        let at = lig + off;
        let Some(value) = resolve(gdef, at, &mut points, gid, &vars) else { continue };
        out.push(value);
    }

    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    out
}

fn lig_caret_list(gdef: &[u8]) -> Option<usize> {
    let off = read_u16_be(gdef, LIG_CARET_LIST_OFF)? as usize;
    if off == 0 { return None; }
    (off < gdef.len()).then_some(off)
}

fn resolve(
    gdef: &[u8],
    at: usize,
    outline: &mut LazyOutline,
    gid: u16,
    vars: &Option<VarResolver>,
) -> Option<f64> {
    match read_u16_be(gdef, at)? {
        1 => Some(read_i16_be(gdef, at + 2)? as f64),
        2 => {
            let point = read_u16_be(gdef, at + 2)? as usize;
            outline.point_x(gid, point).map(f64::from)
        }
        3 => {
            let base = read_i16_be(gdef, at + 2)? as f64;
            let device = read_u16_be(gdef, at + 4)? as usize;
            Some(base + device_delta(gdef, at + device, vars))
        }
        _ => None,
    }
}

struct LazyOutline<'a> {
    source: Option<(&'a [u8], &'a [usize])>,
    xs: Option<Option<Vec<i32>>>,
}

impl<'a> LazyOutline<'a> {
    fn new(source: Option<(&'a [u8], &'a [usize])>) -> LazyOutline<'a> {
        LazyOutline { source, xs: None }
    }

    fn point_x(&mut self, gid: u16, point: usize) -> Option<i32> {
        if self.xs.is_none() {
            self.xs = Some(self.source.and_then(|(glyf, loca)| glyph_x_coords(glyf, loca, gid)));
        }
        self.xs.as_ref()?.as_ref()?.get(point).copied()
    }
}

fn glyph_x_coords(glyf: &[u8], loca: &[usize], gid: u16) -> Option<Vec<i32>> {
    let gid = gid as usize;
    if gid + 1 >= loca.len() { return None; }
    let (start, end) = (loca[gid], loca[gid + 1]);
    if end <= start { return None; }
    let n_contours = read_i16_be(glyf, start)?;
    if n_contours < 0 { return None; }
    Some(extract_coords(glyf, start, n_contours as usize).x_coords)
}

struct VarResolver {
    store: super::format::ivs::ItemVariationStore,
    scalars: Vec<f64>,
}

impl VarResolver {
    fn new(gdef: &[u8], location: &[f64]) -> Option<VarResolver> {
        let minor = read_u16_be(gdef, 2)?;
        if minor < 3 || location.is_empty() { return None; }
        let off = super::decoder::read_u32_be(gdef, ITEM_VAR_STORE_OFF)? as usize;
        if off == 0 { return None; }
        let store = parse_item_variation_store(gdef, off).ok()?;
        let scalars = precompute_region_scalars(&store, location);
        Some(VarResolver { store, scalars })
    }
}

fn device_delta(gdef: &[u8], at: usize, vars: &Option<VarResolver>) -> f64 {
    let Some(vars) = vars else { return 0.0 };
    if read_u16_be(gdef, at + 4) != Some(0x8000) { return 0.0; }
    let (Some(outer), Some(inner)) = (read_u16_be(gdef, at), read_u16_be(gdef, at + 2)) else {
        return 0.0;
    };
    compute_ivs_delta_f64(&vars.store, outer as usize, inner as usize, &vars.scalars)
}
