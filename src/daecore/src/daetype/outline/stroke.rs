use alloc::vec::Vec;
use core::f32::consts::PI;

#[allow(unused_imports, reason = "the inherent method shadows this whenever std is linked")]
use crate::daecore::daemachine::float::FloatExt;
use crate::daecore::daemachine::float::{atan2, sin_cos};

use super::{OutlinePen, Path, Verb};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Join {
    Miter { limit: f32 },
    Round,
    Bevel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug)]
pub struct StrokeStyle {
    pub width: f32,
    pub join: Join,
    pub cap: Cap,
}

impl Default for StrokeStyle {
    fn default() -> StrokeStyle {
        StrokeStyle { width: 1.0, join: Join::Miter { limit: 4.0 }, cap: Cap::Butt }
    }
}

type Pt = (f32, f32);

pub fn stroke_simplified(path: &Path, style: &StrokeStyle, tolerance: f32, pen: &mut dyn OutlinePen) {
    let mut collect = Collect::default();
    stroke(path, style, tolerance, &mut collect);
    for c in super::simplify::union(&collect.contours) {
        if c.len() < 3 {
            continue;
        }
        pen.move_to(c[0].0, c[0].1);
        for p in &c[1..] {
            pen.line_to(p.0, p.1);
        }
        pen.close();
    }
}

#[derive(Default)]
struct Collect {
    contours: Vec<Vec<Pt>>,
}

impl OutlinePen for Collect {
    fn move_to(&mut self, x: f32, y: f32) {
        self.contours.push(alloc::vec![(x, y)]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        if let Some(c) = self.contours.last_mut() {
            c.push((x, y));
        }
    }
    fn quad_to(&mut self, _: f32, _: f32, x: f32, y: f32) {
        self.line_to(x, y);
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, x: f32, y: f32) {
        self.line_to(x, y);
    }
    fn close(&mut self) {}
}

pub fn stroke(path: &Path, style: &StrokeStyle, tolerance: f32, pen: &mut dyn OutlinePen) {
    let r = style.width.abs() * 0.5;
    if r <= 0.0 || !r.is_finite() {
        return;
    }
    let tolerance = if tolerance > 0.0 { tolerance } else { 0.1 };

    for contour in flatten(path, tolerance) {
        let (pts, closed) = contour;
        let pts = dedup(&pts);
        if pts.len() < 2 {
            if pts.len() == 1 && !closed {
                dot(pts[0], r, style, tolerance, pen);
            }
            continue;
        }
        if closed {
            emit_side(&pts, r, style, tolerance, true, pen);
            let mut back = pts.clone();
            back.reverse();
            emit_side(&back, r, style, tolerance, true, pen);
        } else {
            emit_open(&pts, r, style, tolerance, pen);
        }
    }
}

fn emit_side(
    pts: &[Pt],
    r: f32,
    style: &StrokeStyle,
    tolerance: f32,
    closed: bool,
    pen: &mut dyn OutlinePen,
) {
    let n = pts.len();
    let mut out: Vec<Pt> = Vec::with_capacity(n * 2);
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        let n_in = normal(prev, cur);
        let n_out = normal(cur, next);
        if i == 0 && !closed {
            out.push(offset(cur, n_out, r));
            continue;
        }
        push_join(&mut out, prev, cur, next, n_in, n_out, r, style, tolerance);
    }
    emit_loop(&out, pen);
}

fn emit_open(pts: &[Pt], r: f32, style: &StrokeStyle, tolerance: f32, pen: &mut dyn OutlinePen) {
    let n = pts.len();
    let mut out: Vec<Pt> = Vec::with_capacity(n * 4);

    out.push(offset(pts[0], normal(pts[0], pts[1]), r));
    for i in 1..n - 1 {
        push_join(&mut out, pts[i - 1], pts[i], pts[i + 1], normal(pts[i - 1], pts[i]), normal(pts[i], pts[i + 1]), r, style, tolerance);
    }
    let end_n = normal(pts[n - 2], pts[n - 1]);
    out.push(offset(pts[n - 1], end_n, r));
    push_cap(&mut out, pts[n - 1], end_n, direction(pts[n - 2], pts[n - 1]), r, style, tolerance);

    out.push(offset(pts[n - 1], neg(end_n), r));
    for i in (1..n - 1).rev() {
        push_join(&mut out, pts[i + 1], pts[i], pts[i - 1], neg(normal(pts[i], pts[i + 1])), neg(normal(pts[i - 1], pts[i])), r, style, tolerance);
    }
    let start_n = normal(pts[0], pts[1]);
    out.push(offset(pts[0], neg(start_n), r));
    push_cap(&mut out, pts[0], neg(start_n), direction(pts[1], pts[0]), r, style, tolerance);

    emit_loop(&out, pen);
}

