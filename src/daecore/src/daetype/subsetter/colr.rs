use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::super::decoder::{read_offset24, read_u16_be, read_u32_be, write_offset24, write_u16_be, write_u32_be};
use super::super::colr_v0::{colr_v0_header, colr_v0_base_glyphs};
use super::super::colr_v1::{paint_layout, PaintBudget};
use super::otl::remap_gid;

const MAX_BASE_GLYPHS: usize = 65536;

pub fn colr_closure(colr: &[u8], active: &GlyphSet) -> Vec<u16> {
    let mut found = Vec::new();
    colr_v0_closure(colr, active, &mut found);
    colr_v1_closure(colr, active, &mut found);
    found
}

fn colr_v0_closure(colr: &[u8], active: &GlyphSet, found: &mut Vec<u16>) {
    let Some((_, _, layers_off, n_layer_records)) = colr_v0_header(colr) else { return };
    for (gid, first, n) in colr_v0_base_glyphs(colr) {
        if !active.contains(&gid) { continue; }
        for i in 0..n.min(65536) {
            let idx = first + i;
            if idx >= n_layer_records { break; }
            if let Some(layer_gid) = read_u16_be(colr, layers_off + idx * 4) {
                found.push(layer_gid);
            }
        }
    }
}

fn colr_v1_closure(colr: &[u8], active: &GlyphSet, found: &mut Vec<u16>) {
    if colr.len() < 34 { return; }
    if read_u16_be(colr, 0) != Some(1) { return; }
    let base_glyph_list_off = match read_u32_be(colr, 14) {
        Some(v) if v != 0 => v as usize,
        _ => return,
    };
    let layer_list_raw = read_u32_be(colr, 18).unwrap_or(0);
    let layer_list_off = if layer_list_raw == 0 { None } else { Some(layer_list_raw as usize) };

    let Some(num_records) = read_u32_be(colr, base_glyph_list_off) else { return };
    let mut budget = PaintBudget::new();
    for i in 0..(num_records as usize).min(MAX_BASE_GLYPHS) {
        let rec = base_glyph_list_off + 4 + i * 6;
        let (Some(gid), Some(paint_rel)) = (read_u16_be(colr, rec), read_u32_be(colr, rec + 2)) else { continue };
        if !active.contains(&gid) { continue; }
        walk_paint_closure(colr, base_glyph_list_off + paint_rel as usize, layer_list_off, found, &mut budget);
    }
}

fn walk_paint_closure(colr: &[u8], off: usize, layer_list_off: Option<usize>, found: &mut Vec<u16>, budget: &mut PaintBudget) {
    if !budget.enter() { return; }
    walk_paint_closure_inner(colr, off, layer_list_off, found, budget);
    budget.leave();
}

fn walk_paint_closure_inner(colr: &[u8], off: usize, layer_list_off: Option<usize>, found: &mut Vec<u16>, budget: &mut PaintBudget) {
    let Some(format) = colr.get(off).copied() else { return };
    let Some(layout) = paint_layout(format) else { return };

    for &pos in layout.glyph_ids {
        if let Some(glyph_id) = read_u16_be(colr, off + pos) { found.push(glyph_id); }
    }

    if format == 1 {
        let Some(num_layers) = colr.get(off + 1).copied() else { return };
        let Some(first_layer_index) = read_u32_be(colr, off + 2) else { return };
        let Some(layer_list_off) = layer_list_off else { return };
        let Some(num_layers_total) = read_u32_be(colr, layer_list_off) else { return };
        for i in 0..(num_layers as usize).min(256) {
            let idx = first_layer_index as usize + i;
            if idx >= num_layers_total as usize { return; }
            let Some(rel) = read_u32_be(colr, layer_list_off + 4 + idx * 4) else { return };
            walk_paint_closure(colr, layer_list_off + rel as usize, Some(layer_list_off), found, budget);
        }
        return;
    }

    for &pos in layout.children {
        if let Some(child_off) = read_offset24(colr, off + pos) {
            walk_paint_closure(colr, off + child_off, layer_list_off, found, budget);
        }
    }
}

