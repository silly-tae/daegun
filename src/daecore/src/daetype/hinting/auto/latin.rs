use alloc::vec::Vec;

use super::blues::BlueZones;
use super::points::AutoPoints;

use crate::daecore::daetype::hinting::f26dot6;

#[derive(Clone, Debug)]
struct Segment {
    pos: f32,
    points: (u32, u32),
    dir: i8,
    x_min: f32,
    x_max: f32,
    link: Option<usize>,
}

#[derive(Clone, Debug)]
struct Edge {
    pos: f32,
    dir: i8,
    head: u32,
    tail: u32,
    count: u32,
    link: Option<usize>,
}

const NIL: u32 = u32::MAX;

#[derive(Default)]
pub struct Scratch {
    seg_arena: Vec<usize>,
    segments: Vec<Segment>,
    seg_next: Vec<u32>,
    owner: Vec<u32>,
    edges: Vec<Edge>,
    order: Vec<usize>,
    chosen: Vec<Option<usize>>,
    targets: Vec<i32>,
    blue_matched: Vec<bool>,
    done: Vec<bool>,
    touched: Vec<bool>,
    covered: Vec<bool>,
    anchors: Vec<usize>,
    y: Vec<i32>,
}

pub fn fit<'s>(pts: &AutoPoints, blues: &BlueZones, ppem: u16, upm: u16, s: &'s mut Scratch) -> &'s [i32] {
    let scale = |v: f32| f26dot6::scale(v as i32, ppem, upm);

    let n = pts.len();
    s.y.clear();
    s.y.extend((0..n).map(|i| scale(pts.y[i])));
    if n == 0 {
        return &s.y;
    }

    let seg_tolerance = upm as f32 / 100.0;
    let edge_tolerance = upm as f32 / 50.0;
    let blue_tolerance = upm as f32 / 40.0;

    compute_segments(pts, seg_tolerance, s);
    if s.segments.is_empty() {
        return &s.y;
    }
    link_segments(&mut s.segments, upm as f32 / 3.0, &mut s.chosen);
    compute_edges(edge_tolerance, s);

    let suppress_overshoot = {
        let px_per_unit = ppem as f32 / upm as f32;
        move |z: &super::blues::BlueZone| {
            let gap = (z.overshoot - z.reference).abs() * px_per_unit;
            gap < 1.0
        }
    };

    s.targets.clear();
    s.blue_matched.clear();
    for edge in &s.edges {
        let scaled = scale(edge.pos);
        match blues.nearest(edge.pos, edge.dir > 0, blue_tolerance) {
            Some(z) => {
                let anchor = if suppress_overshoot(z) {
                    z.reference
                } else if (z.overshoot - edge.pos).abs() < (z.reference - edge.pos).abs() {
                    z.overshoot
                } else {
                    z.reference
                };
                s.targets.push(f26dot6::round_to_grid(scale(anchor)));
                s.blue_matched.push(true);
            }
            None => {
                s.targets.push(f26dot6::round_to_grid(scaled));
                s.blue_matched.push(false);
            }
        }
    }

    s.done.clear();
    s.done.resize(s.edges.len(), false);
    for i in 0..s.edges.len() {
        let Some(j) = s.edges[i].link else { continue };
        if s.done[i] || s.done[j] || j >= s.edges.len() {
            continue;
        }
        let anchor = if s.blue_matched[i] || !s.blue_matched[j] { i } else { j };
        let other = if anchor == i { j } else { i };
        let width = fit_stem_width((scale(s.edges[other].pos) - scale(s.edges[anchor].pos)).abs());
        let signed = if s.edges[other].pos > s.edges[anchor].pos { width } else { -width };
        s.targets[other] = s.targets[anchor] + signed;
        s.done[i] = true;
        s.done[j] = true;
    }

    s.touched.clear();
    s.touched.resize(n, false);
    for e in 0..s.edges.len() {
        let delta = s.targets[e] - scale(s.edges[e].pos);
        let mut seg = s.edges[e].head;
        while seg != NIL {
            let sd = &s.segments[seg as usize];
            let (off, len) = (sd.points.0 as usize, sd.points.1 as usize);
            for k in off..off + len {
                let i = s.seg_arena[k];
                if delta != 0 {
                    s.y[i] += delta;
                }
                s.touched[i] = true;
            }
            seg = s.seg_next[seg as usize];
        }
    }

    interpolate_untouched(pts, &mut s.y, &s.touched, ppem, upm, &mut s.anchors);
    &s.y
}

fn side(dx: f32, convention: f32, fallback: i8) -> i8 {
    let d = dx * convention;
    if d > 0.0 {
        1
    } else if d < 0.0 {
        -1
    } else {
        fallback
    }
}

