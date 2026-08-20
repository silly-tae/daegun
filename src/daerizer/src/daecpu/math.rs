use alloc::vec::Vec;
use super::platform::{self, abs, f32x4};
use crate::daecore::daetype::outline::OutlinePen;

#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct OutlineBounds {
    pub xmin: f32,
    pub ymin: f32,
    pub width: f32,
    pub height: f32,
}

impl OutlineBounds {
    #[inline(always)]
    pub fn scale(&self, scale: f32) -> OutlineBounds {
        OutlineBounds {
            xmin: self.xmin * scale,
            ymin: self.ymin * scale,
            width: self.width * scale,
            height: self.height * scale,
        }
    }
}

#[derive(Default)]
pub struct Glyph {
    pub v_segments: Vec<[Point; 2]>,
    pub m_segments: Vec<[Point; 2]>,
    pub bounds: OutlineBounds,
    // Flattened contours, kept only so `Geometry` can borrow the allocations back. `push` drops
    // horizontal segments, so the contours cannot be rebuilt from the two segment lists.
    pub contour_points: Vec<(f32, f32)>,
    pub contour_ends: Vec<u32>,
}

#[derive(Copy, Clone, PartialEq, Debug, Default)]
struct Aabb {
    xmin: f32,
    xmax: f32,
    ymin: f32,
    ymax: f32,
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct CubeCurve {
    a: Point,
    b: Point,
    c: Point,
    d: Point,
}

impl CubeCurve {
    const fn new(a: Point, b: Point, c: Point, d: Point) -> CubeCurve {
        CubeCurve { a, b, c, d }
    }