#[allow(clippy::too_many_arguments, reason = "a corner is its two neighbors as well as itself")]
fn push_join(
    out: &mut Vec<Pt>,
    prev: Pt,
    at: Pt,
    next: Pt,
    n_in: Pt,
    n_out: Pt,
    r: f32,
    style: &StrokeStyle,
    tolerance: f32,
) {
    let a = offset(at, n_in, r);
    let b = offset(at, n_out, r);
    let turn = n_in.0 * n_out.1 - n_in.1 * n_out.0;
    if turn.abs() < 1e-6 {
        out.push(b);
        return;
    }
    if turn > 0.0 {
        match segment_intersection(offset(prev, n_in, r), a, b, offset(next, n_out, r)) {
            Some(p) => out.push(p),
            None => {
                out.push(a);
                out.push(b);
            }
        }
        return;
    }
    out.push(a);
    match style.join {
        Join::Bevel => {}
        Join::Round => arc(out, at, a, b, r, tolerance),
        Join::Miter { limit } => {
            let (sx, sy) = (n_in.0 + n_out.0, n_in.1 + n_out.1);
            let cos_half = ((sx * sx + sy * sy) * 0.25).sqrt();
            if cos_half > 1e-6 && 1.0 / cos_half <= limit.max(1.0) {
                let m = (n_in.0 + n_out.0, n_in.1 + n_out.1);
                let len = (m.0 * m.0 + m.1 * m.1).sqrt();
                if len > 1e-6 {
                    let scale = r / cos_half / len;
                    out.push((at.0 + m.0 * scale, at.1 + m.1 * scale));
                }
            }
        }
    }
    out.push(b);
}

fn push_cap(out: &mut Vec<Pt>, at: Pt, n: Pt, dir: Pt, r: f32, style: &StrokeStyle, tolerance: f32) {
    match style.cap {
        Cap::Butt => {}
        Cap::Square => {
            out.push((at.0 + (n.0 + dir.0) * r, at.1 + (n.1 + dir.1) * r));
            out.push((at.0 + (dir.0 - n.0) * r, at.1 + (dir.1 - n.1) * r));
        }
        Cap::Round => {
            let tip = offset(at, dir, r);
            arc(out, at, offset(at, n, r), tip, r, tolerance);
            arc(out, at, tip, offset(at, neg(n), r), r, tolerance);
        }
    }
}

fn dot(at: Pt, r: f32, style: &StrokeStyle, tolerance: f32, pen: &mut dyn OutlinePen) {
    let mut out = Vec::new();
    match style.cap {
        Cap::Butt => return,
        Cap::Round => {
            let q = [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)];
            for k in 0..4 {
                let from = (at.0 + q[k].0, at.1 + q[k].1);
                let to = (at.0 + q[(k + 1) % 4].0, at.1 + q[(k + 1) % 4].1);
                arc(&mut out, at, from, to, r, tolerance);
            }
        }
        Cap::Square => out.extend_from_slice(&[
            (at.0 - r, at.1 - r),
            (at.0 + r, at.1 - r),
            (at.0 + r, at.1 + r),
            (at.0 - r, at.1 + r),
        ]),
    }
    emit_loop(&out, pen);
}

fn arc(out: &mut Vec<Pt>, center: Pt, a: Pt, b: Pt, r: f32, tolerance: f32) {
    let a0 = atan2((a.1 - center.1) as f64, (a.0 - center.0) as f64) as f32;
    let a1 = atan2((b.1 - center.1) as f64, (b.0 - center.0) as f64) as f32;
    let mut sweep = a1 - a0;
    while sweep > PI {
        sweep -= 2.0 * PI;
    }
    while sweep < -PI {
        sweep += 2.0 * PI;
    }
    let max_step = (8.0 * tolerance / r).sqrt();
    let steps = if max_step > 1e-4 { (sweep.abs() / max_step).ceil() as usize } else { 1 };
    let steps = steps.clamp(1, 256);
    for k in 1..steps {
        let t = a0 + sweep * (k as f32 / steps as f32);
        let (sin, cos) = sin_cos(t as f64);
        out.push((center.0 + r * cos as f32, center.1 + r * sin as f32));
    }
    out.push(b);
}

