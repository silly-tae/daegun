#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use super::{Band, GlyphSlot, GpuBatch, SubpixelParams, MAX_SUBPIXEL_TAPS, MAX_SUBPIXEL_WEIGHTS,
            MAX_SUPERSAMPLE};

fn root_contribution(y1: f32, y2: f32, y3: f32) -> u32 {
    let sign_code = (if y1 > 0.0 { 2 } else { 0 })
        + (if y2 > 0.0 { 4 } else { 0 })
        + (if y3 > 0.0 { 8 } else { 0 });
    (0x2E74u32 >> sign_code) & 3
}

fn curve_x(p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> f32 {
    let u = 1.0 - t;
    u * u * p1[0] + 2.0 * t * u * p2[0] + t * t * p3[0]
}

fn quarter_turn(p: [f32; 2]) -> [f32; 2] {
    [p[1], -p[0]]
}

fn curve_roots(p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) -> Option<(u32, f32, f32)> {
    let contribution = root_contribution(p1[1], p2[1], p3[1]);
    if contribution == 0 {
        return None;
    }

    let a = p1[1] - 2.0 * p2[1] + p3[1];
    let b = p1[1] - p2[1];
    let c = p1[1];

    let (t1, t2) = if a.abs() < 1.0e-5 {
        // Zero is the exact answer, not a fallback: `b` vanishing while the contribution does not
        // forces y1 = y2 = 0, so `c` is zero too and t = 0 solves it. This used to divide by `b`.
        let t = if b != 0.0 { c / (2.0 * b) } else { 0.0 };
        (t, t)
    } else {
        // Clamping the discriminant is part of the method, not a NaN guard: the classes where y1
        // and y3 agree but y2 differs can have no real root, and t1 = t2 = b/a cancels them exactly.
        let root = (b * b - a * c).max(0.0).sqrt();
        ((b - root) / a, (b + root) / a)
    };
    Some((contribution, t1, t2))
}

fn curve_coverage_at(
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
    contribution: u32,
    t1: f32,
    t2: f32,
    pixels_per_em: f32,
) -> [f32; 2] {
    let mut coverage = 0.0;
    let mut weight = 0.0f32;
    if contribution & 1 != 0 {
        let x = pixels_per_em * curve_x(p1, p2, p3, t1);
        coverage += (x + 0.5).clamp(0.0, 1.0);
        weight = weight.max((1.0 - 2.0 * x.abs()).clamp(0.0, 1.0));
    }
    if contribution & 2 != 0 {
        let x = pixels_per_em * curve_x(p1, p2, p3, t2);
        coverage -= (x + 0.5).clamp(0.0, 1.0);
        weight = weight.max((1.0 - 2.0 * x.abs()).clamp(0.0, 1.0));
    }
    [coverage, weight]
}

fn combine(h: [f32; 2], v: [f32; 2]) -> f32 {
    // Not dead code, though it cannot raise coverage – `floor` is the smaller magnitude and
    // `picked` is one of the two rays. What it buys is a NaN: `f32::max` returns the other operand,
    // so one ray going NaN is replaced rather than poisoning the pixel. Both NaN still gives NaN.
    let floor = h[0].abs().min(v[0].abs());
    let picked = if h[1] >= v[1] { h[0] } else { v[0] };
    picked.abs().max(floor)
}

fn curve_coverage(p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], pixels_per_em: f32) -> [f32; 2] {
    match curve_roots(p1, p2, p3) {
        None => [0.0, 0.0],
        Some((contribution, t1, t2)) => {
            curve_coverage_at(p1, p2, p3, contribution, t1, t2, pixels_per_em)
        }
    }
}

fn band_index(t: f32, lo: f32, hi: f32, count: u32) -> u32 {
    if count == 0 {
        return 0;
    }
    let span = (hi - lo).max(1.0e-6);
    let f = (t - lo) / span * count as f32;
    f.clamp(0.0, (count - 1) as f32) as u32
}

fn scan_band(
    batch: &GpuBatch,
    band: u32,
    origin: [f32; 2],
    pixels_per_em: f32,
    transpose: bool,
) -> [f32; 2] {
    let Some(&Band { first_curve, curve_count }) = batch.bands().get(band as usize) else {
        return [0.0, 0.0];
    };

    let mut coverage = 0.0;
    let mut weight = 0.0f32;
    for i in 0..curve_count {
        let Some(slot) = first_curve.checked_add(i) else { break };
        let Some(&curve) = batch.band_curves().get(slot as usize) else { break };
        let Some(base) = (curve as usize).checked_mul(3) else { break };
        let Some(points) = batch.curves().get(base..base + 3) else { break };

        let mut p = [[0.0f32; 2]; 3];
        for (k, q) in points.iter().enumerate() {
            p[k] = [q.x - origin[0], q.y - origin[1]];
            if transpose {
                p[k] = quarter_turn(p[k]);
            }
        }

        if p[0][0].max(p[1][0]).max(p[2][0]) * pixels_per_em < -0.5 {
            break;
        }

        let c = curve_coverage(p[0], p[1], p[2], pixels_per_em);
        coverage += c[0];
        weight = weight.max(c[1]);
    }

    [coverage, weight]
}

