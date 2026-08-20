use daegun::{Font, GpuBatch, RasterOptions};
use daegun::eval;

const SUB: usize = 16;
const TRAVEL: usize = 3;
const SAMPLES: usize = SUB * TRAVEL;
const HZ: f64 = 60.0;

fn gamma_stage(t: f64, n: u32, tau: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    let ln_fact: f64 = (2..n).map(|k| f64::from(k).ln()).sum();
    let x = t / tau;
    ((f64::from(n - 1)) * x.ln() - x - ln_fact - tau.ln()).exp()
}

fn watson_filter(frame_period: f64) -> Vec<f64> {
    const N1: u32 = 9;
    const N2: u32 = 10;
    const KAPPA: f64 = 1.33;
    const ZETA: f64 = 0.9;
    const TAU: f64 = 0.0043;

    let taps = (0.300 / frame_period).ceil() as usize;
    let mut h: Vec<f64> = (0..taps)
        .map(|k| {
            let t = k as f64 * frame_period;
            gamma_stage(t, N1, TAU) - ZETA * gamma_stage(t, N2, KAPPA * TAU)
        })
        .collect();
    let norm: f64 = h.iter().map(|v| v.abs()).sum();
    assert!(norm > 0.0, "degenerate filter");
    for v in &mut h {
        *v /= norm;
    }
    h
}

fn roughness(series: &[f32], h: &[f64]) -> f64 {
    if series.len() < 3 {
        return 0.0;
    }
    let d2: Vec<f32> = (1..series.len() - 1)
        .map(|k| series[k + 1] - 2.0 * series[k] + series[k - 1])
        .collect();
    respond(&d2, h)
}

fn respond(series: &[f32], h: &[f64]) -> f64 {
    let (mut sum_sq, mut n) = (0.0f64, 0u64);
    for k in h.len()..series.len() {
        let acc: f64 = h
            .iter()
            .enumerate()
            .map(|(j, hj)| hj * f64::from(series[k - j]))
            .sum();
        sum_sq += (acc * 255.0).powi(2);
        n += 1;
    }
    if n == 0 { 0.0 } else { (sum_sq / n as f64).sqrt() }
}

#[test]
fn the_temporal_filter_is_bandpass_and_peaks_in_the_flicker_band() {
    let h = watson_filter(1.0 / HZ);
    let dc: f64 = h.iter().sum();
    let abs: f64 = h.iter().map(|v| v.abs()).sum();

    let at = |f: f64| -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (k, hk) in h.iter().enumerate() {
            let ph = -2.0 * core::f64::consts::PI * f * k as f64 / HZ;
            re += hk * ph.cos();
            im += hk * ph.sin();
        }
        (re * re + im * im).sqrt()
    };

    let (mut peak_f, mut peak_v) = (0.0, 0.0);
    let mut f = 0.25;
    while f < 30.0 {
        if at(f) > peak_v {
            peak_v = at(f);
            peak_f = f;
        }
        f += 0.25;
    }

    assert!(dc.abs() / abs < 0.15, "not bandpass: DC gain is {} of unit", dc / abs);
    assert!(
        (4.0..=20.0).contains(&peak_f),
        "peak at {peak_f} Hz falls outside the 5-20 Hz flicker band"
    );
    assert!(at(1.0) < peak_v * 0.5, "1 Hz is not attenuated: {} of peak", at(1.0) / peak_v);
}

