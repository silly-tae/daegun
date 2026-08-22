use alloc::vec::Vec;

#[allow(unused_imports, reason = "the inherent method shadows this whenever std is linked")]
use crate::daecore::daemachine::float::FloatExt;

pub type Pt = (f32, f32);

type Edge = (Pt, Pt);

const EPS: f32 = 1e-4;

// A stroke's pieces overlap by construction, and only a non-zero fill hides that: under even-odd
// the second covering cancels the first and the overlap renders as a hole. This resolves the
// arrangement so both rules agree, which is also what makes an area measurement meaningful.
const PROBE: f32 = EPS * 10.0;

pub fn union(contours: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    let Some((_, kept)) = boundary(contours) else { return contours.to_vec() };
    chain(&kept).0
}

// The union, adopted only when it provably describes the same filled region as its input.
//
// `union` alone cannot say whether it succeeded, and its failures are quiet: `chain` gives up on an
// edge it cannot continue, `EPS` is an absolute distance so it means different things at different
// scales, and two collinear coincident edges are never split apart. A caller rasterizing the result
// would ship a wrong glyph. So the answer is withheld unless the arrangement closed, kept its
// bounding box, and agrees with the input about which side of every edge is filled – the last being
// what catches a misclassified edge, which is how a spurious hole or spike would get through.
//
// `None` is not a failure to handle, it is the instruction to keep what you had.
// The union, but only for contours that actually overlap, and only when it verifies. One
// arrangement serves both questions.
pub fn union_if_overlapping(contours: &[Vec<Pt>]) -> Option<Vec<Vec<Pt>>> {
    if contours.len() < 2 {
        return None;
    }
    let before = Arrangement::new(edges_of(contours));
    if !overlaps(&before) {
        return None;
    }
    union_from(before)
}

fn union_from(before: Arrangement) -> Option<Vec<Vec<Pt>>> {
    let (before, kept) = boundary_of(before)?;
    let (out, closed) = chain(&kept);
    if !closed || out.is_empty() {
        return None;
    }

    let mut after: Vec<Edge> = Vec::with_capacity(out.iter().map(Vec::len).sum());
    for c in &out {
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            if !same(a, b) {
                after.push((a, b));
            }
        }
    }

    let after = Arrangement::new(after);
    if !same_bounds(before.edges(), after.edges()) {
        return None;
    }
    // Both sides of every edge, in both arrangements. An edge on the boundary has one side filled
    // in each; a seam left inside the union has both sides filled in each. Either way they agree.
    for list in [before.edges(), after.edges()] {
        for &(a, b) in list {
            let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
            let n = normal(a, b);
            for side in [PROBE, -PROBE] {
                let p = (mid.0 + n.0 * side, mid.1 + n.1 * side);
                if before.covered(p) != after.covered(p) {
                    return None;
                }
            }
        }
    }
    Some(out)
}

// Whether a coverage accumulator would get this arrangement wrong, which is exactly when some point
// is wound twice or more: the integral of winding over a pixel only equals the filled area while the
// winding is 0 or 1. A nested counter winds its interior back to 0 and a disjoint contour never adds
// to another, so neither reaches here — only genuine overlap does, whether the contours cross or
// merely run along each other, which is most of what a glyph drawn on a grid does.
//
// Winding changes only across an edge, so probing both sides of every edge finds any twice wound
// region without having to know where it is. That is one probe per edge against a bucketed
// arrangement rather than against every edge, which is what makes it cheap enough to run for every
// glyph — about a fifth of a microsecond, against the sixty the resolution itself costs.
// Whether any point is covered twice over. Takes the arrangement rather than the contours so the
// caller that goes on to resolve them does not build a second identical one.
fn overlaps(arrangement: &Arrangement) -> bool {
    for &(a, b) in arrangement.edges() {
        let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
        let n = normal(a, b);
        for side in [PROBE, -PROBE] {
            if arrangement.winding((mid.0 + n.0 * side, mid.1 + n.1 * side)).abs() >= 2 {
                return true;
            }
        }
    }
    false
}

fn edges_of(contours: &[Vec<Pt>]) -> Vec<Edge> {
    let mut edges: Vec<Edge> = Vec::with_capacity(contours.iter().map(Vec::len).sum());
    for c in contours {
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            if !same(a, b) {
                edges.push((a, b));
            }
        }
    }
    edges
}

fn boundary(contours: &[Vec<Pt>]) -> Option<(Arrangement, Vec<Edge>)> {
    boundary_of(Arrangement::new(edges_of(contours)))
}

fn boundary_of(before: Arrangement) -> Option<(Arrangement, Vec<Edge>)> {
    if before.edges().len() < 3 {
        return None;
    }

    let split = split_at_crossings(&before);
    let mut kept: Vec<Edge> = Vec::with_capacity(split.len());
    for &(a, b) in &split {
        let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
        let n = normal(a, b);
        let left = before.covered((mid.0 + n.0 * PROBE, mid.1 + n.1 * PROBE));
        let right = before.covered((mid.0 - n.0 * PROBE, mid.1 - n.1 * PROBE));
        match (left, right) {
            (true, false) => kept.push((a, b)),
            (false, true) => kept.push((b, a)),
            _ => {}
        }
    }
    Some((before, dedup(kept)))
}