fn scan_band_row(
    batch: &GpuBatch,
    band: u32,
    y_origin: f32,
    x_origins: &[f32],
    pixels_per_em: f32,
    out: &mut [f32],
    out_weight: &mut [f32],
) {
    let Some(&Band { first_curve, curve_count }) = batch.bands().get(band as usize) else {
        return;
    };
    let n = x_origins.len().min(out.len()).min(out_weight.len());
    let mut running = n;
    let mut stopped = [false; MAX_ROW];
    if n == 0 {
        return;
    }

    for i in 0..curve_count {
        if running == 0 {
            break;
        }
        let Some(slot) = first_curve.checked_add(i) else { break };
        let Some(&curve) = batch.band_curves().get(slot as usize) else { break };
        let Some(base) = (curve as usize).checked_mul(3) else { break };
        let Some(points) = batch.curves().get(base..base + 3) else { break };

        let ys = [points[0].y - y_origin, points[1].y - y_origin, points[2].y - y_origin];
        let roots = curve_roots([0.0, ys[0]], [0.0, ys[1]], [0.0, ys[2]]);

        for (k, &x_origin) in x_origins.iter().enumerate().take(n) {
            if stopped[k] {
                continue;
            }
            let p1 = [points[0].x - x_origin, ys[0]];
            let p2 = [points[1].x - x_origin, ys[1]];
            let p3 = [points[2].x - x_origin, ys[2]];

            if p1[0].max(p2[0]).max(p3[0]) * pixels_per_em < -0.5 {
                stopped[k] = true;
                running -= 1;
                continue;
            }
            if let Some((contribution, t1, t2)) = roots {
                let c = curve_coverage_at(p1, p2, p3, contribution, t1, t2, pixels_per_em);
                out[k] += c[0];
                if let Some(w) = out_weight.get_mut(k) {
                    *w = w.max(c[1]);
                }
            }
        }
    }
}

const MAX_ROW: usize = MAX_SUBPIXEL_TAPS as usize * MAX_SUPERSAMPLE as usize;

pub fn winding(batch: &GpuBatch, slot: &GlyphSlot, em_coord: [f32; 2], em_pixels: [f32; 2]) -> f32 {
    let h = slot
        .band_base
        .saturating_add(band_index(em_coord[1], slot.box_min[1], slot.box_max[1], slot.h_bands));
    let v = slot
        .band_base
        .saturating_add(slot.h_bands)
        .saturating_add(band_index(em_coord[0], slot.box_min[0], slot.box_max[0], slot.v_bands));

    combine(
        scan_band(batch, h, em_coord, em_pixels[0], false),
        scan_band(batch, v, em_coord, em_pixels[1], true),
    )
}

pub fn coverage(batch: &GpuBatch, slot: &GlyphSlot, em_coord: [f32; 2], em_pixels: [f32; 2]) -> f32 {
    winding(batch, slot, em_coord, em_pixels).min(1.0)
}

fn tap_offset(tap: u32, origin: i32, oversample: u32, em_pixels: f32) -> f32 {
    if oversample == 0 || em_pixels == 0.0 {
        return 0.0;
    }
    let centre = (origin as f32 + tap as f32 + 0.5) / oversample as f32;
    (centre - 0.5) / em_pixels
}

