use alloc::vec::Vec;
use super::extract::Quad;

#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;

pub const HULL_VERTICES: usize = 5;

// Bounds what actually misbehaves. Testing the determinant of the two edge normals instead – the
// sine of the corner's turn – is small for a nearly flat corner as well as a folded one, and only
// the folded one runs away. Rejecting on the sine is why more vertices used to render worse.
const MAX_TRAVEL: f32 = 6.0;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[non_exhaustive]
pub struct HullVertex {
    pub pos: [f32; 2],
    // A matrix, because dilation is not isotropic: a subpixel layout reaches a pixel sideways and
    // half a pixel up. The solve is linear in `pad`, so the division by a near-degenerate
    // determinant happens here once per glyph in f64, not per vertex per frame on hardware.
    pub dilate: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(crate) struct Hull {
    pub(crate) verts: [HullVertex; HULL_VERTICES],
}

const fn ord_key(v: f32) -> u32 {
    let b = v.to_bits();
    b ^ ((((b as i32) >> 31) as u32) | 0x8000_0000)
}

const fn un_ord(k: u32) -> f32 {
    f32::from_bits(k ^ ((!(((k as i32) >> 31) as u32)) | 0x8000_0000))
}

fn sort_dedup(pts: &mut Vec<[f32; 2]>, keys: &mut Vec<u64>) {
    keys.clear();
    keys.reserve(pts.len());
    keys.extend(pts.iter().map(|p| key_of(*p)));
    keys.sort_unstable();
    keys.dedup();
    pts.clear();
    pts.extend(keys.iter().map(|&k| [un_ord((k >> 32) as u32), un_ord(k as u32)]));
}

fn chain(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if pts.len() < 3 {
        return Vec::new();
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut h: Vec<[f32; 2]> = Vec::with_capacity(pts.len() + 1);
    for &p in pts.iter() {
        while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    let floor = h.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while h.len() >= floor && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    h.pop();
    h
}

fn key_of(p: [f32; 2]) -> u64 {
    (u64::from(ord_key(p[0])) << 32) | u64::from(ord_key(p[1]))
}

const EXTREME_DIRECTIONS: usize = 8;

const CHAIN_SCRATCH: usize = EXTREME_DIRECTIONS + 1;

fn chain8(pts: &[[f32; 2]], out: &mut [[f32; 2]; CHAIN_SCRATCH]) -> usize {
    debug_assert!(pts.len() <= EXTREME_DIRECTIONS, "chain8 got {} points", pts.len());
    if pts.len() < 3 {
        return 0;
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut n = 0usize;
    for &p in pts {
        while n >= 2 && cross(out[n - 2], out[n - 1], p) <= 0.0 {
            n -= 1;
        }
        out[n] = p;
        n += 1;
    }
    let floor = n + 1;
    for &p in pts.iter().rev().skip(1) {
        while n >= floor && cross(out[n - 2], out[n - 1], p) <= 0.0 {
            n -= 1;
        }
        out[n] = p;
        n += 1;
    }
    n - 1
}

fn extremes(curves: &[Quad]) -> Option<[[f32; 2]; EXTREME_DIRECTIONS]> {
    let first = curves.first()?[0];
    let mut e = [first; 8];
    let mut b = [f32::NEG_INFINITY; 8];
    for c in curves {
        for p in c {
            let (x, y) = (p[0], p[1]);
            let (s, d) = (x + y, x - y);
            if x > b[0] { b[0] = x; e[0] = *p; }
            if -x > b[1] { b[1] = -x; e[1] = *p; }
            if y > b[2] { b[2] = y; e[2] = *p; }
            if -y > b[3] { b[3] = -y; e[3] = *p; }
            if s > b[4] { b[4] = s; e[4] = *p; }
            if -s > b[5] { b[5] = -s; e[5] = *p; }
            if d > b[6] { b[6] = d; e[6] = *p; }
            if -d > b[7] { b[7] = -d; e[7] = *p; }
        }
    }
    if b.iter().all(|v| v.is_finite()) { Some(e) } else { None }
}

fn candidates(curves: &[Quad], out: &mut Vec<[f32; 2]>) {
    out.clear();
    let n = curves.len() * 3;
    if n < 24 {
        out.reserve(n);
        for c in curves {
            for p in c {
                out.push(*p);
            }
        }
        return;
    }

    let Some(mut ext) = extremes(curves) else {
        out.reserve(n);
        for c in curves {
            for p in c {
                out.push(*p);
            }
        }
        return;
    };

    let mut keys = [0u64; 8];
    for i in 0..8 {
        keys[i] = key_of(ext[i]);
    }
    for i in 1..8 {
        let (v, k) = (ext[i], keys[i]);
        let mut j = i;
        while j > 0 && keys[j - 1] > k {
            ext[j] = ext[j - 1];
            keys[j] = keys[j - 1];
            j -= 1;
        }
        ext[j] = v;
        keys[j] = k;
    }
    let mut m = 0usize;
    for i in 0..8 {
        if m == 0 || keys[m - 1] != keys[i] {
            ext[m] = ext[i];
            keys[m] = keys[i];
            m += 1;
        }
    }

    let mut buf = [[0.0f32; 2]; CHAIN_SCRATCH];
    let plen = chain8(&ext[..m], &mut buf);
    if plen < 3 {
        out.reserve(n);
        for c in curves {
            for p in c {
                out.push(*p);
            }
        }
        return;
    }
    let poly = &buf[..plen];

    out.reserve(n / 2);
    for c in curves {
        for p in c {
            let mut a = poly[plen - 1];
            let mut inside = true;
            for &b in poly {
                if (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) <= 0.0 {
                    inside = false;
                    break;
                }
                a = b;
            }
            if !inside {
                out.push(*p);
            }
        }
    }
}

fn convex_hull(pts: &mut Vec<[f32; 2]>, keys: &mut Vec<u64>) -> Vec<[f32; 2]> {
    sort_dedup(pts, keys);
    chain(pts)
}

fn meet(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> Option<[f32; 2]> {
    let (r, s) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1.0e-12 {
        return None;
    }
    let t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den;
    Some([a[0] + r[0] * t, a[1] + r[1] * t])
}

fn simplify(mut poly: Vec<[f32; 2]>, k: usize) -> Vec<[f32; 2]> {
    while poly.len() > k && poly.len() > 3 {
        let n = poly.len();
        let mut best: Option<(f32, usize, [f32; 2])> = None;
        for i in 0..n {
            let (p0, p1) = (poly[(i + n - 1) % n], poly[i]);
            let (p2, p3) = (poly[(i + 1) % n], poly[(i + 2) % n]);
            let Some(x) = meet(p0, p1, p2, p3) else { continue };
            let side = (p2[0] - p1[0]) * (x[1] - p1[1]) - (p2[1] - p1[1]) * (x[0] - p1[0]);
            if side >= 0.0 {
                continue;
            }
            let added = -side * 0.5;
            if best.is_none_or(|(b, _, _)| added < b) {
                best = Some((added, i, x));
            }
        }
        let Some((_, i, x)) = best else { break };
        poly[i] = x;
        poly.remove((i + 1) % n);
    }
    poly
}

fn dilation_matrices(poly: &[[f32; 2]]) -> Option<[[f32; 4]; HULL_VERTICES]> {
    let n = poly.len();
    if !(3..=HULL_VERTICES).contains(&n) {
        return None;
    }
    let mut edge = [[0.0f64; 2]; HULL_VERTICES];
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (ex, ey) = (f64::from(b[0] - a[0]), f64::from(b[1] - a[1]));
        let len = (ex * ex + ey * ey).sqrt();
        if len < 1.0e-12 {
            return None;
        }
        edge[i] = [ey / len, -ex / len];
    }

    let mut out = [[0.0f32; 4]; HULL_VERTICES];
    for i in 0..n {
        let a = edge[(i + n - 1) % n];
        let b = edge[i];
        let det = a[0] * b[1] - a[1] * b[0];
        if det == 0.0 {
            return None;
        }
        let (sa, sb) = ([a[0].abs(), a[1].abs()], [b[0].abs(), b[1].abs()]);
        let m = [
            (sa[0] * b[1] - a[1] * sb[0]) / det,
            (sa[1] * b[1] - a[1] * sb[1]) / det,
            (a[0] * sb[0] - sa[0] * b[0]) / det,
            (a[0] * sb[1] - sa[1] * b[0]) / det,
        ];
        if m.iter().any(|v| !v.is_finite() || v.abs() > f64::from(MAX_TRAVEL)) {
            return None;
        }
        out[i] = [m[0] as f32, m[1] as f32, m[2] as f32, m[3] as f32];
    }
    Some(out)
}

fn box_hull(box_min: [f32; 2], box_max: [f32; 2]) -> Hull {
    let corner = |x: f32, y: f32, sx: f32, sy: f32| HullVertex {
        pos: [x, y],
        dilate: [sx, 0.0, 0.0, sy],
    };
    let strip = [
        corner(box_min[0], box_min[1], -1.0, -1.0),
        corner(box_max[0], box_min[1], 1.0, -1.0),
        corner(box_min[0], box_max[1], -1.0, 1.0),
        corner(box_max[0], box_max[1], 1.0, 1.0),
    ];
    let mut verts = [HullVertex::default(); HULL_VERTICES];
    for (i, v) in verts.iter_mut().enumerate() {
        *v = strip[i.min(strip.len() - 1)];
    }
    Hull { verts }
}

// Rests on one property: the polygon contains everything the outline covers. A quadratic lies
// inside the hull of its three control points, simplification only ever grows the polygon, and the
// vertex stage dilates outward – so nothing in the chain can crop a glyph.
pub(crate) fn build(curves: &[Quad], box_min: [f32; 2], box_max: [f32; 2]) -> Hull {
    let mut pts: Vec<[f32; 2]> = Vec::new();
    candidates(curves, &mut pts);
    let mut keys: Vec<u64> = Vec::new();

    let poly = simplify(convex_hull(&mut pts, &mut keys), HULL_VERTICES);
    if poly.len() < 3 || poly.len() > HULL_VERTICES {
        return box_hull(box_min, box_max);
    }
    let Some(dilate) = dilation_matrices(&poly) else {
        return box_hull(box_min, box_max);
    };

    let vert = |i: usize| HullVertex { pos: poly[i], dilate: dilate[i] };
    let m = poly.len();
    let mut verts = [HullVertex::default(); HULL_VERTICES];
    let (mut front, mut back) = (0usize, m - 1);
    let mut last = 0usize;
    for (slot, out) in verts.iter_mut().enumerate() {
        let i = if slot >= m {
            last
        } else if slot <= 1 || slot % 2 == 1 {
            let i = front;
            front += 1;
            i
        } else {
            let i = back;
            back = back.saturating_sub(1);
            i
        };
        last = i;
        *out = vert(i);
    }
    Hull { verts }
}
