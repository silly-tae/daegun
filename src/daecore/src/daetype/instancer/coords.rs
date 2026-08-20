use alloc::vec::Vec;
use super::super::decoder::{read_u16_be, read_i16_be, write_i16_be};
use super::super::format::round::ot_round;
use super::super::format::glyf::{
    ARGS_ARE_XY_VALUES, ARG_1_AND_2_ARE_WORDS, MORE_COMPONENTS, WE_HAVE_AN_X_AND_Y_SCALE,
    WE_HAVE_A_SCALE, WE_HAVE_A_TWO_BY_TWO,
};

#[derive(Default)]
pub struct GlyphCoords {
    pub x_coords:   Vec<i32>,
    pub y_coords:   Vec<i32>,
    pub flags:      Vec<u8>,
    pub end_pts:    Vec<usize>,
    pub num_points: usize,
}

pub fn extract_coords(data: &[u8], start: usize, n_contours: usize) -> GlyphCoords {
    let mut out = GlyphCoords::default();
    extract_coords_into(data, start, n_contours, &mut out);
    out
}

impl GlyphCoords {
    fn reset(&mut self) {
        self.x_coords.clear();
        self.y_coords.clear();
        self.flags.clear();
        self.end_pts.clear();
        self.num_points = 0;
    }
}

pub fn extract_coords_into(data: &[u8], start: usize, n_contours: usize, out: &mut GlyphCoords) {
    out.reset();
    let empty = |out: &mut GlyphCoords| out.reset();
    out.end_pts.reserve(n_contours);
    for i in 0..n_contours {
        let off = start + 10 + i * 2;
        let v = match read_u16_be(data, off) { Some(v) => v, None => return empty(out) };
        // `iup` indexes by these, so a non-monotonic sequence would panic rather than skip the glyph.
        if let Some(&prev) = out.end_pts.last()
            && v as usize <= prev { return empty(out); }
        out.end_pts.push(v as usize);
    }
    let num_points = out.end_pts.last().map_or(0, |&e| e + 1);

    let instr_len_off = start + 10 + n_contours * 2;
    let instr_len = match read_u16_be(data, instr_len_off) { Some(v) => v as usize, None => return empty(out) };
    let mut pos = instr_len_off + 2 + instr_len;

    out.flags.reserve(num_points);
    while out.flags.len() < num_points {
        let f = match data.get(pos) { Some(&b) => { pos += 1; b } None => break };
        out.flags.push(f);
        if f & 0x08 != 0 {
            let r = match data.get(pos) { Some(&b) => { pos += 1; b as usize } None => break };
            let take = r.min(num_points - out.flags.len());
            out.flags.resize(out.flags.len() + take, f);
        }
    }
    if out.flags.len() < num_points { return empty(out); }

    out.x_coords.resize(num_points, 0);
    {
        let flags = &out.flags[..num_points];
        let xs = &mut out.x_coords[..num_points];
        let mut cur = 0i32;
        for (x, &f) in xs.iter_mut().zip(flags) {
            if f & 0x02 != 0 {
                let v = match data.get(pos) { Some(&b) => { pos += 1; b as i32 } None => break };
                cur = cur.saturating_add(if f & 0x10 != 0 { v } else { v.saturating_neg() });
            } else if f & 0x10 == 0 {
                let v = match read_i16_be(data, pos) { Some(v) => v, None => break };
                cur = cur.saturating_add(v as i32); pos += 2;
            }
            *x = cur;
        }
    }

    out.y_coords.resize(num_points, 0);
    {
        let flags = &out.flags[..num_points];
        let ys = &mut out.y_coords[..num_points];
        let mut cur = 0i32;
        for (y, &f) in ys.iter_mut().zip(flags) {
            if f & 0x04 != 0 {
                let v = match data.get(pos) { Some(&b) => { pos += 1; b as i32 } None => break };
                cur = cur.saturating_add(if f & 0x20 != 0 { v } else { v.saturating_neg() });
            } else if f & 0x20 == 0 {
                let v = match read_i16_be(data, pos) { Some(v) => v, None => break };
                cur = cur.saturating_add(v as i32); pos += 2;
            }
            *y = cur;
        }
    }

    out.num_points = num_points;
}

pub fn iup(
    dx: &mut [f64], dy: &mut [f64],
    touched_points: &[usize],
    end_pts: &[usize],
    num_points: usize,
    orig_x: &[i32], orig_y: &[i32],
) {
    let touched: Vec<bool> = {
        let mut t = vec![false; num_points];
        for &p in touched_points { if p < num_points { t[p] = true; } }
        t
    };

    let mut start = 0;
    for &end in end_pts {
        iup_axis(dx, &touched, orig_x, start, end);
        iup_axis(dy, &touched, orig_y, start, end);
        start = end + 1;
    }
}

