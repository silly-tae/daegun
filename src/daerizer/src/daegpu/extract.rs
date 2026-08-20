use alloc::vec::Vec;
use crate::daecore::daetype::outline::OutlinePen;

pub type Quad = [[f32; 2]; 3];

const CUBIC_TOLERANCE: f32 = 1.0 / 4096.0;

const MAX_CUBIC_DEPTH: u8 = 8;

pub const MAX_CURVES_PER_GLYPH: usize = 16_384;

// Tuned on Inter, whose mean glyph is 30.3 curves. A serif face is not Inter – EB Garamond means
// 70.7 and 97% of its glyphs realloc past this anyway. Do not re-tune it on one face.
const CURVES_RESERVE: usize = 32;

pub struct Collector {
    curves: Vec<Quad>,
    start: [f32; 2],
    cur: [f32; 2],
    scale: f32,
    rejected: Option<Reject>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reject {
    TooComplex,
    NonFinite,
}

impl Collector {
    pub(crate) fn new(units_per_em: f32) -> Collector {
        Collector {
            curves: Vec::with_capacity(CURVES_RESERVE),
            start: [0.0; 2],
            cur: [0.0; 2],
            scale: if units_per_em.is_finite() && units_per_em > 0.0 { 1.0 / units_per_em } else { 0.0 },
            rejected: (!(units_per_em.is_finite() && units_per_em > 0.0)).then_some(Reject::NonFinite),
        }
    }

    fn point(&mut self, x: f32, y: f32) -> [f32; 2] {
        let p = [x * self.scale, y * self.scale];
        if !p[0].is_finite() || !p[1].is_finite() {
            self.rejected.get_or_insert(Reject::NonFinite);
        }
        p
    }

    fn push(&mut self, quad: Quad) {
        if self.curves.len() >= MAX_CURVES_PER_GLYPH {
            self.rejected.get_or_insert(Reject::TooComplex);
            return;
        }
        self.curves.push(quad);
    }

    fn line(&mut self, to: [f32; 2]) {
        let mid = [(self.cur[0] + to[0]) * 0.5, (self.cur[1] + to[1]) * 0.5];
        self.push([self.cur, mid, to]);
        self.cur = to;
    }

    pub fn finish(mut self) -> Result<Vec<Quad>, Option<Reject>> {
        // Both pens in this crate always close their last contour, so this does nothing today. It
        // stays because an open contour is a gap the winding rule reads straight through, turning
        // a glyph inside out – too quiet a failure to rest on a caller's good manners.
        self.close();

        if let Some(why) = self.rejected {
            return Err(Some(why));
        }
        if self.curves.is_empty() {
            return Err(None);
        }
        Ok(self.curves)
    }
}

fn signed_area(curves: &[Quad]) -> f32 {
    let mut total = 0.0;
    for c in curves {
        let ([x0, y0], [x1, y1], [x2, y2]) = (c[0], c[1], c[2]);
        total += 2.0 * ((x0 * y1 - x1 * y0) + (x1 * y2 - x2 * y1)) + (x0 * y2 - x2 * y0);
    }
    total
}

pub(crate) fn normalize_winding(curves: &mut [Quad]) {
    if signed_area(curves) > 0.0 {
        for c in curves {
            c.swap(0, 2);
        }
    }
}

fn cubic_at(p: &[[f32; 2]; 4], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [
        a * p[0][0] + b * p[1][0] + c * p[2][0] + d * p[3][0],
        a * p[0][1] + b * p[1][1] + c * p[2][1] + d * p[3][1],
    ]
}

fn quad_at(q: &Quad, t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let (a, b, c) = (u * u, 2.0 * u * t, t * t);
    [
        a * q[0][0] + b * q[1][0] + c * q[2][0],
        a * q[0][1] + b * q[1][1] + c * q[2][1],
    ]
}

fn quad_for(p: &[[f32; 2]; 4]) -> Quad {
    let ctrl = [
        (3.0 * p[1][0] - p[0][0] + 3.0 * p[2][0] - p[3][0]) * 0.25,
        (3.0 * p[1][1] - p[0][1] + 3.0 * p[2][1] - p[3][1]) * 0.25,
    ];
    [p[0], ctrl, p[3]]
}

fn deviation(p: &[[f32; 2]; 4], q: &Quad) -> f32 {
    let mut worst: f32 = 0.0;
    for &t in &[0.25, 0.5, 0.75] {
        let a = cubic_at(p, t);
        let b = quad_at(q, t);
        let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
        worst = worst.max(dx * dx + dy * dy);
    }
    worst
}

fn split_cubic(p: &[[f32; 2]; 4]) -> ([[f32; 2]; 4], [[f32; 2]; 4]) {
    let mid = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let ab = mid(p[0], p[1]);
    let bc = mid(p[1], p[2]);
    let cd = mid(p[2], p[3]);
    let abc = mid(ab, bc);
    let bcd = mid(bc, cd);
    let abcd = mid(abc, bcd);
    ([p[0], ab, abc, abcd], [abcd, bcd, cd, p[3]])
}

impl OutlinePen for Collector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        let p = self.point(x, y);
        self.cur = p;
        self.start = p;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.line(p);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let c = self.point(cx, cy);
        let to = self.point(x, y);
        self.push([self.cur, c, to]);
        self.cur = to;
    }

    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let c1 = self.point(c1x, c1y);
        let c2 = self.point(c2x, c2y);
        let to = self.point(x, y);
        let cubic = [self.cur, c1, c2, to];

        let limit = CUBIC_TOLERANCE * CUBIC_TOLERANCE;
        let mut stack = [([[0.0f32; 2]; 4], 0u8); MAX_CUBIC_DEPTH as usize + 2];
        stack[0] = (cubic, 0);
        let mut top = 1usize;

        while top > 0 {
            top -= 1;
            let (seg, depth) = stack[top];
            let q = quad_for(&seg);

            if depth >= MAX_CUBIC_DEPTH || deviation(&seg, &q) <= limit || top + 2 > stack.len() {
                self.push(q);
            } else {
                let (lo, hi) = split_cubic(&seg);
                stack[top] = (hi, depth + 1);
                stack[top + 1] = (lo, depth + 1);
                top += 2;
            }

            if self.rejected.is_some() {
                return;
            }
        }
        self.cur = to;
    }

    fn close(&mut self) {
        if self.cur != self.start {
            let s = self.start;
            self.line(s);
        }
        self.cur = self.start;
    }
}
