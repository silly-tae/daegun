use std::time::{Duration, Instant};

use daegun::daerizer::daegpu::backend::Backend;
use daegun::daerizer::daegpu::{GpuBatch, Mode, SubpixelParams};

const SIZES: [(u32, u32); 5] = [(64, 64), (256, 256), (512, 512), (1024, 1024), (2048, 1024)];

const ITERS: usize = 200;

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn fit(points: &[(f64, f64)]) -> (f64, f64) {
    let n = points.len() as f64;
    let (sx, sy): (f64, f64) = points.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
    let sxx: f64 = points.iter().map(|(x, _)| x * x).sum();
    let sxy: f64 = points.iter().map(|(x, y)| x * y).sum();
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    (sy / n - slope * sx / n, slope)
}

fn draw_cost<B: Backend>() {
    let Ok(r) = B::new() else {
        eprintln!("{}: no device", B::NAME);
        return;
    };
    let batch = GpuBatch::new();
    let Ok(geometry) = r.geometry(&batch) else {
        eprintln!("{}: geometry failed", B::NAME);
        return;
    };
    let (sub, mode) = (SubpixelParams::default(), Mode::Grayscale);

    eprintln!("{} — empty draw, median of {ITERS}", B::NAME);
    let (mut submit_fit, mut wait_fit, mut read_fit) = (Vec::new(), Vec::new(), Vec::new());

    for (w, h) in SIZES {
        let Ok(mut t) = r.target(w, h) else { continue };
        for _ in 0..8 {
            r.draw(&mut t, &geometry, &[], &sub, mode).expect("warm draw");
        }
        r.read_pixels(&mut t).expect("warm read");

        let mut submit = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let at = Instant::now();
            r.draw(&mut t, &geometry, &[], &sub, mode).expect("draw");
            submit.push(at.elapsed());
        }
        r.wait(&mut t).expect("drain");

        let mut waited = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let at = Instant::now();
            r.draw(&mut t, &geometry, &[], &sub, mode).expect("draw");
            r.wait(&mut t).expect("wait");
            waited.push(at.elapsed());
        }

        let mut read = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let at = Instant::now();
            r.draw(&mut t, &geometry, &[], &sub, mode).expect("draw");
            r.read_pixels(&mut t).expect("read");
            read.push(at.elapsed());
        }

        let (s, wt, rd) = (median(&mut submit), median(&mut waited), median(&mut read));
        let px = f64::from(w) * f64::from(h);
        submit_fit.push((px, s.as_secs_f64() * 1e6));
        wait_fit.push((px, wt.as_secs_f64() * 1e6));
        read_fit.push((px, rd.as_secs_f64() * 1e6));

        eprintln!(
            "  {w:>4}x{h:<4}  submit {:>9.1?}   +wait {:>9.1?}   +read {:>9.1?}   readback {:>9.1?}",
            s, wt, rd, rd.saturating_sub(wt),
        );
    }

    for (label, pts) in [("submit", &submit_fit), ("+wait", &wait_fit), ("+read", &read_fit)] {
        if pts.len() >= 2 {
            let (fixed, slope) = fit(pts);
            eprintln!("  fit {label:<7} {fixed:8.1} us fixed  + {:.3} us per 1000 px", slope * 1000.0);
        }
    }
}

#[test]
#[ignore = "latency measurement, not a pass/fail property"]
fn draw_cost_by_stage() {
    #[cfg(target_vendor = "apple")]
    draw_cost::<daegun::daerizer::daegpu::ffi::Renderer>();
    draw_cost::<daegun::daerizer::daegpu::vk::Renderer>();
    #[cfg(windows)]
    draw_cost::<daegun::daerizer::daegpu::d3d11::Renderer>();
    #[cfg(windows)]
    draw_cost::<daegun::daerizer::daegpu::d3d12::Renderer>();
}

#[test]
#[ignore = "latency measurement, not a pass/fail property"]
fn a_read_is_never_cheaper_than_the_wait_inside_it() {
    fn check<B: Backend>() {
        let Ok(r) = B::new() else { return };
        let batch = GpuBatch::new();
        let Ok(g) = r.geometry(&batch) else { return };
        let Ok(mut t) = r.target(512, 512) else { return };
        let (sub, mode) = (SubpixelParams::default(), Mode::Grayscale);
        for _ in 0..8 {
            r.draw(&mut t, &g, &[], &sub, mode).expect("warm");
        }
        r.read_pixels(&mut t).expect("warm read");

        let mut waited = Vec::new();
        let mut read = Vec::new();
        for _ in 0..64 {
            let at = Instant::now();
            r.draw(&mut t, &g, &[], &sub, mode).expect("draw");
            r.wait(&mut t).expect("wait");
            waited.push(at.elapsed());

            let at = Instant::now();
            r.draw(&mut t, &g, &[], &sub, mode).expect("draw");
            r.read_pixels(&mut t).expect("read");
            read.push(at.elapsed());
        }
        let (w, rd) = (median(&mut waited), median(&mut read));
        eprintln!("{}: wait {w:.1?}  read {rd:.1?}", B::NAME);
        assert!(
            rd * 2 >= w,
            "{}: a read ({rd:?}) came back in under half the wait it contains ({w:?}), so the wait \
             inside it is not happening",
            B::NAME,
        );
    }
    #[cfg(target_vendor = "apple")]
    check::<daegun::daerizer::daegpu::ffi::Renderer>();
    check::<daegun::daerizer::daegpu::vk::Renderer>();
    #[cfg(windows)]
    check::<daegun::daerizer::daegpu::d3d11::Renderer>();
    #[cfg(windows)]
    check::<daegun::daerizer::daegpu::d3d12::Renderer>();
}