fn iup_axis(delta: &mut [f64], touched: &[bool], coords: &[i32], start: usize, end: usize) {
    if end < start { return; }
    let n = end - start + 1;

    let first = match (start..=end).find(|&i| touched[i]) { Some(f) => f, None => return };

    let mut touched_pos: Vec<usize> = Vec::new();
    for step in 0..n {
        let i = (first - start + step) % n + start;
        if touched[i] { touched_pos.push(i); }
    }

    for t in 0..touched_pos.len() {
        let prev_idx   = touched_pos[t];
        let next_idx   = touched_pos[(t + 1) % touched_pos.len()];
        let prev_delta = delta[prev_idx];
        let next_delta = delta[next_idx];
        let prev_coord = coords[prev_idx] as f64;
        let next_coord = coords[next_idx] as f64;

        let mut cur = (prev_idx - start + 1) % n + start;
        let mut steps = 0usize;
        while cur != next_idx {
            if steps >= n { break; }
            steps += 1;
            let cur_coord = coords[cur] as f64;
            delta[cur] = if prev_coord == next_coord {
                if prev_delta == next_delta { prev_delta } else { 0.0 }
            } else if cur_coord <= prev_coord.min(next_coord) {
                if prev_coord < next_coord { prev_delta } else { next_delta }
            } else if cur_coord >= prev_coord.max(next_coord) {
                if prev_coord > next_coord { prev_delta } else { next_delta }
            } else {
                prev_delta + (next_delta - prev_delta) * (cur_coord - prev_coord) / (next_coord - prev_coord)
            };
            cur = (cur - start + 1) % n + start;
        }
    }
}

pub fn apply_simple_glyph_deltas(
    data: &[u8], start: usize, end: usize,
    n_contours: usize,
    dx: &[f64], dy: &[f64],
    pre: Option<&GlyphCoords>,
) -> Vec<u8> {
    let instr_len_off = start + 10 + n_contours * 2;
    let instruction_len = match read_u16_be(data, instr_len_off) {
        Some(v) => v as usize,
        None => {
            let s = start.min(data.len());
            let e = end.min(data.len()).max(s);
            return data[s..e].to_vec();
        }
    };

    let (num_points, flags, mut x_coords, mut y_coords) = if let Some(cc) = pre {
        (cc.num_points, cc.flags.clone(), cc.x_coords.clone(), cc.y_coords.clone())
    } else {
        let cc = extract_coords(data, start, n_contours);
        (cc.num_points, cc.flags, cc.x_coords, cc.y_coords)
    };

    for i in 0..num_points {
        x_coords[i] = x_coords[i].saturating_add(ot_round(dx[i]));
        y_coords[i] = y_coords[i].saturating_add(ot_round(dy[i]));
    }

    let (x_min, x_max, y_min, y_max) = if num_points > 0 {
        let mut xmn = x_coords[0]; let mut xmx = x_coords[0];
        let mut ymn = y_coords[0]; let mut ymx = y_coords[0];
        for i in 1..num_points {
            if x_coords[i] < xmn { xmn = x_coords[i]; }
            if x_coords[i] > xmx { xmx = x_coords[i]; }
            if y_coords[i] < ymn { ymn = y_coords[i]; }
            if y_coords[i] > ymx { ymx = y_coords[i]; }
        }
        (xmn, xmx, ymn, ymx)
    } else { (0, 0, 0, 0) };

    let mut ttf_flags = vec![0u8; num_points];
    let mut x_deltas  = vec![0i16; num_points];
    let mut y_deltas  = vec![0i16; num_points];
    let mut prev_x = 0i32; let mut prev_y = 0i32;
    for i in 0..num_points {
        let dxp = x_coords[i].wrapping_sub(prev_x) as i16;
        let dyp = y_coords[i].wrapping_sub(prev_y) as i16;
        prev_x = x_coords[i]; prev_y = y_coords[i];
        x_deltas[i] = dxp; y_deltas[i] = dyp;

        let mut f = flags[i] & 0xC1;
        if dxp == 0 { f |= 0x10; }
        else if (-255..=255).contains(&dxp) { f |= 0x02; if dxp > 0 { f |= 0x10; } }
        if dyp == 0 { f |= 0x20; }
        else if (-255..=255).contains(&dyp) { f |= 0x04; if dyp > 0 { f |= 0x20; } }
        ttf_flags[i] = f;
    }

    let mut encoded_flags: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut outer_guard = 0usize;
    while i < num_points {
        outer_guard += 1;
        if outer_guard > num_points { break; }
        encoded_flags.push(ttf_flags[i]);
        let mut rep = 0usize;
        let mut rep_guard = 0usize;
        while i + 1 + rep < num_points && rep < 255 && rep_guard < 255 && ttf_flags[i + 1 + rep] == ttf_flags[i] { rep += 1; rep_guard += 1; }
        if rep > 0 {
            #[allow(clippy::unwrap_used, reason = "encoded_flags was just pushed to unconditionally above")]
            { *encoded_flags.last_mut().unwrap() |= 0x08; }
            encoded_flags.push(rep as u8);
            i += 1 + rep;
        } else { i += 1; }
    }

    let mut x_bytes: Vec<u8> = Vec::new();
    for i in 0..num_points {
        let f = ttf_flags[i];
        if f & 0x02 != 0 { x_bytes.push(x_deltas[i].unsigned_abs() as u8); }
        else if f & 0x10 == 0 { x_bytes.push((x_deltas[i] >> 8) as u8); x_bytes.push(x_deltas[i] as u8); }
    }

    let mut y_bytes: Vec<u8> = Vec::new();
    for i in 0..num_points {
        let f = ttf_flags[i];
        if f & 0x04 != 0 { y_bytes.push(y_deltas[i].unsigned_abs() as u8); }
        else if f & 0x20 == 0 { y_bytes.push((y_deltas[i] >> 8) as u8); y_bytes.push(y_deltas[i] as u8); }
    }

    let header_size = instr_len_off + 2 + instruction_len - start;
    let header_bytes = match data.get(start..start + header_size) {
        Some(b) => b,
        None => {
            let s = start.min(data.len());
            let e = end.min(data.len()).max(s);
            return data[s..e].to_vec();
        }
    };
    let mut out = Vec::with_capacity(header_size + encoded_flags.len() + x_bytes.len() + y_bytes.len());
    out.extend_from_slice(header_bytes);
    write_i16_be(&mut out, 2, x_min as i16);
    write_i16_be(&mut out, 4, y_min as i16);
    write_i16_be(&mut out, 6, x_max as i16);
    write_i16_be(&mut out, 8, y_max as i16);
    out.extend_from_slice(&encoded_flags);
    out.extend_from_slice(&x_bytes);
    out.extend_from_slice(&y_bytes);
    out
}

