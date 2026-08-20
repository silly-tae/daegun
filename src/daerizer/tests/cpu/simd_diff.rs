use daegun::daerizer::daecpu::math::{Geometry, Glyph};
use daegun::daerizer::daecpu::rasterize::{metrics_raw, Raster};

fn scalar_coverage(a: &mut [f32], length: usize) {
    let mut height = 0.0f32;
    for slot in a.iter_mut().take(length) {
        height += *slot;
        *slot = f32::from_bits((height).to_bits() & 0x7fff_ffff).clamp(0.0, 1.0);
    }
}

fn scalar_bitmap(a: &[f32], length: usize) -> Vec<u8> {
    let mut height = 0.0f32;
    a.iter()
        .take(length)
        .map(|d| {
            height += d;
            (f32::from_bits(height.to_bits() & 0x7fff_ffff) * 255.9).clamp(0.0, 255.0) as u8
        })
        .collect()
}

fn accumulator(rel: &str, ch: char, px: f32) -> (Vec<f32>, usize) {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let map = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("parses");
    let head = map.get("head").expect("head");
    let fmt = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("fmt");
    let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
    let n = daegun::daecore::daetype::decoder::read_u16_be(map.get("maxp").expect("maxp"), 4).expect("n");
    let loca = daegun::daecore::daetype::instancer::parse_loca(&map, fmt, n as usize).expect("loca");
    let gid = daegun::daecore::daetype::subsetter::cmap_glyph_id(map.get("cmap").expect("cmap"), ch as u32)
        .unwrap_or_else(|| panic!("no {ch}"));

    let mut g = Geometry::new(px, upm);
    daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&map, &loca, gid, &mut g).expect("draws");
    let mut glyph = Glyph::default();
    g.finalize(&mut glyph);

    let scale = px / upm;
    let (m, ox, oy) = metrics_raw(scale, glyph.bounds, 0.0, 0.0, 0.0);
    let mut raster = Raster::new(m.width, m.height);
    raster.draw(&glyph, scale, scale, ox, oy);
    let n = m.width * m.height;
    (raster.into_coverage(daegun::daecore::daetype::outline::FillRule::NonZero), n)
}

#[test]
#[ignore]
fn how_far_the_vector_prefix_sum_moves_the_answer() {
    eprintln!("  backend: {}", daegun::daerizer::daecpu::simd::BACKEND);
    let mut worst_cov = 0.0f32;
    let mut bytes_differing = 0usize;
    let mut bytes_total = 0usize;
    let mut worst_byte = 0i32;

    for (ch, px) in [('B', 16.0f32), ('B', 64.0), ('g', 12.0), ('W', 128.0), ('@', 32.0), ('o', 48.0)] {
        let (cov_scalar, n) = accumulator("eb-garamond/EBGaramond.ttf", ch, px);
        let (raw, _) = {
            let (c, n2) = accumulator("eb-garamond/EBGaramond.ttf", ch, px);
            (c, n2)
        };
        let _ = raw;

        let mut deltas = vec![0.0f32; n];
        let mut state = 0x9E3779B97F4A7C15u64;
        for (i, d) in deltas.iter_mut().enumerate() {
            state ^= state << 13; state ^= state >> 7; state ^= state << 17;
            if state.is_multiple_of(5) {
                let u = (state >> 11) as f32 / (1u64 << 53) as f32;
                *d = (u - 0.5) * if i % 97 == 0 { 2.0 } else { 0.25 };
            }
        }

        let mut a = deltas.clone();
        let mut b = deltas.clone();
        scalar_coverage(&mut a, n);
        daegun::daerizer::daecpu::simd::coverage_in_place(&mut b, n);
        for (x, y) in a.iter().zip(&b) {
            worst_cov = worst_cov.max((x - y).abs());
        }

        let sb = scalar_bitmap(&deltas, n);
        let vb = daegun::daerizer::daecpu::simd::get_bitmap(&deltas, n);
        assert_eq!(sb.len(), vb.len(), "byte counts differ");
        for (x, y) in sb.iter().zip(&vb) {
            bytes_total += 1;
            if x != y {
                bytes_differing += 1;
                worst_byte = worst_byte.max((i32::from(*x) - i32::from(*y)).abs());
            }
        }
        let _ = cov_scalar;
    }

    eprintln!("  worst coverage delta {worst_cov:.3e}");
    eprintln!(
        "  bytes differing {bytes_differing} of {bytes_total} ({:.4}%), worst by {worst_byte}",
        100.0 * bytes_differing as f64 / bytes_total as f64
    );
}

