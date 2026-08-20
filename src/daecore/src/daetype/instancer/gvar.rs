use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::super::decoder::{pad4, read_u16_be, read_u32_be, read_i16_be};
use super::super::format::round::ot_round;
use super::coords::{GlyphCoords, extract_coords, iup, apply_simple_glyph_deltas, count_composite_components, apply_composite_glyph_deltas};
use crate::daecore::daetype::TableBytes;

const MAX_INSTANCED_GLYF_SIZE: usize = 1024 * 1024 * 1024;

pub struct GvarResult {
    pub glyf_data: Vec<u8>,
    pub new_loca:  Vec<usize>,
    pub advance_deltas: Vec<f64>,
    pub lsb_new: Vec<Option<i32>>,
    pub vadvance_deltas: Vec<f64>,
    pub tsb_new: Vec<Option<i32>>,
}

fn read_side_bearing(mtx: &[u8], long_metrics: usize, gid: usize) -> i32 {
    super::super::subsetter::metric_pair(mtx, long_metrics, 0, gid).1 as i32
}

pub fn apply_gvar(
    table_map:     &BTreeMap<String, TableBytes>,
    glyf_data:     &[u8],
    glyph_offsets: &[usize],
    num_glyphs:    usize,
    location:      &[f64],
    axis_count:    usize,
) -> Result<GvarResult, String> {
    let gvar = table_map.get("gvar").ok_or("missing gvar")?;

    let gvar_axis_count    = read_u16_be(gvar, 4).ok_or("gvar: header truncated")? as usize;
    let shared_tuple_count = read_u16_be(gvar, 6).ok_or("gvar: header truncated")? as usize;
    let shared_tuples_off  = read_u32_be(gvar, 8).ok_or("gvar: header truncated")? as usize;
    let flags              = read_u16_be(gvar, 14).ok_or("gvar: header truncated")?;
    let var_array_off      = read_u32_be(gvar, 16).ok_or("gvar: header truncated")? as usize;
    let use_words          = (flags & 1) != 0;
    let offsets_base       = 20usize;

    if shared_tuple_count > 0 {
        let shared_bytes = shared_tuple_count
            .checked_mul(gvar_axis_count)
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| shared_tuples_off.checked_add(n));
        if shared_bytes.is_none_or(|end| end > gvar.len()) {
            return Err("gvar: shared tuple array does not fit the table".into());
        }
    }

    let mut shared_tuples: Vec<Vec<f64>> = Vec::with_capacity(shared_tuple_count);
    for i in 0..shared_tuple_count {
        let mut coords = Vec::with_capacity(gvar_axis_count);
        for j in 0..gvar_axis_count {
            let v = read_i16_be(gvar, shared_tuples_off + (i * gvar_axis_count + j) * 2)
                .ok_or("gvar: shared tuple truncated")?;
            coords.push(v as f64 / 16384.0);
        }
        shared_tuples.push(coords);
    }

    let mut shared_tuple_scalars: Vec<Option<f64>> = vec![None; shared_tuple_count];

    let glyph_var_offset = |gid: usize| -> Option<usize> {
        if use_words {
            read_u32_be(gvar, offsets_base + gid * 4).map(|v| v as usize)
        } else {
            read_u16_be(gvar, offsets_base + gid * 2).map(|v| v as usize * 2)
        }
    };

    let hmtx_for_lsb = table_map.get("hmtx");
    let long_metrics = table_map.get("hhea")
        .and_then(|hhea| read_u16_be(hhea, 34))
        .map(|v| v as usize);
    let vmtx_for_tsb = table_map.get("vmtx");
    let long_ver_metrics = table_map.get("vhea")
        .and_then(|vhea| read_u16_be(vhea, 34))
        .map(|v| v as usize);

    let mut new_glyf: Vec<u8> = Vec::with_capacity(glyf_data.len() + glyf_data.len() / 8);
    let mut new_loca: Vec<usize>          = vec![0; num_glyphs + 1];
    let mut advance_deltas: Vec<f64>      = vec![0.0; num_glyphs];
    let mut lsb_new: Vec<Option<i32>>     = vec![None; num_glyphs];
    let mut vadvance_deltas: Vec<f64>     = vec![0.0; num_glyphs];
    let mut tsb_new: Vec<Option<i32>>     = vec![None; num_glyphs];

    let mut xr_buf:  Vec<f64> = Vec::new();
    let mut yr_buf:  Vec<f64> = Vec::new();
    let mut dx_buf:  Vec<f64> = Vec::new();
    let mut dy_buf:  Vec<f64> = Vec::new();
    let mut fdx_buf: Vec<f64> = Vec::new();
    let mut fdy_buf: Vec<f64> = Vec::new();

    let mut font_work_left = gvar.len().saturating_mul(64);

    for gid in 0..num_glyphs {
        let glyph_start  = glyph_offsets[gid];
        let glyph_end    = glyph_offsets[gid + 1];
        let var_off      = glyph_var_offset(gid).ok_or("gvar: glyph variation offset truncated")?;
        let var_off_next = glyph_var_offset(gid + 1).ok_or("gvar: glyph variation offset truncated")?;

        if var_off == var_off_next {
            let raw = glyf_data.get(glyph_start..glyph_end)
                .ok_or("gvar: glyph data range out of bounds")?;
            new_glyf.extend_from_slice(raw);
            new_glyf.resize(pad4(new_glyf.len()), 0);
            new_loca[gid + 1] = new_glyf.len();
            if new_glyf.len() > MAX_INSTANCED_GLYF_SIZE {
                return Err("gvar: instanced glyf size exceeds sanity limit".to_string());
            }
            continue;
        }

        let (n_contours, num_points) = if glyph_start == glyph_end {
            (0i16, 0usize)
        } else {
            let nc = read_i16_be(glyf_data, glyph_start).ok_or("gvar: glyph header truncated")?;
            if nc > 0 {
                let np = read_u16_be(glyf_data, glyph_start + 10 + (nc as usize - 1) * 2)
                    .ok_or("gvar: contour endpoint truncated")? as usize + 1;
                (nc, np)
            } else if nc == -1 {
                (nc, count_composite_components(glyf_data, glyph_start))
            } else if nc == 0 {
                (0, 0usize)
            } else {
                let raw = glyf_data.get(glyph_start..glyph_end)
                    .ok_or("gvar: glyph data range out of bounds")?;
                new_glyf.extend_from_slice(raw);
                new_glyf.resize(pad4(new_glyf.len()), 0);
                new_loca[gid + 1] = new_glyf.len();
                if new_glyf.len() > MAX_INSTANCED_GLYF_SIZE {
                    return Err("gvar: instanced glyf size exceeds sanity limit".to_string());
                }
                continue;
            }
        };

        let total_points = num_points + 4;
        let gvd_base = var_array_off + var_off;
        let raw_tuple_count  = read_u16_be(gvar, gvd_base).ok_or("gvar: tuple header truncated")?;
        let has_shared_pts   = (raw_tuple_count & 0x8000) != 0;
        let tup_count        = (raw_tuple_count & 0x0FFF) as usize;
        let serialized_start = gvd_base
            + read_u16_be(gvar, gvd_base + 2).ok_or("gvar: tuple header truncated")? as usize;

        let mut serialized_pos = serialized_start;
        let mut shared_points: Option<Vec<usize>> = None;
        if has_shared_pts {
            let (pts, next) = parse_packed_points(gvar, serialized_pos, total_points);
            shared_points  = pts;
            serialized_pos = next;
        }

        dx_buf.clear(); dx_buf.resize(total_points, 0.0);
        dy_buf.clear(); dy_buf.resize(total_points, 0.0);
        let dx = &mut dx_buf;
        let dy = &mut dy_buf;

        let mut header_pos       = gvd_base + 4;
        let mut private_data_pos = serialized_pos;

        let mut cached_coords: Option<GlyphCoords> = None;

        const MAX_TUPLE_POINT_WORK: usize = 16_777_216;
        let mut tuple_work_left = MAX_TUPLE_POINT_WORK;

        for _ in 0..tup_count {
            let var_data_size    = read_u16_be(gvar, header_pos)
                .ok_or("gvar: tuple var data size truncated")? as usize;
            let tuple_index_word = read_u16_be(gvar, header_pos + 2)
                .ok_or("gvar: tuple index truncated")?;
            header_pos += 4;

            let has_peak         = (tuple_index_word & 0x8000) != 0;
            let has_intermediate = (tuple_index_word & 0x4000) != 0;
            let has_private_pts  = (tuple_index_word & 0x2000) != 0;
            let shared_idx       = (tuple_index_word & 0x0FFF) as usize;

            let peak_buf: Vec<f64>;
            let peak: &[f64] = if has_peak {
                let mut p = Vec::with_capacity(gvar_axis_count);
                for _ in 0..gvar_axis_count {
                    let v = read_i16_be(gvar, header_pos).ok_or("gvar: peak tuple truncated")?;
                    p.push(v as f64 / 16384.0);
                    header_pos += 2;
                }
                peak_buf = p;
                &peak_buf
            } else {
                shared_tuples.get(shared_idx).ok_or("gvar: shared tuple index out of range")?
            };

            let (start_tuple, end_tuple) = if has_intermediate {
                let mut s = Vec::with_capacity(gvar_axis_count);
                let mut e = Vec::with_capacity(gvar_axis_count);
                for _ in 0..gvar_axis_count {
                    let v = read_i16_be(gvar, header_pos).ok_or("gvar: intermediate start truncated")?;
                    s.push(v as f64 / 16384.0); header_pos += 2;
                }
                for _ in 0..gvar_axis_count {
                    let v = read_i16_be(gvar, header_pos).ok_or("gvar: intermediate end truncated")?;
                    e.push(v as f64 / 16384.0); header_pos += 2;
                }
                (Some(s), Some(e))
            } else {
                (None, None)
            };

            let scalar = if !has_peak && !has_intermediate {
                *shared_tuple_scalars[shared_idx]
                    .get_or_insert_with(|| compute_tuple_scalar(location, peak, None, None, axis_count))
            } else {
                compute_tuple_scalar(location, peak, start_tuple.as_deref(), end_tuple.as_deref(), axis_count)
            };
            let tuple_end = private_data_pos + var_data_size;

            if scalar.abs() > 1e-10 {
                tuple_work_left = tuple_work_left
                    .checked_sub(total_points.max(1))
                    .ok_or("gvar: per-glyph tuple work budget exhausted")?;
                font_work_left = font_work_left
                    .checked_sub(total_points.max(1))
                    .ok_or("gvar: font-wide tuple work budget exhausted")?;
                let private_pts;
                let points: Option<&Vec<usize>> = if has_private_pts {
                    let (pts, next) = parse_packed_points(gvar, private_data_pos, total_points);
                    private_data_pos = next;
                    private_pts = pts;
                    private_pts.as_ref()
                } else {
                    shared_points.as_ref()
                };

                let n_delta = points.map_or(total_points, |p| p.len());
                private_data_pos = parse_packed_deltas_into(gvar, private_data_pos, n_delta, &mut xr_buf);
                parse_packed_deltas_into(gvar, private_data_pos, n_delta, &mut yr_buf);
                let (xr, yr) = (&xr_buf, &yr_buf);

                match points {
                    None => {
                        for i in 0..total_points {
                            dx[i] += scalar * xr[i];
                            dy[i] += scalar * yr[i];
                        }
                    }
                    Some(pts) => {
                        fdx_buf.clear(); fdx_buf.resize(total_points, 0.0);
                        fdy_buf.clear(); fdy_buf.resize(total_points, 0.0);
                        let fdx = &mut fdx_buf;
                        let fdy = &mut fdy_buf;
                        for (i, &p) in pts.iter().enumerate() {
                            if p < total_points {
                                fdx[p] = xr[i];
                                fdy[p] = yr[i];
                            }
                        }

                        if n_contours > 0 {
                            let cc = cached_coords.get_or_insert_with(|| extract_coords(glyf_data, glyph_start, n_contours as usize));
                            iup(fdx, fdy, pts, &cc.end_pts, num_points, &cc.x_coords, &cc.y_coords);
                        }

                        for i in 0..total_points {
                            dx[i] += scalar * fdx[i];
                            dy[i] += scalar * fdy[i];
                        }
                    }
                }
            }

            private_data_pos = tuple_end;
        }

        advance_deltas[gid] = dx[num_points + 1];
        vadvance_deltas[gid] = dy[num_points + 2] - dy[num_points + 3];

        let modified = if n_contours > 0 {
            let out = apply_simple_glyph_deltas(glyf_data, glyph_start, glyph_end, n_contours as usize, dx, dy, cached_coords.as_ref());
            if let (Some(hmtx), Some(lm)) = (hmtx_for_lsb, long_metrics)
                && let (Some(orig_xmin), Some(new_xmin)) = (
                    read_i16_be(glyf_data, glyph_start + 2),
                    read_i16_be(&out, 2),
                ) {
                    let lsb_delta = ot_round(dx[num_points]);
                    lsb_new[gid] = Some(i32::from(new_xmin) - i32::from(orig_xmin)
                        + read_side_bearing(hmtx, lm, gid) - lsb_delta);
                }
            if let (Some(vmtx), Some(lm)) = (vmtx_for_tsb, long_ver_metrics)
                && let (Some(orig_ymax), Some(new_ymax)) = (
                    read_i16_be(glyf_data, glyph_start + 8),
                    read_i16_be(&out, 8),
                ) {
                    tsb_new[gid] = Some(i32::from(orig_ymax) - i32::from(new_ymax)
                        + read_side_bearing(vmtx, lm, gid) + ot_round(dy[num_points + 2]));
                }
            out
        } else if n_contours == -1 {
            apply_composite_glyph_deltas(glyf_data, glyph_start, glyph_end, dx, dy, num_points)
        } else {
            glyf_data.get(glyph_start..glyph_end).map_or(Vec::new(), |s| s.to_vec())
        };

        new_glyf.extend_from_slice(&modified);
        new_glyf.resize(pad4(new_glyf.len()), 0);
        new_loca[gid + 1] = new_glyf.len();
        if new_glyf.len() > MAX_INSTANCED_GLYF_SIZE {
            return Err("gvar: instanced glyf size exceeds sanity limit".to_string());
        }
    }

    recompute_composite_bboxes(&mut new_glyf, &new_loca, num_glyphs);

    Ok(GvarResult { glyf_data: new_glyf, new_loca, advance_deltas, lsb_new, vadvance_deltas, tsb_new })
}

