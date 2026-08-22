use alloc::string::String;
use alloc::collections::BTreeMap;
use super::pen::OutlinePen;
use super::super::decoder::{read_i16_be, read_u16_be};
use super::super::instancer::{extract_coords_into, GlyphCoords};
use crate::daecore::daetype::TableBytes;
use super::super::format::glyf::{
    ARG_1_AND_2_ARE_WORDS, ARGS_ARE_XY_VALUES, MORE_COMPONENTS, SCALED_COMPONENT_OFFSET,
    UNSCALED_COMPONENT_OFFSET, WE_HAVE_AN_X_AND_Y_SCALE, WE_HAVE_A_SCALE, WE_HAVE_A_TWO_BY_TWO,
};

pub fn outline_glyf_glyph_with_loca(
    table_map: &BTreeMap<String, TableBytes>,
    loca:      &[usize],
    gid:       u16,
    pen:       &mut dyn OutlinePen,
) -> Result<(), String> {
    let glyf = table_map.get("glyf").ok_or("glyf: missing glyf table")?;
    outline_glyf_bytes(glyf, loca, gid, pen)
}

pub fn outline_glyf_bytes(
    glyf: &[u8],
    loca: &[usize],
    gid:  u16,
    pen:  &mut dyn OutlinePen,
) -> Result<(), String> {
    let mut budget = Budget { visits: MAX_COMPONENT_VISITS, points: MAX_COMPONENT_POINTS };
    let mut scratch = GlyphCoords::default();
    draw_component(glyf, loca, gid, 0, &mut budget, &mut scratch, pen)
}

pub fn outline_glyf_glyph_reusing(
    table_map: &BTreeMap<String, TableBytes>,
    loca: &[usize],
    gid: u16,
    scratch: &mut GlyphCoords,
    pen: &mut dyn OutlinePen,
) -> Result<(), String> {
    let glyf = table_map.get("glyf").ok_or("glyf: table missing")?;
    outline_glyf_glyph_reusing_bytes(glyf, loca, gid, scratch, pen)
}

pub fn outline_glyf_glyph_reusing_bytes(
    glyf: &[u8],
    loca: &[usize],
    gid: u16,
    scratch: &mut GlyphCoords,
    pen: &mut dyn OutlinePen,
) -> Result<(), String> {
    let mut budget = Budget { visits: MAX_COMPONENT_VISITS, points: MAX_COMPONENT_POINTS };
    draw_component(glyf, loca, gid, 0, &mut budget, scratch, pen)
}

struct Budget {
    visits: u32,
    points: u32,
}

const MAX_COMPONENT_DEPTH: usize = 10;

const MAX_COMPONENT_VISITS: u32 = 65_536;

const MAX_COMPONENT_POINTS: u32 = 1_000_000;

fn draw_component(
    glyf: &[u8], loca: &[usize], gid: u16, depth: usize, budget: &mut Budget,
    scratch: &mut GlyphCoords, pen: &mut dyn OutlinePen,
) -> Result<(), String> {
    if depth > MAX_COMPONENT_DEPTH {
        return Err("glyf: composite glyph nesting too deep".into());
    }
    if budget.visits == 0 {
        return Err("glyf: composite glyph work budget exhausted".into());
    }
    budget.visits -= 1;
    let gid = gid as usize;
    if gid + 1 >= loca.len() { return Err("glyf: glyph index out of range".into()); }
    let (start, end) = (loca[gid], loca[gid + 1]);
    if end <= start { return Ok(()); }

    let n_contours = read_i16_be(glyf, start).ok_or("glyf: glyph header truncated")?;
    if n_contours >= 0 {
        draw_simple_glyph(glyf, start, n_contours as usize, budget, scratch, pen)
    } else {
        draw_composite_glyph(glyf, loca, start, end, depth, budget, scratch, pen)
    }
}

fn draw_simple_glyph(
    glyf: &[u8], start: usize, n_contours: usize, budget: &mut Budget,
    coords: &mut GlyphCoords, pen: &mut dyn OutlinePen,
) -> Result<(), String> {
    extract_coords_into(glyf, start, n_contours, coords);
    budget.points = budget
        .points
        .checked_sub(coords.num_points as u32)
        .ok_or("glyf: composite glyph point budget exhausted")?;
    let mut c_start = 0usize;
    for &c_end in &coords.end_pts {
        if c_end >= coords.num_points { break; }
        draw_contour_over(
            &GlyfContour { coords, start: c_start, end: c_end },
            pen,
        );
        c_start = c_end + 1;
    }
    Ok(())
}

pub fn draw_contour(pts: &[(f64, f64, bool)], pen: &mut dyn OutlinePen) {
    draw_contour_over(&pts, pen);
}

pub trait ContourPoints {
    fn len(&self) -> usize;
    fn get(&self, i: usize) -> (f32, f32, bool);
}

impl ContourPoints for &[(f64, f64, bool)] {
    fn len(&self) -> usize { <[_]>::len(self) }
    fn get(&self, i: usize) -> (f32, f32, bool) {
        let (x, y, on) = self[i];
        (x as f32, y as f32, on)
    }
}

struct GlyfContour<'a> {
    coords: &'a GlyphCoords,
    start: usize,
    end: usize,
}

impl ContourPoints for GlyfContour<'_> {
    fn len(&self) -> usize { self.end - self.start + 1 }
    fn get(&self, i: usize) -> (f32, f32, bool) {
        let k = self.start + i;
        (
            self.coords.x_coords[k] as f32,
            self.coords.y_coords[k] as f32,
            self.coords.flags[k] & 0x01 != 0,
        )
    }
}