// Two contours that share part of an edge – which is most of what a glyph does, being drawn on a
// grid – leave that piece in `kept` twice. Identical copies are one boundary counted twice, so one
// survives; a back to back pair faces away from each other and bounds nothing, so neither does.
fn dedup(kept: Vec<Edge>) -> Vec<Edge> {
    let mut drop = alloc::vec![false; kept.len()];
    for i in 0..kept.len() {
        if drop[i] {
            continue;
        }
        for j in i + 1..kept.len() {
            if drop[j] {
                continue;
            }
            let ((a, b), (c, d)) = (kept[i], kept[j]);
            if same(a, c) && same(b, d) {
                drop[j] = true;
            } else if same(a, d) && same(b, c) {
                drop[i] = true;
                drop[j] = true;
                break;
            }
        }
    }
    kept.into_iter().zip(drop).filter(|&(_, d)| !d).map(|(e, _)| e).collect()
}

fn same_bounds(a: &[Edge], b: &[Edge]) -> bool {
    let box_of = |e: &[Edge]| {
        let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for &(p, q) in e {
            for v in [p, q] {
                r[0] = r[0].min(v.0);
                r[1] = r[1].min(v.1);
                r[2] = r[2].max(v.0);
                r[3] = r[3].max(v.1);
            }
        }
        r
    };
    let (x, y) = (box_of(a), box_of(b));
    x.iter().zip(&y).all(|(p, q)| (p - q).abs() < EPS * 10.0)
}

fn split_at_crossings(before: &Arrangement) -> Vec<Edge> {
    let edges = before.edges();
    // Boxes first. Every edge is otherwise tested against every other with real arithmetic, twice
    // over once collinear overlap is looked for as well, and almost none of those pairs are
    // anywhere near each other. The margin is the tolerance the tests below work to.
    let boxes: Vec<[f32; 4]> = edges
        .iter()
        .map(|&(a, b)| {
            [
                a.0.min(b.0) - EPS,
                a.1.min(b.1) - EPS,
                a.0.max(b.0) + EPS,
                a.1.max(b.1) + EPS,
            ]
        })
        .collect();

    let mut out = Vec::with_capacity(edges.len() * 2);
    let mut ts: Vec<f32> = Vec::new();
    for (i, &(a, b)) in edges.iter().enumerate() {
        ts.clear();
        for (j, &(c, d)) in edges.iter().enumerate() {
            if i == j {
                continue;
            }
            let (p, q) = (boxes[i], boxes[j]);
            if p[2] < q[0] || q[2] < p[0] || p[3] < q[1] || q[3] < p[1] {
                continue;
            }
            if let Some(t) = cross_param(a, b, c, d)
                && t > EPS
                && t < 1.0 - EPS
            {
                ts.push(t);
            }
            collinear_params(a, b, c, d, &mut ts);
        }
        ts.push(1.0);
        ts.sort_by(f32::total_cmp);

        let mut from = a;
        let mut last = 0.0f32;
        for &t in &ts {
            if t - last < EPS {
                continue;
            }
            let to = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if !same(from, to) {
                out.push((from, to));
            }
            from = to;
            last = t;
        }
    }
    out
}

// Where an edge lies along another, split it at the other's endpoints. `cross_param` cannot see
// this: parallel lines have no crossing parameter, so without it two contours sharing part of an
// edge are never cut apart and the arrangement never resolves.
fn collinear_params(a: Pt, b: Pt, c: Pt, d: Pt, ts: &mut Vec<f32>) {
    let r = (b.0 - a.0, b.1 - a.1);
    let len2 = r.0 * r.0 + r.1 * r.1;
    if len2 < 1e-12 {
        return;
    }
    let len = len2.sqrt();
    for p in [c, d] {
        let off = ((p.0 - a.0) * r.1 - (p.1 - a.1) * r.0) / len;
        if off.abs() > EPS {
            continue;
        }
        let t = ((p.0 - a.0) * r.0 + (p.1 - a.1) * r.1) / len2;
        if t > EPS && t < 1.0 - EPS {
            ts.push(t);
        }
    }
}

// The edges bucketed by y, because the inside test is a ray cast and every one of them was scanning
// every edge. The arrangement is asked about a point once per side of every edge, twice over for the
// guard, so that scan is quadratic and it dominated everything else here.
//
// A bucket per edge, each holding the edges whose y range reaches it. A glyph outline is mostly
// short edges, so a bucket holds a handful and building the whole thing costs about what one scan
// used to.
pub(crate) struct Arrangement {
    edges: Vec<Edge>,
    // One flat list with a start offset per bucket, rather than a vector per bucket: the buckets are
    // built and thrown away once per glyph, and a hundred small allocations cost more than the scan
    // this is here to shorten.
    index: Vec<u32>,
    start: Vec<u32>,
    ymin:  f32,
    inv:   f32,
}