#[derive(Default)]
struct BboxPen {
    min: (f32, f32),
    max: (f32, f32),
    any: bool,
}

impl BboxPen {
    fn add(&mut self, x: f32, y: f32) {
        if !self.any {
            self.any = true;
            self.min = (x, y);
            self.max = (x, y);
            return;
        }
        self.min = (self.min.0.min(x), self.min.1.min(y));
        self.max = (self.max.0.max(x), self.max.1.max(y));
    }
}

impl crate::daecore::daetype::outline::OutlinePen for BboxPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.add(cx, cy);
        self.add(x, y);
    }
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.add(c1x, c1y);
        self.add(c2x, c2y);
        self.add(x, y);
    }
    fn close(&mut self) {}
}

fn recompute_composite_bboxes(glyf: &mut [u8], loca: &[usize], num_glyphs: usize) {
    use crate::daecore::daetype::decoder::write_i16_be;

    for gid in 0..num_glyphs {
        let (Some(&start), Some(&end)) = (loca.get(gid), loca.get(gid + 1)) else { break };
        if end.saturating_sub(start) < 10 {
            continue;
        }
        if read_i16_be(glyf, start).is_none_or(|n| n >= 0) {
            continue;
        }

        let mut pen = BboxPen::default();
        if crate::daecore::daetype::outline::outline_glyf_bytes(glyf, loca, gid as u16, &mut pen).is_err() {
            continue;
        }
        if !pen.any {
            continue;
        }
        let clamp = |v: f32| ot_round(f64::from(v)).clamp(i16::MIN.into(), i16::MAX.into()) as i16;
        write_i16_be(glyf, start + 2, clamp(pen.min.0));
        write_i16_be(glyf, start + 4, clamp(pen.min.1));
        write_i16_be(glyf, start + 6, clamp(pen.max.0));
        write_i16_be(glyf, start + 8, clamp(pen.max.1));
    }
}