fn winding_convention(pts: &AutoPoints) -> f32 {
    let mut widest = 0.0f32;
    let mut sign = -1.0f32;
    for c in 0..pts.contour_ends.len() {
        let Some((start, end)) = pts.contour(c) else { continue };
        let n = end - start;
        let mut area = 0.0f32;
        for i in 0..n {
            let (a, b) = (start + i, start + (i + 1) % n);
            area += pts.x[a] * pts.y[b] - pts.x[b] * pts.y[a];
        }
        if area.abs() > widest {
            widest = area.abs();
            sign = if area >= 0.0 { 1.0 } else { -1.0 };
        }
    }
    -sign
}

fn compute_segments(pts: &AutoPoints, tolerance: f32, s: &mut Scratch) {
    let convention = winding_convention(pts);
    s.segments.clear();
    s.seg_arena.clear();
    for c in 0..pts.contour_ends.len() {
        let Some((start, end)) = pts.contour(c) else { continue };
        let n = end - start;
        if n < 2 {
            continue;
        }
        let at = |k: usize| start + k % n;

        let Some(break_at) = (0..n).find(|&k| (pts.y[at(k)] - pts.y[at(k + n - 1)]).abs() > tolerance)
        else {
            continue;
        };

        let mut k = 0usize;
        while k < n {
            let first = break_at + k;
            let mut len = 1usize;
            while len < n && (pts.y[at(first + len)] - pts.y[at(first)]).abs() <= tolerance {
                len += 1;
            }
            if len >= 2 {
                let mut sum = 0.0f32;
                for m in 0..len {
                    sum += pts.y[at(first + m)];
                }
                let pos = sum / len as f32;
                let off = s.seg_arena.len() as u32;
                s.seg_arena.extend((0..len).map(|m| at(first + m)));
                let (mut x_min, mut x_max) = (f32::MAX, f32::MIN);
                for &i in &s.seg_arena[off as usize..] {
                    x_min = x_min.min(pts.x[i]);
                    x_max = x_max.max(pts.x[i]);
                }
                let dx = pts.x[at(first + len - 1)] - pts.x[at(first)];
                let neighbours = (pts.y[at(first + n - 1)] + pts.y[at(first + len)]) * 0.5;
                s.segments.push(Segment {
                    pos,
                    points: (off, len as u32),
                    dir: side(dx, convention, if neighbours < pos { 1 } else { -1 }),
                    x_min,
                    x_max,
                    link: None,
                });
            }
            k += len;
        }
    }
    round_extrema(pts, convention, s);
}

fn link_segments(segments: &mut [Segment], max_distance: f32, chosen: &mut Vec<Option<usize>>) {
    for i in 0..segments.len() {
        let mut best: Option<(f32, usize)> = None;
        for j in 0..segments.len() {
            if i == j || segments[i].dir == segments[j].dir {
                continue;
            }
            let (a, b) = (&segments[i], &segments[j]);
            let inward = if a.dir > 0 { b.pos < a.pos } else { b.pos > a.pos };
            if !inward {
                continue;
            }
            let overlap = a.x_max.min(b.x_max) - a.x_min.max(b.x_min);
            if overlap <= 0.0 {
                continue;
            }
            let d = (a.pos - b.pos).abs();
            if d <= max_distance && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, j));
            }
        }
        segments[i].link = best.map(|(_, j)| j);
    }
    chosen.clear();
    chosen.extend(segments.iter().map(|s| s.link));
    for (i, seg) in segments.iter_mut().enumerate() {
        if seg.link.is_some_and(|j| chosen[j] != Some(i)) {
            seg.link = None;
        }
    }
}

fn fit_stem_width(scaled: i32) -> i32 {
    f26dot6::round_to_grid(scaled).max(f26dot6::ONE)
}

fn round_extrema(pts: &AutoPoints, convention: f32, s: &mut Scratch) {
    s.covered.clear();
    s.covered.resize(pts.len(), false);
    for seg in s.segments.iter() {
        let (off, len) = (seg.points.0 as usize, seg.points.1 as usize);
        for k in off..off + len {
            if let Some(slot) = s.covered.get_mut(s.seg_arena[k]) {
                *slot = true;
            }
        }
    }
    for c in 0..pts.contour_ends.len() {
        let Some((start, end)) = pts.contour(c) else { continue };
        let n = end - start;
        if n < 3 {
            continue;
        }
        for i in start..end {
            if s.covered.get(i).copied().unwrap_or(false) {
                continue;
            }
            let prev = start + (i - start + n - 1) % n;
            let next = start + (i - start + 1) % n;
            let (y, yp, yn) = (pts.y[i], pts.y[prev], pts.y[next]);
            let is_min = y < yp && y < yn;
            let is_max = y > yp && y > yn;
            if !is_min && !is_max {
                continue;
            }
            let off = s.seg_arena.len() as u32;
            s.seg_arena.push(i);
            s.segments.push(Segment {
                pos: y,
                points: (off, 1),
                dir: side(pts.x[next] - pts.x[prev], convention, if (yp + yn) * 0.5 < y { 1 } else { -1 }),
                x_min: pts.x[i],
                x_max: pts.x[i],
                link: None,
            });
        }
    }
}