    fn point(&self, t: f32) -> Point {
        let tm = 1.0 - t;
        let a = tm * tm * tm;
        let b = 3.0 * (tm * tm) * t;
        let c = 3.0 * tm * (t * t);
        let d = t * t * t;

        let x = a * self.a.x + b * self.b.x + c * self.c.x + d * self.d.x;
        let y = a * self.a.y + b * self.b.y + c * self.c.y + d * self.d.y;
        Point::new(x, y)
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct QuadCurve {
    a: Point,
    b: Point,
    c: Point,
}

impl QuadCurve {
    fn new(a: Point, b: Point, c: Point) -> QuadCurve {
        QuadCurve { a, b, c }
    }

    fn point(&self, t: f32) -> Point {
        let tm = 1.0 - t;
        let a = tm * tm;
        let b = 2.0 * tm * t;
        let c = t * t;

        let x = a * self.a.x + b * self.b.x + c * self.c.x;
        let y = a * self.a.y + b * self.b.y + c * self.c.y;
        Point::new(x, y)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Line {
    pub coords: f32x4,
    pub nudge: f32x4,
    pub adjustment: f32x4,
    pub params: f32x4,
}

impl Line {
    pub fn new(start: Point, end: Point) -> Line {
        const FLOOR_NUDGE: u32 = 0;
        const CEIL_NUDGE: u32 = 1;

        let (x_start_nudge, x_first_adj) = if end.x >= start.x {
            (FLOOR_NUDGE, 1.0)
        } else {
            (CEIL_NUDGE, 0.0)
        };
        let (y_start_nudge, y_first_adj) = if end.y >= start.y {
            (FLOOR_NUDGE, 1.0)
        } else {
            (CEIL_NUDGE, 0.0)
        };

        let x_end_nudge = if end.x > start.x {
            CEIL_NUDGE
        } else {
            FLOOR_NUDGE
        };
        let y_end_nudge = if end.y > start.y {
            CEIL_NUDGE
        } else {
            FLOOR_NUDGE
        };

        let dx = end.x - start.x;
        let dy = end.y - start.y;
        // The substitute has to be positive, not finite: a negative one sends the walker down its
        // y branch and NaNs the row. `copysign` and `1.0 / d` both fail – tests/cpu/zero_delta.rs.
        let tdx = if dx == 0.0 { f32::MAX } else { 1.0 / dx };
        let tdy = if dy == 0.0 { f32::MAX } else { 1.0 / dy };

        Line {
            coords: f32x4::new(start.x, start.y, end.x, end.y),
            nudge: f32x4::new_u32(x_start_nudge, y_start_nudge, x_end_nudge, y_end_nudge),
            adjustment: f32x4::new(x_first_adj, y_first_adj, 0.0, 0.0),
            params: f32x4::new(tdx, tdy, dx, dy),
        }
    }
}

pub struct Geometry {
    v_segments: Vec<[Point; 2]>,
    m_segments: Vec<[Point; 2]>,
    effective_bounds: Aabb,
    start_point: Point,
    previous_point: Point,
    area: f32,
    max_area: f32,
    stack: [Segment; 16],
    points: Vec<(f32, f32)>,
    ends: Vec<u32>,
    recording: bool,
}

// Past this many flattened edges the arrangement work stops being worth what it costs: the crossing
// split and the guard are both quadratic in the edge count. A glyph that big is either enormous on
// screen, where the over-coverage is a pixel at the end of a long edge, or a stroke outline, which
// has other ways to be resolved.
const MAX_RESOLVE_EDGES: usize = 512;

#[derive(Clone, Copy)]
struct Segment {
    a: Point,
    at: f32,
    c: Point,
    ct: f32,
}

impl Segment {
    const fn new(a: Point, at: f32, c: Point, ct: f32) -> Segment {
        Segment { a, at, c, ct }
    }
}

const MIN_SEGMENT_SPAN: f32 = 1.0 / 4096.0;

#[inline(always)]
fn too_curved(a: Point, b: Point, c: Point, max_area: f32) -> bool {
    platform::abs((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)) > max_area
}

impl OutlinePen for Geometry {
    fn move_to(&mut self, x0: f32, y0: f32) {
        let next_point = Point::new(x0, y0);
        self.end_contour();
        if self.recording {
            self.points.push((x0, y0));
        }
        self.start_point = next_point;
        self.previous_point = next_point;
    }

    fn line_to(&mut self, x0: f32, y0: f32) {
        let next_point = Point::new(x0, y0);
        self.push(self.previous_point, next_point);
        self.previous_point = next_point;
    }

    fn quad_to(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let control_point = Point::new(x0, y0);
        let next_point = Point::new(x1, y1);
        let curve = QuadCurve::new(self.previous_point, control_point, next_point);
        self.flatten(self.previous_point, next_point, |t| curve.point(t));

        self.previous_point = next_point;
    }

    fn curve_to(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, x2: f32, y2: f32) {
        let first_control = Point::new(x0, y0);
        let second_control = Point::new(x1, y1);
        let next_point = Point::new(x2, y2);

        let curve = CubeCurve::new(
            self.previous_point,
            first_control,
            second_control,
            next_point,
        );
        self.flatten(self.previous_point, next_point, |t| curve.point(t));

        self.previous_point = next_point;
    }

    fn close(&mut self) {
        if self.start_point != self.previous_point {
            self.push(self.previous_point, self.start_point);
        }
        self.previous_point = self.start_point;
        self.end_contour();
    }
}

impl Geometry {
    pub fn new(px: f32, units_per_em: f32) -> Geometry {
        Self::with_segments(px, units_per_em, Vec::new(), Vec::with_capacity(64))
    }

    pub fn reusing(px: f32, units_per_em: f32, glyph: &mut Glyph) -> Geometry {
        let mut v = core::mem::take(&mut glyph.v_segments);
        let mut m = core::mem::take(&mut glyph.m_segments);
        let mut p = core::mem::take(&mut glyph.contour_points);
        let mut e = core::mem::take(&mut glyph.contour_ends);
        v.clear();
        m.clear();
        p.clear();
        e.clear();
        if m.capacity() == 0 {
            m.reserve(64);
        }
        Self::with_all(px, units_per_em, v, m, p, e)
    }

    fn with_segments(
        px: f32,
        units_per_em: f32,
        v_segments: Vec<[Point; 2]>,
        m_segments: Vec<[Point; 2]>,
    ) -> Geometry {
        Self::with_all(px, units_per_em, v_segments, m_segments, Vec::new(), Vec::new())
    }

    fn with_all(
        px: f32,
        units_per_em: f32,
        v_segments: Vec<[Point; 2]>,
        m_segments: Vec<[Point; 2]>,
        points: Vec<(f32, f32)>,
        ends: Vec<u32>,
    ) -> Geometry {
        const ERROR_THRESHOLD: f32 = 3.0;
        let max_area = ERROR_THRESHOLD * 2.0 * (units_per_em / px);

        Geometry {
            stack: [Segment::new(Point::new(0.0, 0.0), 0.0, Point::new(0.0, 0.0), 0.0); 16],
            v_segments,
            m_segments,
            effective_bounds: Aabb {
                xmin: f32::MAX,
                xmax: f32::MIN,
                ymin: f32::MAX,
                ymax: f32::MIN,
            },
            start_point: Point::default(),
            previous_point: Point::default(),
            area: 0.0,
            max_area,
            points,
            ends,
            recording: true,
        }
    }

    fn flatten(&mut self, start: Point, end: Point, point_at: impl Fn(f32) -> Point) {
        let mid = point_at(0.5);
        // Negated rather than `<=`, which is a different test: on a NaN control point both are
        // false, and `<=` would subdivide to the depth cap and emit 4,096 segments for one curve.
        if !too_curved(start, mid, end, self.max_area) {
            self.push(start, end);
            return;
        }

        // Later half first, because the stack pops from its end: that is what makes the segments
        // come out in curve order. The accumulator does not care – it sums signed area – but the
        // contour recording does, and a scrambled point sequence is a self-intersecting contour.
        self.stack[0] = Segment::new(mid, 0.5, end, 1.0);
        self.stack[1] = Segment::new(start, 0.0, mid, 0.5);
        let mut len = 2usize;

        while len > 0 {
            len -= 1;
            let seg = self.stack[len];
            let bt = (seg.at + seg.ct) * 0.5;
            let b = point_at(bt);
            if too_curved(seg.a, b, seg.c, self.max_area) && seg.ct - seg.at > MIN_SEGMENT_SPAN {
                // Tested against the room the pair needs, not the array bound – `== 16` could
                // never fire, so it would read as a guard while leaving a real overrun.
                if len > 11 {
                    self.push(seg.a, seg.c);
                    continue;
                }
                self.stack[len] = Segment::new(b, bt, seg.c, seg.ct);
                self.stack[len + 1] = Segment::new(seg.a, seg.at, b, bt);
                len += 2;
            } else {
                self.push(seg.a, seg.c);
            }
        }
    }

    fn push(&mut self, start: Point, end: Point) {
        if self.recording {
            self.points.push((end.x, end.y));
        }
        self.emit(start, end);
    }

    fn emit(&mut self, start: Point, end: Point) {
        if start.y.to_bits() != end.y.to_bits() {
            self.area += (end.y - start.y) * (end.x + start.x);
            if start.x.to_bits() == end.x.to_bits() {
                self.v_segments.push([start, end]);
            } else {
                self.m_segments.push([start, end]);
            }
            Self::recalculate_bounds(&mut self.effective_bounds, start.x, start.y);
            Self::recalculate_bounds(&mut self.effective_bounds, end.x, end.y);
        }
    }

    fn end_contour(&mut self) {
        if self.recording && self.points.len() > self.ends.last().map_or(0, |&e| e as usize) {
            self.ends.push(self.points.len() as u32);
        }
    }

    // The contours as `simplify` wants them, with the closing duplicate dropped: `push` records the
    // end of every flattened edge, and `close` emits one back to the start.
    fn contours(&self) -> Vec<Vec<(f32, f32)>> {
        let mut out = Vec::with_capacity(self.ends.len());
        let mut from = 0usize;
        for &end in &self.ends {
            let mut c = self.points[from..end as usize].to_vec();
            if c.len() > 1 && c[0] == c[c.len() - 1] {
                c.pop();
            }
            if c.len() >= 3 {
                out.push(c);
            }
            from = end as usize;
        }
        out
    }

    // Overlapping contours are the one arrangement the accumulator cannot represent: it holds the
    // integral of winding over each pixel, so where two contours cover the same partly covered pixel
    // it sums their coverage and clamps. Interiors survive that, antialiased edges do not – Inter's
    // "4" renders its flat top at 255 where the diagonal and the stem overlap and 188 either side.
    //
    // Unioning the contours first removes the overlap and leaves a single boundary, and since the
    // rasterizer flattens anyway this costs no curve fidelity. Nested and disjoint contours are
    // already a clean arrangement, so nothing runs for them.
    fn resolve_overlaps(&mut self) {
        self.end_contour();
        if self.ends.len() < 2 || self.points.len() > MAX_RESOLVE_EDGES {
            return;
        }
        let contours = self.contours();
        if contours.len() < 2 {
            return;
        }
        if !crate::daecore::daetype::outline::simplify::needs_union(&contours) {
            return;
        }
        let Some(resolved) = crate::daecore::daetype::outline::simplify::union_verified(&contours)
        else {
            return;
        };

        self.recording = false;
        self.v_segments.clear();
        self.m_segments.clear();
        // The empty sentinel, not `Aabb::default()`: that is all zeros, which pins the origin inside
        // every box it then accumulates into.
        self.effective_bounds = Aabb { xmin: f32::MAX, xmax: f32::MIN, ymin: f32::MAX, ymax: f32::MIN };
        self.area = 0.0;
        for c in &resolved {
            for i in 0..c.len() {
                let (a, b) = (c[i], c[(i + 1) % c.len()]);
                self.emit(Point::new(a.0, a.1), Point::new(b.0, b.1));
            }
        }
    }

    pub fn finalize(mut self, glyph: &mut Glyph) {
        self.resolve_overlaps();
        if self.v_segments.is_empty() && self.m_segments.is_empty() {
            self.effective_bounds = Aabb::default();
        } else {
            let reverse = self.area > 0.0;
            let bounds = self.effective_bounds;
            for seg in self.v_segments.iter_mut().chain(self.m_segments.iter_mut()) {
                let (s, e) = if reverse { (seg[1], seg[0]) } else { (seg[0], seg[1]) };
                *seg = [
                    Point::new(s.x - bounds.xmin, abs(s.y - bounds.ymax)),
                    Point::new(e.x - bounds.xmin, abs(e.y - bounds.ymax)),
                ];
            }
        }
        glyph.v_segments = self.v_segments;
        glyph.m_segments = self.m_segments;
        glyph.contour_points = self.points;
        glyph.contour_ends = self.ends;
        glyph.bounds = OutlineBounds {
            xmin: self.effective_bounds.xmin,
            ymin: self.effective_bounds.ymin,
            width: self.effective_bounds.xmax - self.effective_bounds.xmin,
            height: self.effective_bounds.ymax - self.effective_bounds.ymin,
        };
    }

    fn recalculate_bounds(bounds: &mut Aabb, x: f32, y: f32) {
        if x < bounds.xmin {
            bounds.xmin = x;
        }
        if x > bounds.xmax {
            bounds.xmax = x;
        }
        if y < bounds.ymin {
            bounds.ymin = y;
        }
        if y > bounds.ymax {
            bounds.ymax = y;
        }
    }
}