impl Arrangement {
    pub(crate) fn new(edges: Vec<Edge>) -> Arrangement {
        let (mut ymin, mut ymax) = (f32::MAX, f32::MIN);
        for &(a, b) in &edges {
            ymin = ymin.min(a.1).min(b.1);
            ymax = ymax.max(a.1).max(b.1);
        }
        let n = edges.len().clamp(1, 256);
        let span = ymax - ymin;
        let inv = if span > 1e-9 { n as f32 / span } else { 0.0 };

        let range = |a: Pt, b: Pt| {
            let last = n as isize - 1;
            let lo = (((a.1.min(b.1) - ymin) * inv) as isize).clamp(0, last);
            let hi = (((a.1.max(b.1) - ymin) * inv) as isize).clamp(0, last);
            (lo as usize, hi as usize)
        };

        let mut start = alloc::vec![0u32; n + 1];
        for &(a, b) in &edges {
            let (lo, hi) = range(a, b);
            for slot in &mut start[lo + 1..=hi + 1] {
                *slot += 1;
            }
        }
        for k in 0..n {
            start[k + 1] += start[k];
        }
        let mut fill = start.clone();
        let mut index = alloc::vec![0u32; start[n] as usize];
        for (i, &(a, b)) in edges.iter().enumerate() {
            let (lo, hi) = range(a, b);
            for k in lo..=hi {
                index[fill[k] as usize] = i as u32;
                fill[k] += 1;
            }
        }
        Arrangement { edges, index, start, ymin, inv }
    }

    pub(crate) fn edges(&self) -> &[Edge] {
        &self.edges
    }


    pub(crate) fn winding(&self, p: Pt) -> i32 {
        let k = (((p.1 - self.ymin) * self.inv) as isize).clamp(0, self.start.len() as isize - 2);
        let (k, mut w) = (k as usize, 0i32);
        // The crossing test multiplied through by the y span, which takes a divide out of the
        // innermost loop here and flips the comparison when that span is negative.
        for &i in &self.index[self.start[k] as usize..self.start[k + 1] as usize] {
            let (a, b) = self.edges[i as usize];
            if (a.1 <= p.1) != (b.1 <= p.1) {
                let dy = b.1 - a.1;
                let lhs = (p.1 - a.1) * (b.0 - a.0);
                let rhs = (p.0 - a.0) * dy;
                if if dy > 0.0 { lhs > rhs } else { lhs < rhs } {
                    w += if dy > 0.0 { 1 } else { -1 };
                }
            }
        }
        w
    }

    pub(crate) fn covered(&self, p: Pt) -> bool {
        self.winding(p) != 0
    }
}

fn cross_param(a: Pt, b: Pt, c: Pt, d: Pt) -> Option<f32> {
    let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    let denom = r.0 * s.1 - r.1 * s.0;
    if denom.abs() < 1e-12 {
        return None;
    }
    let (dx, dy) = (c.0 - a.0, c.1 - a.1);
    let t = (dx * s.1 - dy * s.0) / denom;
    let u = (dx * r.1 - dy * r.0) / denom;
    (-EPS..=1.0 + EPS).contains(&u).then_some(t)
}

// The kept edges walked into closed contours. The flag says whether every one of them was consumed
// into a contour that came back to where it started, which is the difference between an arrangement
// that resolved and one that ran out of edges partway round.
fn chain(edges: &[Edge]) -> (Vec<Vec<Pt>>, bool) {
    let mut used = alloc::vec![false; edges.len()];
    let mut out: Vec<Vec<Pt>> = Vec::new();
    let mut closed = true;


    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        // Bounded by what is left rather than grown from one: a contour can consume every remaining
        // edge, and MAX_RESOLVE_EDGES keeps the over-reservation small.
        let mut contour = Vec::with_capacity(edges.len() - start);
        contour.push(edges[start].0);
        let mut at = edges[start].1;
        let first = edges[start].0;

        for _ in 0..edges.len() {
            if same(at, first) {
                break;
            }
            contour.push(at);
            match (0..edges.len()).find(|&k| !used[k] && same(edges[k].0, at)) {
                Some(next) => {
                    used[next] = true;
                    at = edges[next].1;
                }
                None => {
                    closed = false;
                    break;
                }
            }
        }
        if !same(at, first) {
            closed = false;
        }
        if contour.len() >= 3 {
            out.push(contour);
        } else {
            closed = false;
        }
    }
    (out, closed && used.iter().all(|&u| u))
}

fn same(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() < EPS && (a.1 - b.1).abs() < EPS
}

fn normal(a: Pt, b: Pt) -> Pt {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len > 1e-9 { (-dy / len, dx / len) } else { (0.0, 1.0) }
}