pub fn subset_colr(colr: &[u8], active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let has_v0 = colr_v0_header(colr).is_some_and(|(n, ..)| n > 0);
    let is_v1 = colr.len() >= 34 && read_u16_be(colr, 0) == Some(1);
    let v1_present = is_v1 && read_u32_be(colr, 14).unwrap_or(0) != 0;
    if !has_v0 && !v1_present { return None; }

    let mut out = colr.to_vec();

    let mut new_v0: Option<(usize, usize, usize, usize)> = None;
    if has_v0 {
        new_v0 = rebuild_v0(colr, &mut out, active, gid_map);
    }

    let mut new_v1_off: Option<usize> = None;
    if v1_present {
        new_v1_off = rebuild_v1(colr, &mut out, active, gid_map);
    }

    if new_v0.is_none() && new_v1_off.is_none() { return None; }

    if let Some((base_off, base_count, layers_off, layers_count)) = new_v0 {
        write_u16_be(&mut out, 2, base_count as u16);
        write_u32_be(&mut out, 4, base_off as u32);
        write_u32_be(&mut out, 8, layers_off as u32);
        write_u16_be(&mut out, 12, layers_count as u16);
    } else if has_v0 {
        write_u16_be(&mut out, 2, 0);
        write_u32_be(&mut out, 4, 0);
        write_u32_be(&mut out, 8, 0);
        write_u16_be(&mut out, 12, 0);
    }
    if is_v1 {
        write_u32_be(&mut out, 14, new_v1_off.unwrap_or(0) as u32);
        if colr.len() >= 26 { write_u32_be(&mut out, 22, 0); }
    }

    Some(out)
}

fn rebuild_v0(colr: &[u8], out: &mut Vec<u8>, active: &GlyphSet, gid_map: &[u16]) -> Option<(usize, usize, usize, usize)> {
    let (_, _, orig_layers_off, orig_n_layers) = colr_v0_header(colr)?;
    let mut survivors: Vec<(u16, usize, usize)> = colr_v0_base_glyphs(colr).into_iter()
        .filter_map(|(gid, first, n)| remap_gid(active, gid_map, gid).map(|ng| (ng, first, n)))
        .collect();
    if survivors.is_empty() { return None; }
    survivors.sort_unstable_by_key(|&(gid, _, _)| gid);

    let layers_off = out.len();
    let mut base_records: Vec<(u16, u16, u16)> = Vec::with_capacity(survivors.len());
    let mut layer_count = 0usize;
    for &(gid, first, n) in &survivors {
        let new_first = layer_count;
        let mut actual_n = 0usize;
        for i in 0..n.min(65536) {
            let idx = first + i;
            if idx >= orig_n_layers { break; }
            let rec = orig_layers_off + idx * 4;
            let (Some(orig_gid), Some(pal)) = (read_u16_be(colr, rec), read_u16_be(colr, rec + 2)) else { break };
            let new_gid = remap_gid(active, gid_map, orig_gid).unwrap_or(orig_gid);
            let mut lrec = [0u8; 4];
            write_u16_be(&mut lrec, 0, new_gid);
            write_u16_be(&mut lrec, 2, pal);
            out.extend_from_slice(&lrec);
            layer_count += 1;
            actual_n += 1;
        }
        base_records.push((gid, new_first as u16, actual_n as u16));
    }

    let base_off = out.len();
    for (gid, first, n) in &base_records {
        let mut rec = [0u8; 6];
        write_u16_be(&mut rec, 0, *gid);
        write_u16_be(&mut rec, 2, *first);
        write_u16_be(&mut rec, 4, *n);
        out.extend_from_slice(&rec);
    }

    Some((base_off, base_records.len(), layers_off, layer_count))
}

