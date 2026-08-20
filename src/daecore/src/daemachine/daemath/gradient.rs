use alloc::vec::Vec;
#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use super::matrix::invert;
use super::{resolve_stops, Extend, Gradient, GradientKind, Rgba, Stop, Stops};

pub enum Ramp {
    Flat(Option<Rgba>),
    Varying { kind: GradientKind, stops: Vec<Stop>, extend: Extend, inverse: [f64; 6] },
}

impl Ramp {
    pub fn new(g: &Gradient, to_device: &[f64; 6]) -> Ramp {
        let stops = match resolve_stops(g.stops.clone()) {
            Stops::Nothing => return Ramp::Flat(None),
            Stops::Solid(c) => return Ramp::Flat(Some(c)),
            Stops::Many(s) => s,
        };
        let Some(inverse) = invert(to_device) else { return Ramp::Flat(None) };
        let degenerate = match g.kind {
            GradientKind::Linear { x0, y0, x1, y1 } => x0 == x1 && y0 == y1,
            GradientKind::Radial { x0, y0, r0, x1, y1, r1 } => x0 == x1 && y0 == y1 && r0 == r1,
            GradientKind::Sweep { .. } => false,
        };
        if degenerate {
            return Ramp::Flat(stops.last().map(|s| s.color));
        }
        Ramp::Varying { kind: g.kind, stops, extend: g.extend, inverse }
    }

    pub fn at(&self, dx: f64, dy: f64) -> Option<Rgba> {
        let (kind, stops, extend, inv) = match self {
            Ramp::Flat(c) => return *c,
            Ramp::Varying { kind, stops, extend, inverse } => (kind, stops, *extend, inverse),
        };
        let (px, py) = (dx + 0.5, dy + 0.5);
        let gx = inv[0] * px + inv[2] * py + inv[4];
        let gy = inv[1] * px + inv[3] * py + inv[5];

        let t = match *kind {
            GradientKind::Linear { x0, y0, x1, y1 } => {
                let (ax, ay) = (f64::from(x1 - x0), f64::from(y1 - y0));
                let len2 = ax * ax + ay * ay;
                ((gx - f64::from(x0)) * ax + (gy - f64::from(y0)) * ay) / len2
            }
            GradientKind::Radial { x0, y0, r0, x1, y1, r1 } => {
                radial_t(gx, gy, x0.into(), y0.into(), r0.into(), x1.into(), y1.into(), r1.into())?
            }
            GradientKind::Sweep { cx, cy, start_angle, end_angle } => {
                sweep_t(gx, gy, cx.into(), cy.into(), start_angle.into(), end_angle.into())?
            }
        };
        if !t.is_finite() {
            return None;
        }
        Some(sample(stops, apply_extend(t, extend)))
    }
}

#[allow(clippy::too_many_arguments)]
fn radial_t(px: f64, py: f64, x0: f64, y0: f64, r0: f64, x1: f64, y1: f64, r1: f64) -> Option<f64> {
    let (cdx, cdy, dr) = (x1 - x0, y1 - y0, r1 - r0);
    let (pdx, pdy) = (px - x0, py - y0);
    let a = cdx * cdx + cdy * cdy - dr * dr;
    let b = pdx * cdx + pdy * cdy + r0 * dr;
    let c = pdx * pdx + pdy * pdy - r0 * r0;

    if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            return None;
        }
        let s = c / (2.0 * b);
        return (r0 + s * dr >= 0.0).then_some(s);
    }
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let (s1, s2) = ((b + root) / a, (b - root) / a);
    [s1.max(s2), s1.min(s2)].into_iter().find(|&s| r0 + s * dr >= 0.0)
}

fn sweep_t(px: f64, py: f64, cx: f64, cy: f64, start: f64, end: f64) -> Option<f64> {
    let span = end - start;
    if span == 0.0 {
        return None;
    }
    let deg = crate::daecore::daemachine::float::atan2(py - cy, px - cx) * 180.0 / core::f64::consts::PI;
    let deg = deg - 360.0 * (deg / 360.0).floor();
    Some((deg - start) / span)
}

fn apply_extend(t: f64, extend: Extend) -> f64 {
    match extend {
        Extend::Pad => t.clamp(0.0, 1.0),
        Extend::Repeat => t - t.floor(),
        Extend::Reflect => {
            let f = t - 2.0 * (t / 2.0).floor();
            if f > 1.0 { 2.0 - f } else { f }
        }
    }
}

fn sample(stops: &[Stop], t: f64) -> Rgba {
    let first = stops[0];
    let last = stops[stops.len() - 1];
    if t <= f64::from(first.offset) {
        return first.color;
    }
    if t >= f64::from(last.offset) {
        return last.color;
    }
    for w in stops.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (oa, ob) = (f64::from(a.offset), f64::from(b.offset));
        if t < oa || t > ob {
            continue;
        }
        if ob <= oa {
            return b.color;
        }
        let f = (t - oa) / (ob - oa);
        return Rgba {
            r: lerp(a.color.r, b.color.r, f),
            g: lerp(a.color.g, b.color.g, f),
            b: lerp(a.color.b, b.color.b, f),
            a: lerp(a.color.a, b.color.a, f),
        };
    }
    last.color
}

fn lerp(a: u8, b: u8, f: f64) -> u8 {
    (f64::from(a) + (f64::from(b) - f64::from(a)) * f).clamp(0.0, 255.0) as u8
}