fn emit_loop(pts: &[Pt], pen: &mut dyn OutlinePen) {
    let pts = dedup(pts);
    if pts.len() < 3 {
        return;
    }
    pen.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pen.line_to(p.0, p.1);
    }
    pen.close();
}

fn dedup(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last().is_none_or(|&q| (p.0 - q.0).abs() > 1e-6 || (p.1 - q.1).abs() > 1e-6) {
            out.push(p);
        }
    }
    if out.len() > 1
        && let (Some(&f), Some(&l)) = (out.first(), out.last())
        && (f.0 - l.0).abs() <= 1e-6
        && (f.1 - l.1).abs() <= 1e-6
    {
        out.pop();
    }
    out
}

fn segment_intersection(a: Pt, b: Pt, c: Pt, d: Pt) -> Option<Pt> {
    let (r1, r2) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    let denom = r1.0 * r2.1 - r1.1 * r2.0;
    if denom.abs() < 1e-9 {
        return None;
    }
    let (dx, dy) = (c.0 - a.0, c.1 - a.1);
    let t = (dx * r2.1 - dy * r2.0) / denom;
    let u = (dx * r1.1 - dy * r1.0) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u))
        .then_some((a.0 + r1.0 * t, a.1 + r1.1 * t))
}

fn direction(a: Pt, b: Pt) -> Pt {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len > 1e-9 { (dx / len, dy / len) } else { (1.0, 0.0) }
}

fn normal(a: Pt, b: Pt) -> Pt {
    let d = direction(a, b);
    (-d.1, d.0)
}

fn neg(p: Pt) -> Pt {
    (-p.0, -p.1)
}

fn offset(at: Pt, n: Pt, r: f32) -> Pt {
    (at.0 + n.0 * r, at.1 + n.1 * r)
}

fn flatten(path: &Path, tolerance: f32) -> Vec<(Vec<Pt>, bool)> {
    let (verbs, points) = path.parts();
    let mut out: Vec<(Vec<Pt>, bool)> = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    let mut at: Pt = (0.0, 0.0);
    let mut i = 0usize;

    for &v in verbs {
        match v {
            Verb::Move => {
                if !cur.is_empty() {
                    out.push((core::mem::take(&mut cur), false));
                }
                at = points[i];
                cur.push(at);
                i += 1;
            }
            Verb::Line => {
                at = points[i];
                cur.push(at);
                i += 1;
            }
            Verb::Quad => {
                let (c, e) = (points[i], points[i + 1]);
                let n = steps(dist(at, c) + dist(c, e), tolerance);
                for k in 1..=n {
                    let t = k as f32 / n as f32;
                    let u = 1.0 - t;
                    cur.push((
                        u * u * at.0 + 2.0 * u * t * c.0 + t * t * e.0,
                        u * u * at.1 + 2.0 * u * t * c.1 + t * t * e.1,
                    ));
                }
                at = e;
                i += 2;
            }
            Verb::Cubic => {
                let (c1, c2, e) = (points[i], points[i + 1], points[i + 2]);
                let n = steps(dist(at, c1) + dist(c1, c2) + dist(c2, e), tolerance);
                for k in 1..=n {
                    let t = k as f32 / n as f32;
                    let u = 1.0 - t;
                    cur.push((
                        u * u * u * at.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * e.0,
                        u * u * u * at.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * e.1,
                    ));
                }
                at = e;
                i += 3;
            }
            Verb::Close => {
                if !cur.is_empty() {
                    at = cur[0];
                    out.push((core::mem::take(&mut cur), true));
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push((cur, false));
    }
    out
}

fn dist(a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    (dx * dx + dy * dy).sqrt()
}

fn steps(len: f32, tolerance: f32) -> usize {
    ((len / tolerance.max(1e-4)).sqrt().ceil() as usize).clamp(1, 256)
}