fn rebuild_v1(colr: &[u8], out: &mut Vec<u8>, active: &GlyphSet, gid_map: &[u16]) -> Option<usize> {
    let base_glyph_list_off = match read_u32_be(colr, 14) {
        Some(v) if v != 0 => v as usize,
        _ => return None,
    };
    let layer_list_raw = read_u32_be(colr, 18).unwrap_or(0);
    let layer_list_off = if layer_list_raw == 0 { None } else { Some(layer_list_raw as usize) };
    let num_records = read_u32_be(colr, base_glyph_list_off)?;

    let mut survivors: Vec<(u16, usize)> = Vec::new();
    for i in 0..(num_records as usize).min(MAX_BASE_GLYPHS) {
        let rec = base_glyph_list_off + 4 + i * 6;
        let (Some(gid), Some(paint_rel)) = (read_u16_be(colr, rec), read_u32_be(colr, rec + 2)) else { continue };
        let Some(new_gid) = remap_gid(active, gid_map, gid) else { continue };
        survivors.push((new_gid, base_glyph_list_off + paint_rel as usize));
    }
    if survivors.is_empty() { return None; }
    survivors.sort_unstable_by_key(|&(gid, _)| gid);

    let mut builder = ColrV1Builder { colr, gid_map, layer_entries: Vec::new() };
    let mut budget = PaintBudget::new();
    let mut built: Vec<(u16, Vec<u8>)> = Vec::with_capacity(survivors.len());
    for (new_gid, paint_off) in &survivors {
        let blob = builder.rebuild_paint(*paint_off, layer_list_off, &mut budget)?;
        built.push((*new_gid, blob));
    }

    let base_glyph_list_start = out.len();
    write_u32_be_push(out, built.len() as u32);
    let records_start = out.len();
    out.extend(core::iter::repeat_n(0u8, built.len() * 6));
    let mut tail: Vec<u8> = Vec::new();
    for (i, (new_gid, blob)) in built.iter().enumerate() {
        let rec = records_start + i * 6;
        write_u16_be(out, rec, *new_gid);
        let rel = (out.len() - base_glyph_list_start) + tail.len();
        write_u32_be(out, rec + 2, rel as u32);
        tail.extend_from_slice(blob);
    }
    out.extend_from_slice(&tail);

    if !builder.layer_entries.is_empty() {
        let layer_list_start = out.len();
        write_u32_be_push(out, builder.layer_entries.len() as u32);
        let entries_start = out.len();
        out.extend(core::iter::repeat_n(0u8, builder.layer_entries.len() * 4));
        let mut ltail: Vec<u8> = Vec::new();
        for (i, entry) in builder.layer_entries.iter().enumerate() {
            let rel = (out.len() - layer_list_start) + ltail.len();
            write_u32_be(out, entries_start + i * 4, rel as u32);
            ltail.extend_from_slice(entry);
        }
        out.extend_from_slice(&ltail);
        write_u32_be(out, 18, layer_list_start as u32);
    }

    Some(base_glyph_list_start)
}

fn write_u32_be_push(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

struct ColrV1Builder<'a> {
    colr: &'a [u8],
    gid_map: &'a [u16],
    layer_entries: Vec<Vec<u8>>,
}

