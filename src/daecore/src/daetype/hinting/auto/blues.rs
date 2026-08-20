use alloc::vec::Vec;

use super::points::{AutoPoints, ON_CURVE};

const ZONE_SPECS: &[(&str, bool)] = &[
    ("THEZOCQS", true),
    ("HEZLOCUS", false),
    ("fijkdbh", true),
    ("xzroesc", true),
    ("xzroesc", false),
    ("pqgjy", false),
];

#[derive(Clone, Copy, Debug)]
pub struct BlueZone {
    pub reference: f32,
    pub overshoot: f32,
    pub is_top: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BlueZones {
    pub zones: Vec<BlueZone>,
}

impl BlueZones {
    pub fn nearest(&self, pos: f32, is_top: bool, tolerance: f32) -> Option<&BlueZone> {
        let mut best: Option<(f32, &BlueZone)> = None;
        for z in &self.zones {
            if z.is_top != is_top {
                continue;
            }
            let d = (z.reference - pos).abs().min((z.overshoot - pos).abs());
            if d <= tolerance && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, z));
            }
        }
        best.map(|(_, z)| z)
    }
}

pub fn compute(
    resolve: &mut dyn FnMut(char) -> Option<u16>,
    outline_of: &mut dyn FnMut(u16) -> Option<AutoPoints>,
) -> BlueZones {
    let mut zones = Vec::new();
    for &(chars, is_top) in ZONE_SPECS {
        let mut flats = Vec::new();
        let mut rounds = Vec::new();

        for ch in chars.chars() {
            let Some(gid) = resolve(ch) else { continue };
            let Some(pts) = outline_of(gid) else { continue };
            let Some((y, flat)) = extremum(&pts, is_top) else { continue };
            if flat { flats.push(y) } else { rounds.push(y) }
        }

        if flats.is_empty() && rounds.is_empty() {
            continue;
        }
        let reference = median(&mut flats).unwrap_or_else(|| median(&mut rounds).unwrap_or(0.0));
        let overshoot = median(&mut rounds).unwrap_or(reference);
        zones.push(BlueZone { reference, overshoot, is_top });
    }
    BlueZones { zones }
}

fn extremum(pts: &AutoPoints, is_top: bool) -> Option<(f32, bool)> {
    let mut best: Option<(f32, usize)> = None;
    for i in 0..pts.len() {
        if pts.flags[i] & ON_CURVE == 0 {
            continue;
        }
        let y = pts.y[i];
        let better = match best {
            None => true,
            Some((by, _)) => if is_top { y > by } else { y < by },
        };
        if better {
            best = Some((y, i));
        }
    }
    let (y, idx) = best?;

    let (start, end) = (0..pts.contour_ends.len())
        .find_map(|c| pts.contour(c).filter(|&(s, e)| idx >= s && idx < e))?;
    let n = end - start;
    if n < 2 {
        return Some((y, false));
    }

    let prev = start + (idx - start + n - 1) % n;
    let next = start + (idx - start + 1) % n;
    let round = pts.flags[prev] & ON_CURVE == 0 || pts.flags[next] & ON_CURVE == 0;
    Some((y, !round))
}

fn median(v: &mut [f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}