pub(crate) fn compute_tuple_scalar(
    location:    &[f64],
    peak:        &[f64],
    start_tuple: Option<&[f64]>,
    end_tuple:   Option<&[f64]>,
    axis_count:  usize,
) -> f64 {
    let mut scalar = 1.0f64;
    for i in 0..axis_count.min(peak.len()) {
        let p   = peak[i];
        let loc = location[i];
        if p == 0.0 { continue; }
        if loc == p { continue; }
        let start = start_tuple.map_or(if p < 0.0 { -1.0 } else { 0.0 }, |s| s[i]);
        let end   = end_tuple.map_or(  if p > 0.0 {  1.0 } else { 0.0 }, |e| e[i]);
        if loc < start || loc > end { return 0.0; }
        scalar *= if loc < p {
            (loc - start) / (p - start)
        } else {
            (end - loc) / (end - p)
        };
    }
    scalar
}

pub(crate) fn parse_packed_points(buf: &[u8], pos: usize, total_points: usize) -> (Option<Vec<usize>>, usize) {
    let mut pos = pos;
    let mut count = match buf.get(pos) {
        Some(&b) => { pos += 1; b as usize }
        None => return (None, pos),
    };
    if count == 0 { return (None, pos); }
    if count & 0x80 != 0 {
        let next = match buf.get(pos) {
            Some(&b) => { pos += 1; b as usize }
            None => return (None, pos),
        };
        count = ((count & 0x7F) << 8) | next;
    }
    if count > total_points { count = total_points; }

    let mut points: Vec<usize> = Vec::with_capacity(count);
    let mut idx = 0usize;
    let mut steps = 0usize;
    while points.len() < count {
        if steps > total_points { break; }
        steps += 1;
        let ctrl = match buf.get(pos) {
            Some(&b) => { pos += 1; b }
            None => break,
        };
        let words = (ctrl & 0x80) != 0;
        let len   = (ctrl & 0x7F) as usize + 1;
        for _ in 0..len {
            if points.len() >= count { break; }
            if words {
                let hi = buf.get(pos).copied().unwrap_or(0) as usize;
                let lo = buf.get(pos + 1).copied().unwrap_or(0) as usize;
                pos += 2;
                idx += (hi << 8) | lo;
            } else {
                idx += buf.get(pos).copied().unwrap_or(0) as usize;
                pos += 1;
            }
            if idx < total_points { points.push(idx); }
        }
    }
    (Some(points), pos)
}