pub fn coverage_channels(
    batch: &GpuBatch,
    slot: &GlyphSlot,
    em_coord: [f32; 2],
    em_pixels: [f32; 2],
    params: &SubpixelParams,
) -> [f32; 3] {
    let (ox, oy) = (params.oversample[0].max(1), params.oversample[1].max(1));
    let taps_x = params.taps[0].clamp(1, MAX_SUBPIXEL_TAPS);
    let taps_y = params.taps[1].clamp(1, MAX_SUBPIXEL_TAPS);

    let sample_m = [em_pixels[0] * ox as f32, em_pixels[1] * oy as f32];

    let ss = params.supersample.clamp(1, MAX_SUPERSAMPLE);
    let inv_ss = 1.0 / (ss * ss) as f32;

    let jitter = |i: u32, m: f32| -> f32 {
        if ss == 1 || m == 0.0 || !m.is_finite() {
            return 0.0;
        }
        ((i as f32 + 0.5) / ss as f32 - 0.5) / m
    };

    let mut out = [0.0f32; 3];
    for ty in 0..taps_y {
        let dy = -tap_offset(ty, params.origin[1], oy, em_pixels[1]);

        let mut tap_cov = [0.0f32; MAX_SUBPIXEL_TAPS as usize];

        for sy in 0..ss {
            let y = em_coord[1] + dy + jitter(sy, sample_m[1]);

            let mut xs = [0.0f32; MAX_ROW];
            let mut count = 0usize;
            for tx in 0..taps_x {
                let dx = tap_offset(tx, params.origin[0], ox, em_pixels[0]);
                for sx in 0..ss {
                    if count >= MAX_ROW {
                        break;
                    }
                    xs[count] = em_coord[0] + dx + jitter(sx, sample_m[0]);
                    count += 1;
                }
            }

            let mut horizontal = [0.0f32; MAX_ROW];
            let mut horizontal_w = [0.0f32; MAX_ROW];
            let hi = band_index(y, slot.box_min[1], slot.box_max[1], slot.h_bands);
            scan_band_row(
                batch,
                slot.band_base.saturating_add(hi),
                y,
                &xs[..count],
                sample_m[0],
                &mut horizontal[..count],
                &mut horizontal_w[..count],
            );

            for (k, &x) in xs.iter().enumerate().take(count) {
                let vi = band_index(x, slot.box_min[0], slot.box_max[0], slot.v_bands);
                let v = scan_band(
                    batch,
                    slot.band_base.saturating_add(slot.h_bands).saturating_add(vi),
                    [x, y],
                    sample_m[1],
                    true,
                );
                if let Some(acc) = tap_cov.get_mut(k / ss as usize) {
                    *acc += combine([horizontal[k], horizontal_w[k]], v).min(1.0);
                }
            }
        }

        for (tx, &acc) in tap_cov.iter().enumerate().take(taps_x as usize) {
            let cov = acc * inv_ss;
            let Some(index) = ty.checked_mul(taps_x).and_then(|r| r.checked_add(tx as u32)) else {
                continue;
            };
            for (c, slot_out) in out.iter_mut().enumerate() {
                if c as u32 >= params.channels {
                    break;
                }
                let w = params
                    .weights
                    .get(c * MAX_SUBPIXEL_WEIGHTS + index as usize)
                    .copied()
                    .unwrap_or(0.0);
                *slot_out += w * cov;
            }
        }
    }

    if params.channels < 2 {
        out = [out[0]; 3];
    }

    for c in out.iter_mut() {
        *c = c.clamp(0.0, 1.0);
    }
    out
}

#[cfg(test)]
mod combine_contract {
    use super::combine;

    #[test]
    fn the_result_is_a_magnitude() {
        for h0 in [-3.0f32, -1.0, -0.25, 0.0, 0.25, 1.0] {
            for v0 in [-3.0f32, -1.0, -0.25, 0.0, 0.25, 1.0] {
                for (hw, vw) in [(1.0f32, 0.0f32), (0.0, 1.0), (0.5, 0.5)] {
                    let got = combine([h0, hw], [v0, vw]);
                    assert!(
                        got >= 0.0,
                        "combine([{h0}, {hw}], [{v0}, {vw}]) = {got}, documented as a magnitude",
                    );
                    let picked = if hw >= vw { h0 } else { v0 };
                    assert_eq!(
                        got,
                        picked.abs().max(h0.abs().min(v0.abs())),
                        "combine([{h0}, {hw}], [{v0}, {vw}]) is not the picked magnitude floored \
                         by the smaller one",
                    );
                }
            }
        }
    }

    #[test]
    fn a_tie_takes_the_horizontal_ray() {
        assert_eq!(combine([0.25, 0.5], [0.75, 0.5]), 0.25, "a tie did not take the horizontal ray");
        assert_eq!(combine([0.75, 0.5], [0.25, 0.5]), 0.75, "a tie did not take the horizontal ray");
        assert_eq!(combine([0.25, 0.4], [0.75, 0.6]), 0.75, "the better-placed ray lost");
        assert_eq!(combine([0.75, 0.6], [0.25, 0.4]), 0.75, "the better-placed ray lost");
    }

    #[test]
    fn the_floor_is_unreachable_for_finite_rays() {
        let vals = [-1.0e9f32, -3.0, -1.0, -0.5, -1.0e-9, 0.0, 1.0e-9, 0.5, 1.0, 3.0, 1.0e9];
        let ws = [0.0f32, 0.25, 0.5, 0.75, 1.0];
        let mut cases = 0usize;
        for &h0 in &vals {
            for &v0 in &vals {
                for &hw in &ws {
                    for &vw in &ws {
                        let got = combine([h0, hw], [v0, vw]);
                        let picked = if hw >= vw { h0 } else { v0 };
                        assert_eq!(
                            got.to_bits(), picked.abs().to_bits(),
                            "combine([{h0}, {hw}], [{v0}, {vw}]) = {got}, but the floor is the \
                             smaller magnitude and cannot exceed |picked| = {}",
                            picked.abs(),
                        );
                        cases += 1;
                    }
                }
            }
        }
        assert_eq!(cases, 3_025, "the grid changed size; the claim is about all of it");
    }

    #[test]
    fn one_ray_going_nan_is_replaced_rather_than_propagated() {
        let nan = f32::NAN;
        assert_eq!(combine([nan, 1.0], [0.5, 0.0]), 0.5, "a NaN on the picked ray propagated");
        assert_eq!(combine([0.5, 1.0], [nan, 0.0]), 0.5, "a NaN on the rejected ray propagated");
        assert_eq!(combine([nan, 0.0], [-0.25, 1.0]), 0.25, "a NaN survived the floor");

        assert!(combine([nan, 1.0], [nan, 0.0]).is_nan(), "two NaN rays produced a finite answer");
    }
}