pub fn count_composite_components(data: &[u8], start: usize) -> usize {
    let mut pos = start + 10;
    let mut count = 0;
    while count <= data.len() {
        let f = match read_u16_be(data, pos) { Some(v) => v, None => break };
        pos += 4; count += 1;
        pos += if f & ARG_1_AND_2_ARE_WORDS != 0 { 4 } else { 2 };
        if      f & WE_HAVE_A_TWO_BY_TWO != 0 { pos += 8; }
        else if f & WE_HAVE_AN_X_AND_Y_SCALE     != 0 { pos += 4; }
        else if f & WE_HAVE_A_SCALE  != 0 { pos += 2; }
        if f & MORE_COMPONENTS == 0 { break; }
    }
    count
}

pub fn apply_composite_glyph_deltas(
    data: &[u8], start: usize, end: usize,
    dx: &[f64], dy: &[f64], num_components: usize,
) -> Vec<u8> {
    let src = match data.get(start..end) {
        Some(s) => s,
        None => return Vec::new(),
    };
    if src.len() < 10 { return src.to_vec(); }

    let mut out = src[..10].to_vec();
    let mut pos = 10usize;
    let mut comp = 0usize;
    while comp < num_components {
        let f = match read_u16_be(src, pos) { Some(v) => v, None => break };
        let glyph_idx = match read_u16_be(src, pos + 2) { Some(v) => v, None => break };
        pos += 4;

        let words_in = f & ARG_1_AND_2_ARE_WORDS != 0;
        let (a0, a1) = if words_in {
            let x = match read_i16_be(src, pos)     { Some(v) => v as i32, None => break };
            let y = match read_i16_be(src, pos + 2) { Some(v) => v as i32, None => break };
            pos += 4;
            (x, y)
        } else {
            let x = match src.get(pos)     { Some(&b) => b as i8 as i32, None => break };
            let y = match src.get(pos + 1) { Some(&b) => b as i8 as i32, None => break };
            pos += 2;
            (x, y)
        };

        let (na0, na1) = if f & ARGS_ARE_XY_VALUES != 0 {
            (a0.saturating_add(ot_round(dx.get(comp).copied().unwrap_or(0.0))),
             a1.saturating_add(ot_round(dy.get(comp).copied().unwrap_or(0.0))))
        } else {
            (a0, a1)
        };

        let words_out = words_in || !(-128..=127).contains(&na0) || !(-128..=127).contains(&na1);
        let nf = if words_out { f | ARG_1_AND_2_ARE_WORDS } else { f };
        out.extend_from_slice(&nf.to_be_bytes());
        out.extend_from_slice(&glyph_idx.to_be_bytes());
        if words_out {
            out.extend_from_slice(&(na0.clamp(-32768, 32767) as i16).to_be_bytes());
            out.extend_from_slice(&(na1.clamp(-32768, 32767) as i16).to_be_bytes());
        } else {
            out.push(na0 as i8 as u8);
            out.push(na1 as i8 as u8);
        }

        let t_len = if f & WE_HAVE_A_TWO_BY_TWO != 0 { 8 } else if f & WE_HAVE_AN_X_AND_Y_SCALE != 0 { 4 } else if f & WE_HAVE_A_SCALE != 0 { 2 } else { 0 };
        match src.get(pos..pos + t_len) {
            Some(t) => out.extend_from_slice(t),
            None => break,
        }
        pos += t_len;
        comp += 1;
        if f & MORE_COMPONENTS == 0 { break; }
    }
    if pos < src.len() { out.extend_from_slice(&src[pos..]); }
    out
}
