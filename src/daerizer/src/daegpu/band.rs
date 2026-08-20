use alloc::vec::Vec;
use super::extract::Quad;
use super::hull::{self, Hull};

const MAX_BANDS_PER_AXIS: u32 = 16;

#[derive(Clone)]
pub struct Banded {
    pub(crate) bands: Vec<(u32, u32)>,
    pub(crate) band_curves: Vec<u32>,
    pub(crate) bands_per_axis: u32,
    pub(crate) box_min: [f32; 2],
    pub(crate) box_max: [f32; 2],
    pub(crate) hull: Hull,
}

const fn ord_bits(v: f32) -> u32 {
    let b = v.to_bits();
    b ^ ((((b as i32) >> 31) as u32) | 0x8000_0000)
}

fn extent(c: &Quad, axis: usize) -> (f32, f32) {
    let (a, b, d) = (c[0][axis], c[1][axis], c[2][axis]);
    (a.min(b).min(d), a.max(b).max(d))
}

struct Span {
    min: [f32; 2],
    max: [f32; 2],
    flat: [bool; 2],
}

fn span_of(c: &Quad) -> Span {
    let mut s = Span { min: [0.0; 2], max: [0.0; 2], flat: [false; 2] };
    for axis in 0..2 {
        let (min, max) = extent(c, axis);
        s.min[axis] = min;
        s.max[axis] = max;
        s.flat[axis] = c[0][axis] == c[1][axis] && c[1][axis] == c[2][axis];
    }
    s
}

pub(crate) fn build(curves: &[Quad]) -> Option<Banded> {
    if curves.is_empty() || curves.len() > u32::MAX as usize {
        return None;
    }

    let mut box_min = [f32::MAX; 2];
    let mut box_max = [f32::MIN; 2];
    for c in curves {
        for p in c {
            for axis in 0..2 {
                box_min[axis] = box_min[axis].min(p[axis]);
                box_max[axis] = box_max[axis].max(p[axis]);
            }
        }
    }

    let n = (curves.len() as u32).isqrt().clamp(1, MAX_BANDS_PER_AXIS);

    let spans: Vec<Span> = curves.iter().map(span_of).collect();

    let mut bands = Vec::with_capacity(n as usize * 2);
    let mut packed: Vec<u64> = Vec::with_capacity(curves.len() * 2);

    for (slice_axis, ray_axis) in [(1usize, 0usize), (0, 1)] {
        let span = box_max[slice_axis] - box_min[slice_axis];
        for b in 0..n {
            let lo = box_min[slice_axis] + span * b as f32 / n as f32;
            let hi = box_min[slice_axis] + span * (b + 1) as f32 / n as f32;

            let first = packed.len();
            for (i, s) in spans.iter().enumerate() {
                // A curve flat along the slice axis runs parallel to this band's ray and so cannot
                // cross it: its three coordinates share a sign, which is code 0 or 14, and 0x2E74
                // answers 0 at both. Dropping it costs the shader nothing but an iteration saved.
                if s.flat[slice_axis] {
                    continue;
                }
                if s.max[slice_axis] >= lo && s.min[slice_axis] <= hi {
                    // Packed as far-edge-then-index so the sort below is a plain integer sort;
                    // inverting the key turns descending on the far edge into ascending.
                    let far = !ord_bits(s.max[ray_axis]);
                    packed.push((u64::from(far) << 32) | i as u64);
                }
            }

            // Far end first is the contract the shader's early exit rests on: once one curve ends
            // behind the pixel every later one does. Unstable is free here – the index tie-break
            // makes no two entries compare equal, so a stable sort would allocate to preserve nothing.
            packed.get_mut(first..)?.sort_unstable();

            let count = packed.len() - first;
            bands.push((first as u32, count as u32));
        }
    }

    let band_curves: Vec<u32> = packed.iter().map(|&k| k as u32).collect();
    let hull = hull::build(curves, box_min, box_max);
    Some(Banded { bands, band_curves, bands_per_axis: n, box_min, box_max, hull })
}