impl ColrV1Builder<'_> {
    fn rebuild_paint(&mut self, off: usize, layer_list_off: Option<usize>, budget: &mut PaintBudget) -> Option<Vec<u8>> {
        if !budget.enter() { return None; }
        let result = self.rebuild_paint_inner(off, layer_list_off, budget);
        budget.leave();
        result
    }

    fn place_child(&mut self, header: &[u8], tail: &mut Vec<u8>, target: usize, layer_list_off: Option<usize>, budget: &mut PaintBudget) -> Option<usize> {
        let built = self.rebuild_paint(target, layer_list_off, budget)?;
        let rel = header.len() + tail.len();
        tail.extend_from_slice(&built);
        Some(rel)
    }

    fn place_verbatim(&self, header: &[u8], tail: &mut Vec<u8>, target: usize, len: usize) -> Option<usize> {
        let bytes = self.colr.get(target..target + len)?;
        let rel = header.len() + tail.len();
        tail.extend_from_slice(bytes);
        Some(rel)
    }

    fn rebuild_paint_inner(&mut self, off: usize, layer_list_off: Option<usize>, budget: &mut PaintBudget) -> Option<Vec<u8>> {
        let colr = self.colr;
        let format = *colr.get(off)?;
        match format {
            1 => {
                let num_layers = *colr.get(off + 1)? as usize;
                let first_layer_index = read_u32_be(colr, off + 2)? as usize;
                let layer_list_off = layer_list_off?;
                let num_layers_total = read_u32_be(colr, layer_list_off)? as usize;
                let new_first = self.layer_entries.len();
                for i in 0..num_layers.min(256) {
                    let idx = first_layer_index + i;
                    if idx >= num_layers_total { return None; }
                    let entry_off = layer_list_off + 4 + idx * 4;
                    let rel = read_u32_be(colr, entry_off)? as usize;
                    let built = self.rebuild_paint(layer_list_off + rel, Some(layer_list_off), budget)?;
                    self.layer_entries.push(built);
                }
                let mut out = vec![0u8; 6];
                out[0] = 1;
                out[1] = num_layers as u8;
                write_u32_be(&mut out, 2, new_first as u32);
                Some(out)
            }
            2 => Some(colr.get(off..off + 5)?.to_vec()),
            3 => Some(colr.get(off..off + 9)?.to_vec()),
            4 | 5 => self.rebuild_gradient(off, if format == 5 { 20 } else { 16 }, format == 5),
            6 | 7 => self.rebuild_gradient(off, if format == 7 { 20 } else { 16 }, format == 7),
            8 | 9 => self.rebuild_gradient(off, if format == 9 { 16 } else { 12 }, format == 9),
            10 => {
                let mut out = self.rebuild_with_children(off, format, layer_list_off, budget)?;
                if let Some(g) = read_u16_be(colr, off + 4) {
                    write_u16_be(&mut out, 4, remap_gid_raw(self.gid_map, g));
                }
                Some(out)
            }
            11 => {
                let mut header = colr.get(off..off + 3)?.to_vec();
                let g = read_u16_be(colr, off + 1)?;
                write_u16_be(&mut header, 1, remap_gid_raw(self.gid_map, g));
                Some(header)
            }
            12 | 13 => {
                let transform_len = if format == 13 { 28 } else { 24 };
                let mut header = colr.get(off..off + 7)?.to_vec();
                let mut tail = Vec::new();
                let child_target = off + read_offset24(colr, off + 1)?;
                let child_rel = self.place_child(&header, &mut tail, child_target, layer_list_off, budget)?;
                write_offset24(&mut header, 1, child_rel);
                let transform_target = off + read_offset24(colr, off + 4)?;
                let transform_rel = self.place_verbatim(&header, &mut tail, transform_target, transform_len)?;
                write_offset24(&mut header, 4, transform_rel);
                header.extend(tail);
                Some(header)
            }
            14..=32 => self.rebuild_with_children(off, format, layer_list_off, budget),
            _ => None,
        }
    }

    fn rebuild_with_children(&mut self, off: usize, format: u8, layer_list_off: Option<usize>, budget: &mut PaintBudget) -> Option<Vec<u8>> {
        let layout = paint_layout(format)?;
        let mut header = self.colr.get(off..off + layout.inline_len)?.to_vec();
        let mut tail = Vec::new();
        for &pos in layout.children {
            let target = off + read_offset24(self.colr, off + pos)?;
            let rel = self.place_child(&header, &mut tail, target, layer_list_off, budget)?;
            write_offset24(&mut header, pos, rel);
        }
        header.extend(tail);
        Some(header)
    }

    fn rebuild_gradient(&mut self, off: usize, inline_len: usize, is_var: bool) -> Option<Vec<u8>> {
        let colr = self.colr;
        let mut header = colr.get(off..off + inline_len)?.to_vec();
        let mut tail = Vec::new();
        let cl_target = off + read_offset24(colr, off + 1)?;
        let cl_len = color_line_len(colr, cl_target, is_var)?;
        let rel = self.place_verbatim(&header, &mut tail, cl_target, cl_len)?;
        write_offset24(&mut header, 1, rel);
        header.extend(tail);
        Some(header)
    }
}

fn remap_gid_raw(gid_map: &[u16], orig: u16) -> u16 {
    gid_map.get(orig as usize).copied().unwrap_or(orig)
}

fn color_line_len(colr: &[u8], off: usize, is_var: bool) -> Option<usize> {
    let num_stops = read_u16_be(colr, off + 1)? as usize;
    let stop_size = if is_var { 10 } else { 6 };
    Some(3 + num_stops * stop_size)
}