pub(crate) fn parse_packed_deltas(buf: &[u8], pos: usize, count: usize) -> (Vec<f64>, usize) {
    let mut deltas = Vec::new();
    let next = parse_packed_deltas_into(buf, pos, count, &mut deltas);
    (deltas, next)
}

pub(crate) fn parse_packed_deltas_into(buf: &[u8], pos: usize, count: usize, deltas: &mut Vec<f64>) -> usize {
    deltas.clear();
    deltas.resize(count, 0.0);
    let mut i = 0;
    let mut pos = pos;
    let mut steps = 0usize;
    'outer: while i < count {
        if steps > count { break; }
        steps += 1;
        let ctrl = match buf.get(pos) { Some(&b) => { pos += 1; b } None => break };
        let len  = (ctrl & 0x3F) as usize + 1;
        if ctrl & 0x80 != 0 {
            i += len;
        } else if ctrl & 0x40 != 0 {
            for _ in 0..len {
                if i >= count { break; }
                let hi = match buf.get(pos) { Some(&b) => { pos += 1; b as u16 } None => break 'outer };
                let lo = match buf.get(pos) { Some(&b) => { pos += 1; b as u16 } None => break 'outer };
                let raw = (hi << 8) | lo;
                deltas[i] = if raw >= 0x8000 { raw as f64 - 0x10000 as f64 } else { raw as f64 };
                i += 1;
            }
        } else {
            for _ in 0..len {
                if i >= count { break; }
                let raw = match buf.get(pos) { Some(&b) => { pos += 1; b } None => break 'outer };
                deltas[i] = if raw >= 0x80 { raw as f64 - 0x100 as f64 } else { raw as f64 };
                i += 1;
            }
        }
    }
    pos
}