fn compute_edges(tolerance: f32, s: &mut Scratch) {
    s.order.clear();
    s.order.extend(0..s.segments.len());
    s.order.sort_by(|&a, &b| s.segments[a].pos.total_cmp(&s.segments[b].pos));

    s.edges.clear();
    s.seg_next.clear();
    s.seg_next.resize(s.segments.len(), NIL);
    s.owner.clear();
    s.owner.resize(s.segments.len(), NIL);

    for oi in 0..s.order.len() {
        let idx = s.order[oi];
        let (spos, sdir) = (s.segments[idx].pos, s.segments[idx].dir);
        let found = s
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.dir == sdir && (e.pos - spos).abs() <= tolerance)
            .min_by(|(_, a), (_, b)| (a.pos - spos).abs().total_cmp(&(b.pos - spos).abs()))
            .map(|(i, _)| i);
        match found {
            Some(ei) => {
                let e = &mut s.edges[ei];
                let n = e.count as f32;
                e.pos = (e.pos * n + spos) / (n + 1.0);
                let tail = e.tail as usize;
                e.tail = idx as u32;
                e.count += 1;
                s.seg_next[tail] = idx as u32;
                s.owner[idx] = ei as u32;
            }
            None => {
                s.owner[idx] = s.edges.len() as u32;
                s.edges.push(Edge {
                    pos: spos,
                    dir: sdir,
                    head: idx as u32,
                    tail: idx as u32,
                    count: 1,
                    link: None,
                });
            }
        }
    }

    for ei in 0..s.edges.len() {
        let mut seg = s.edges[ei].head;
        let mut link = None;
        while seg != NIL {
            if let Some(l) = s.segments[seg as usize].link {
                let target = s.owner[l];
                if target != NIL && target as usize != ei {
                    link = Some(target as usize);
                    break;
                }
            }
            seg = s.seg_next[seg as usize];
        }
        s.edges[ei].link = link;
    }
}

fn interpolate_untouched(pts: &AutoPoints, out: &mut [i32], touched: &[bool], ppem: u16, upm: u16, anchors: &mut Vec<usize>) {
    let scale = |v: f32| f26dot6::scale(v as i32, ppem, upm);

    for c in 0..pts.contour_ends.len() {
        let Some((start, end)) = pts.contour(c) else { continue };
        let n = end - start;
        if n == 0 {
            continue;
        }
        anchors.clear();
        anchors.extend((start..end).filter(|&i| touched[i]));
        if anchors.is_empty() {
            continue;
        }
        if anchors.len() == 1 {
            let a = anchors[0];
            let delta = out[a] - scale(pts.y[a]);
            for (i, o) in out[start..end].iter_mut().enumerate() {
                let i = start + i;
                if i != a {
                    *o = scale(pts.y[i]) + delta;
                }
            }
            continue;
        }

        for w in 0..anchors.len() {
            let a = anchors[w];
            let b = anchors[(w + 1) % anchors.len()];
            let mut i = start + (a - start + 1) % n;
            while i != b {
                out[i] = interpolate_one(pts, out, i, a, b, scale);
                i = start + (i - start + 1) % n;
            }
        }
    }
}

fn interpolate_one(
    pts: &AutoPoints,
    out: &[i32],
    i: usize,
    a: usize,
    b: usize,
    scale: impl Fn(f32) -> i32,
) -> i32 {
    let (ya, yb, yi) = (pts.y[a], pts.y[b], pts.y[i]);
    let (lo, hi, lo_out, hi_out) = if ya <= yb { (ya, yb, out[a], out[b]) } else { (yb, ya, out[b], out[a]) };

    if yi <= lo {
        lo_out + (scale(yi) - scale(lo))
    } else if yi >= hi {
        hi_out + (scale(yi) - scale(hi))
    } else if hi > lo {
        let t = (yi - lo) / (hi - lo);
        lo_out + ((hi_out - lo_out) as f32 * t) as i32
    } else {
        lo_out
    }
}