fn cpu_series(font: &Font, gid: u16, ppem: f32, upm: f32) -> Vec<Vec<f32>> {
    let mut frames = Vec::with_capacity(SAMPLES);
    for s in 0..SAMPLES {
        let dy = s as f32 / SUB as f32 * upm / ppem;
        let opts = RasterOptions::default().with_transform([1.0, 0.0, 0.0, 1.0, 0.0, dy]);
        match font.rasterize_glyph_with(gid, ppem, &[], &opts) {
            Some(g) if g.metrics.width > 0 && g.metrics.height > 0 => frames.push(g),
            _ => return Vec::new(),
        }
    }

    let x0 = frames.iter().map(|f| f.metrics.xmin).min().unwrap_or(0);
    let y0 = frames.iter().map(|f| f.metrics.ymin).min().unwrap_or(0);
    let x1 = frames.iter().map(|f| f.metrics.xmin + f.metrics.width as i32).max().unwrap_or(0);
    let y1 = frames.iter().map(|f| f.metrics.ymin + f.metrics.height as i32).max().unwrap_or(0);
    let (gw, gh) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
    if gw == 0 || gh == 0 || gw * gh > 4_000 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for gy in 0..gh {
        for gx in 0..gw {
            let mut series = vec![0.0f32; SAMPLES];
            let mut active = false;
            for (s, f) in frames.iter().enumerate() {
                let fx = x0 + gx as i32 - f.metrics.xmin;
                let fy = (f.metrics.ymin + f.metrics.height as i32 - 1) - (y0 + gy as i32);
                let v = if fx >= 0
                    && fy >= 0
                    && (fx as usize) < f.metrics.width
                    && (fy as usize) < f.metrics.height
                {
                    f32::from(f.bitmap[fy as usize * f.metrics.width + fx as usize]) / 255.0
                } else {
                    0.0
                };
                series[s] = v;
                if v > 0.0 && v < 1.0 {
                    active = true;
                }
            }
            if active {
                out.push(series);
            }
        }
    }
    out
}

fn gpu_series(font: &Font, gid: u16, ppem: f32) -> Vec<Vec<f32>> {
    let mut batch = GpuBatch::new();
    let Ok(slot) = font.gpu_glyph(&mut batch, gid, &[]) else { return Vec::new() };
    let px_em = 1.0 / ppem;
    let lo = [slot.box_min[0] - px_em, slot.box_min[1] - px_em];
    let hi = [slot.box_max[0] + px_em, slot.box_max[1] + px_em];
    let (w, h) = (
        ((hi[0] - lo[0]) * ppem).ceil() as i32,
        ((hi[1] - lo[1]) * ppem).ceil() as i32,
    );
    if w <= 0 || h <= 0 || w * h > 4_000 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for py in 0..h {
        for px in 0..w {
            let mut series = vec![0.0f32; SAMPLES];
            let mut active = false;
            for (s, slot_v) in series.iter_mut().enumerate() {
                let shift = s as f32 / SUB as f32 * px_em;
                let at = [
                    lo[0] + (px as f32 + 0.5) * px_em,
                    lo[1] + (py as f32 + 0.5) * px_em + shift,
                ];
                let v = (eval::coverage(&batch, &slot, at, [ppem, ppem]) * 255.0).round() / 255.0;
                *slot_v = v;
                if v > 0.0 && v < 1.0 {
                    active = true;
                }
            }
            if active {
                out.push(series);
            }
        }
    }
    out
}

#[test]
fn the_two_paths_stay_equally_steady_under_motion() {
    let bytes = std::fs::read(format!("{}/inter/InterVariable.ttf", super::FONTS)).expect("read");
    let font = Font::from_bytes(&bytes).expect("parse");
    let upm = f32::from(font.upm());
    let filt = watson_filter(1.0 / HZ);
    assert!(SAMPLES > filt.len() * 2, "sweep is too short for a {}-tap filter", filt.len());

    let ppem = 16.0f32;
    let (mut cpu, mut gpu) = (Vec::new(), Vec::new());
    for gid in (1..120u16).step_by(17) {
        for s in cpu_series(&font, gid, ppem, upm) {
            cpu.push(roughness(&s, &filt));
        }
        for s in gpu_series(&font, gid, ppem) {
            gpu.push(roughness(&s, &filt));
        }
    }

    assert!(cpu.len() > 200, "only {} CPU pixels reached the check", cpu.len());
    assert!(gpu.len() > 200, "only {} GPU pixels reached the check", gpu.len());

    let cm = cpu.iter().sum::<f64>() / cpu.len() as f64;
    let gm = gpu.iter().sum::<f64>() / gpu.len() as f64;
    let ratio = gm / cm;

    assert!(
        ratio <= 8.0,
        "the GPU path drifted away from the scanline one: CPU {cm:.3}, GPU {gm:.3}, ratio {ratio:.2}"
    );
    assert!(
        cm < 2.0 && gm < 4.0,
        "a path got jumpy under motion: CPU {cm:.3}, GPU {gm:.3} levels of 255"
    );
}