#[test]
#[ignore]
fn prefix_sum_scalar_against_simd() {
    use std::time::{Duration, Instant};

    eprintln!("prefix_sum_scalar_against_simd  (backend {})", daegun::daerizer::daecpu::simd::BACKEND);
    for n in [63usize, 420, 1540, 5934, 23_427, 93_708] {
        let mut deltas = vec![0.0f32; n];
        let mut state = 0x9E3779B97F4A7C15u64;
        for (i, d) in deltas.iter_mut().enumerate() {
            state ^= state << 13; state ^= state >> 7; state ^= state << 17;
            if state.is_multiple_of(5) {
                let u = (state >> 11) as f32 / (1u64 << 53) as f32;
                *d = (u - 0.5) * if i % 97 == 0 { 2.0 } else { 0.25 };
            }
        }

        let (mut ts, mut tv) = (Vec::new(), Vec::new());
        let mut proof = 0u64;
        for round in 0..140 {
            let mut a = deltas.clone();
            let t = Instant::now();
            scalar_coverage(&mut a, n);
            let e_s = t.elapsed();
            proof += a[n / 2].to_bits() as u64;

            let mut b = deltas.clone();
            let t = Instant::now();
            daegun::daerizer::daecpu::simd::coverage_in_place(&mut b, n);
            let e_v = t.elapsed();
            proof += b[n / 2].to_bits() as u64;

            core::hint::black_box((&a, &b));
            if round >= 40 {
                ts.push(e_s);
                tv.push(e_v);
            }
        }
        assert!(proof > 0, "the kernels produced nothing");
        ts.sort();
        tv.sort();
        let med = |v: &Vec<Duration>| v[v.len() / 2].as_secs_f64() * 1e9 / n as f64;
        let (s, v) = (med(&ts), med(&tv));
        eprintln!("  n {n:>6}   scalar {s:>6.3} ns/elem   simd {v:>6.3} ns/elem   {:>5.2}x", s / v.max(1e-12));
    }
}

fn scalar_tap3(cov: &[f32], w0: &[f32], w1: &[f32], w2: &[f32]) -> (f32, f32, f32) {
    let (mut s0, mut s1, mut s2) = (0.0f32, 0.0, 0.0);
    for i in 0..cov.len() {
        s0 += cov[i] * w0[i];
        s1 += cov[i] * w1[i];
        s2 += cov[i] * w2[i];
    }
    (s0, s1, s2)
}

#[test]
#[ignore]
fn tap_run3_against_scalar() {
    use std::time::{Duration, Instant};

    eprintln!("tap_run3_against_scalar  (backend {})", daegun::daerizer::daecpu::simd::BACKEND);
    let mut state = 0x2545F4914F6CDD1Du64;
    let mut next = move || {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17;
        (state >> 11) as f32 / (1u64 << 53) as f32
    };

    let mut worst_rel = 0.0f64;
    for span in 1..=16usize {
        for _ in 0..2000 {
            let cov: Vec<f32> = (0..span).map(|_| next()).collect();
            let w0: Vec<f32> = (0..span).map(|_| next() * 0.3).collect();
            let w1: Vec<f32> = (0..span).map(|_| next() * 0.3).collect();
            let w2: Vec<f32> = (0..span).map(|_| next() * 0.3).collect();
            let s = scalar_tap3(&cov, &w0, &w1, &w2);
            let v = daegun::daerizer::daecpu::simd::tap_run3(&cov, &w0, &w1, &w2);
            for (a, b) in [(s.0, v.0), (s.1, v.1), (s.2, v.2)] {
                let rel = if a.abs() > 1e-6 { ((a - b) / a).abs() as f64 } else { (a - b).abs() as f64 };
                worst_rel = worst_rel.max(rel);
            }
        }
    }
    eprintln!("  worst relative disagreement over spans 1..=16: {worst_rel:.3e}");
    assert!(worst_rel < 1e-5, "the kernels disagree by {worst_rel:.3e}, far past rounding");

    for span in [5usize, 7, 16, 64] {
        let cov: Vec<f32> = (0..span).map(|_| next()).collect();
        let w0: Vec<f32> = (0..span).map(|_| next()).collect();
        let w1: Vec<f32> = (0..span).map(|_| next()).collect();
        let w2: Vec<f32> = (0..span).map(|_| next()).collect();
        let (mut ts, mut tv) = (Vec::new(), Vec::new());
        let mut proof = 0.0f64;
        const REPS: usize = 4000;
        for round in 0..120 {
            let t = Instant::now();
            let mut acc = 0.0f32;
            for _ in 0..REPS {
                let r = scalar_tap3(&cov, &w0, &w1, &w2);
                acc += r.0 + r.1 + r.2;
            }
            let e_s = t.elapsed();
            core::hint::black_box(acc);

            let t = Instant::now();
            let mut acc2 = 0.0f32;
            for _ in 0..REPS {
                let r = daegun::daerizer::daecpu::simd::tap_run3(&cov, &w0, &w1, &w2);
                acc2 += r.0 + r.1 + r.2;
            }
            let e_v = t.elapsed();
            core::hint::black_box(acc2);
            proof += f64::from(acc + acc2);
            if round >= 40 { ts.push(e_s); tv.push(e_v); }
        }
        assert!(proof.is_finite(), "the kernels produced nothing");
        ts.sort(); tv.sort();
        let med = |v: &Vec<Duration>| v[v.len() / 2].as_secs_f64() * 1e9 / REPS as f64;
        let (s, v) = (med(&ts), med(&tv));
        eprintln!("  span {span:>3}   scalar {s:>7.3} ns/run   simd {v:>7.3} ns/run   {:>5.2}x", s / v.max(1e-12));
    }
}