pub(crate) fn draw_contour_over<P: ContourPoints + ?Sized>(pts: &P, pen: &mut dyn OutlinePen) {
    let n = pts.len();
    if n == 0 { return; }

    // TrueType starts a contour at point 0 if it is on-curve, otherwise at the last point if that
    // one is, and only failing both at the midpoint the `None` arm implies.
    let start_idx = if pts.get(0).2 {
        Some(0)
    } else if pts.get(n - 1).2 {
        Some(n - 1)
    } else {
        None
    };
    let (start_x, start_y, walk_from) = match start_idx {
        Some(i) => { let p = pts.get(i); (p.0, p.1, (i + 1) % n) }
        None => {
            let (lx, ly, _) = pts.get(n - 1);
            let (fx, fy, _) = pts.get(0);
            ((lx + fx) * 0.5, (ly + fy) * 0.5, 0)
        }
    };
    pen.move_to(start_x, start_y);

    let steps = if start_idx.is_some() { n - 1 } else { n };
    let mut pending_ctrl: Option<(f32, f32)> = None;
    let mut idx = walk_from;
    for _ in 0..steps {
        let (x, y, on) = pts.get(idx);
        idx += 1;
        if idx == n { idx = 0; }
        if on {
            match pending_ctrl.take() {
                Some((cx, cy)) => pen.quad_to(cx, cy, x, y),
                None => pen.line_to(x, y),
            }
        } else if let Some((cx, cy)) = pending_ctrl.replace((x, y)) {
            let (mx, my) = ((cx + x) * 0.5, (cy + y) * 0.5);
            pen.quad_to(cx, cy, mx, my);
        }
    }
    if let Some((cx, cy)) = pending_ctrl {
        pen.quad_to(cx, cy, start_x, start_y);
    }
    pen.close();
}

fn f2dot14(raw: i16) -> f64 { raw as f64 / 16384.0 }

#[derive(Clone, Copy)]
struct Transform { a: f64, b: f64, c: f64, d: f64, dx: f64, dy: f64 }

const IDENTITY: Transform = Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, dx: 0.0, dy: 0.0 };

#[allow(clippy::too_many_arguments, reason = "the recursion carries its budget and its scratch")]
fn draw_composite_glyph(
    glyf: &[u8], loca: &[usize], start: usize, end: usize, depth: usize, budget: &mut Budget,
    scratch: &mut GlyphCoords, pen: &mut dyn OutlinePen,
) -> Result<(), String> {
    let mut pos = start + 10;
    let limit = end.min(glyf.len());
    loop {
        if pos + 4 > limit { return Err("glyf: composite stream truncated".into()); }
        let flags    = read_u16_be(glyf, pos).ok_or("glyf: composite stream truncated")?;
        let comp_gid = read_u16_be(glyf, pos + 2).ok_or("glyf: composite stream truncated")?;
        pos += 4;

        if flags & ARGS_ARE_XY_VALUES == 0 {
            return Err("glyf: point-matching composite alignment is not supported".into());
        }
        let (dx, dy) = if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            let x = read_i16_be(glyf, pos).ok_or("glyf: composite args truncated")? as f64;
            let y = read_i16_be(glyf, pos + 2).ok_or("glyf: composite args truncated")? as f64;
            pos += 4;
            (x, y)
        } else {
            let x = *glyf.get(pos).ok_or("glyf: composite args truncated")? as i8 as f64;
            let y = *glyf.get(pos + 1).ok_or("glyf: composite args truncated")? as i8 as f64;
            pos += 2;
            (x, y)
        };

        let (a, b, c, d) = if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            let a = f2dot14(read_i16_be(glyf, pos).ok_or("glyf: composite transform truncated")?);
            let b = f2dot14(read_i16_be(glyf, pos + 2).ok_or("glyf: composite transform truncated")?);
            let c = f2dot14(read_i16_be(glyf, pos + 4).ok_or("glyf: composite transform truncated")?);
            let d = f2dot14(read_i16_be(glyf, pos + 6).ok_or("glyf: composite transform truncated")?);
            pos += 8;
            (a, b, c, d)
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            let a = f2dot14(read_i16_be(glyf, pos).ok_or("glyf: composite transform truncated")?);
            let d = f2dot14(read_i16_be(glyf, pos + 2).ok_or("glyf: composite transform truncated")?);
            pos += 4;
            (a, 0.0, 0.0, d)
        } else if flags & WE_HAVE_A_SCALE != 0 {
            let s = f2dot14(read_i16_be(glyf, pos).ok_or("glyf: composite transform truncated")?);
            pos += 2;
            (s, 0.0, 0.0, s)
        } else {
            (IDENTITY.a, IDENTITY.b, IDENTITY.c, IDENTITY.d)
        };

        let (tdx, tdy) = if flags & SCALED_COMPONENT_OFFSET != 0 && flags & UNSCALED_COMPONENT_OFFSET == 0 {
            (dx * a + dy * c, dx * b + dy * d)
        } else {
            (dx, dy)
        };
        let t = Transform { a, b, c, d, dx: tdx, dy: tdy };

        let mut tp = super::pen::TransformPen::new(pen, [t.a, t.b, t.c, t.d, t.dx, t.dy]);
        draw_component(glyf, loca, comp_gid, depth + 1, budget, scratch, &mut tp)?;

        if flags & MORE_COMPONENTS == 0 { break; }
    }
    Ok(())
}