#[test]
#[ignore]
fn analytic_flattening_setup_against_the_flatness_test() {
    use std::time::{Duration, Instant};

    #[inline(always)]
    fn current(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, max_area: f32) -> bool {
        let mx = 0.25 * ax + 0.5 * bx + 0.25 * cx;
        let my = 0.25 * ay + 0.5 * by + 0.25 * cy;
        let _ = (mx, my);
        ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs() > max_area
    }

    #[inline(always)]
    fn analytic(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, tol: f32) -> u32 {
        let (ddx, ddy) = (2.0 * bx - ax - cx, 2.0 * by - ay - cy);
        let (d1x, d1y) = (bx - ax, by - ay);
        let cross = (cx - ax) * ddy - (cy - ay) * ddx;
        let p0 = (d1x * ddx + d1y * ddy) / cross;
        let p2 = ((cx - bx) * ddx + (cy - by) * ddy) / cross;
        let scale = (cross / ((ddx * ddx + ddy * ddy).sqrt() * (p2 - p0))).abs();
        let f = |x: f32| x / (0.33 + (0.201_511 + x * x * 0.25).sqrt().sqrt());
        let (i0, i1) = (f(p0), f(p2));
        let n = 0.5 * (i1 - i0).abs() * (scale / tol).sqrt();
        if n.is_finite() { (n.ceil() as u32).max(1) } else { 1 }
    }

    let mut state = 0x2545F4914F6CDD1Du64;
    let mut next = move || {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17;
        (state >> 11) as f32 / (1u64 << 53) as f32
    };
    let curves: Vec<[f32; 6]> = (0..4096)
        .map(|_| [next() * 700.0, next() * 700.0, next() * 700.0, next() * 700.0, next() * 700.0, next() * 700.0])
        .collect();

    let (mut tc, mut ta) = (Vec::new(), Vec::new());
    let mut proof = 0u64;
    for round in 0..140 {
        let t = Instant::now();
        let mut hits = 0u32;
        for c in &curves {
            if current(c[0], c[1], c[2], c[3], c[4], c[5], 384.0) { hits += 1; }
        }
        let e_c = t.elapsed();
        core::hint::black_box(hits);

        let t = Instant::now();
        let mut segs = 0u32;
        for c in &curves {
            segs += analytic(c[0], c[1], c[2], c[3], c[4], c[5], 3.0);
        }
        let e_a = t.elapsed();
        core::hint::black_box(segs);

        proof += u64::from(hits) + u64::from(segs);
        if round >= 40 { tc.push(e_c); ta.push(e_a); }
    }
    assert!(proof > 0, "neither path produced anything");
    tc.sort(); ta.sort();
    let med = |v: &Vec<Duration>| v[v.len() / 2].as_secs_f64() * 1e9 / curves.len() as f64;
    let (c, a) = (med(&tc), med(&ta));
    eprintln!("analytic_flattening_setup_against_the_flatness_test");
    eprintln!("  current flatness test  {c:>6.3} ns/curve");
    eprintln!("  analytic segment count {a:>6.3} ns/curve   {:>5.2}x the cost", a / c.max(1e-12));
}

#[test]
#[ignore]
fn raster_reset_against_fresh_allocation() {
    use daegun::daerizer::daecpu::rasterize::Raster;
    use std::time::{Duration, Instant};

    eprintln!("raster_reset_against_fresh_allocation");
    for (w, h, label) in [
        (7usize, 9usize, "12px 'B'"),
        (9, 12, "16px 'B'"),
        (18, 23, "32px 'B'"),
        (35, 44, "64px 'B'"),
        (69, 86, "128px 'B'"),
        (137, 171, "256px 'B'"),
    ] {
        let (mut tf, mut tr) = (Vec::new(), Vec::new());
        let mut reused = Raster::new(w, h);
        let mut proof = 0usize;
        const REPS: usize = 200;

        for round in 0..100 {
            let t = Instant::now();
            for _ in 0..REPS {
                let r = Raster::new(w, h);
                core::hint::black_box(&r);
                proof += 1;
            }
            tf.push(t.elapsed());

            let t = Instant::now();
            for _ in 0..REPS {
                reused.reset(w, h);
                core::hint::black_box(&reused);
                proof += 1;
            }
            tr.push(t.elapsed());

            if round < 30 {
                tf.clear();
                tr.clear();
            }
        }
        assert!(proof > 0, "nothing allocated");
        tf.sort();
        tr.sort();
        let med = |v: &Vec<Duration>| v[v.len() / 2].as_secs_f64() * 1e9 / REPS as f64;
        let (f, r) = (med(&tf), med(&tr));
        eprintln!(
            "  {label:>10} {w:>4}x{h:<4}  fresh {f:>8.1} ns   reset {r:>8.1} ns   {:>5.2}x",
            f / r.max(1e-12)
        );
    }
}

#[test]
#[ignore]
fn how_much_of_the_accumulator_is_zero() {
    eprintln!("how_much_of_the_accumulator_is_zero");
    for (ch, px) in [('B', 16.0f32), ('B', 64.0), ('B', 256.0), ('o', 32.0), ('W', 128.0), ('.', 16.0)] {
        let (cov, n) = accumulator("eb-garamond/EBGaramond.ttf", ch, px);
        let _ = cov;

        let path = format!("{}/eb-garamond/EBGaramond.ttf", crate::FONTS);
        let bytes = std::fs::read(&path).expect("font");
        let map = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("parses");
        let head = map.get("head").expect("head");
        let fmt = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("fmt");
        let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
        let ng = daegun::daecore::daetype::decoder::read_u16_be(map.get("maxp").expect("maxp"), 4).expect("n");
        let loca = daegun::daecore::daetype::instancer::parse_loca(&map, fmt, ng as usize).expect("loca");
        let gid = daegun::daecore::daetype::subsetter::cmap_glyph_id(map.get("cmap").expect("cmap"), ch as u32)
            .expect("glyph");

        let mut g = daegun::daerizer::daecpu::math::Geometry::new(px, upm);
        daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&map, &loca, gid, &mut g).expect("draws");
        let mut glyph = daegun::daerizer::daecpu::math::Glyph::default();
        g.finalize(&mut glyph);
        let scale = px / upm;
        let (m, ox, oy) = daegun::daerizer::daecpu::rasterize::metrics_raw(scale, glyph.bounds, 0.0, 0.0, 0.0);
        if m.width == 0 || m.height == 0 {
            eprintln!("  {ch} at {px}px: empty box");
            continue;
        }
        let mut r = daegun::daerizer::daecpu::rasterize::Raster::new(m.width, m.height);
        r.draw(&glyph, scale, scale, ox, oy);
        let deltas = r.into_coverage(daegun::daecore::daetype::outline::FillRule::NonZero);
        let (w, h) = (m.width, m.height);
        let mut zero_rows = 0usize;
        let mut zero_cells = 0usize;
        for row in 0..h {
            let slice = &deltas[row * w..(row + 1) * w];
            if slice.iter().all(|v| *v == 0.0) {
                zero_rows += 1;
            }
            zero_cells += slice.iter().filter(|v| **v == 0.0).count();
        }
        eprintln!(
            "  {ch:>1} at {px:>5}px  {w:>4}x{h:<4} = {n:>6} cells   fully-zero rows {zero_rows:>4}/{h:<4} ({:>5.1}%)   zero cells {:>5.1}%",
            100.0 * zero_rows as f64 / h as f64,
            100.0 * zero_cells as f64 / (w * h) as f64,
        );
    }
}
