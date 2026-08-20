use daegun::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};
use super::face::Face;
use daegun::daerizer::daegpu::{eval, GpuBatch, SubpixelParams, ffi::{Mode, Renderer}};

fn renderer() -> Option<Renderer> {
    Renderer::new().ok()
}

#[test]
fn metal_grayscale_matches_the_reference_evaluator() {
    let Some(gpu) = renderer() else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let params = SubpixelParams::default();
    let (w, h) = (128u32, 128u32);
    let px = 96.0f32;
    let inv_px = 1.0f32 / px;
    let mut target = gpu.target(w, h).expect("target");

    let (mut checked, mut worst, mut inked) = (0usize, 0i32, 0usize);
    let (mut sum, mut over) = (0f64, 0usize);
    for gid in [36u16, 50, 37, 74, 90, 25] {
        let Some(slot) = face.glyph(&mut batch, gid) else { continue };
        let geometry = gpu.geometry(&batch).expect("geometry");
        let inst = slot.instance([16.0, 16.0], px, [px, px], [1.0, 1.0, 1.0, 1.0]);

        gpu.draw(&mut target, &geometry, &[inst], &params, Mode::Grayscale).expect("draw");
        let pixels = gpu.read_pixels(&mut target).expect("read").to_vec();

        for y in 0..h {
            for x in 0..w {
                let em = [
                    (x as f32 + 0.5 - 16.0) * inv_px,
                    ((h - 1 - y) as f32 + 0.5 - 16.0) * inv_px,
                ];
                let want = (eval::coverage(&batch, &slot, em, [px, px]) * 255.0).round() as i32;
                let got = i32::from(pixels[((y * w + x) * 4 + 3) as usize]);
                let delta = (got - want).abs();
                worst = worst.max(delta);
                sum += f64::from(delta);
                if delta > 16 { over += 1 }
                checked += 1;
                if got > 0 { inked += 1 }
            }
        }
    }
    assert!(checked > 50_000, "only {checked} pixels compared");
    assert!(inked > 2_000, "only {inked} pixels took ink, so the draw produced nothing to grade");

    let mean = sum / checked as f64;
    let rate = over as f64 / checked as f64;
    std::eprintln!("metal-vs-eval: mean {mean:.5}  over-16 rate {:.4}%  worst {worst}", rate * 100.0);
    assert!(mean < 0.003, "mean |delta| is {mean:.5} of 255; the interpolated coordinate scored 0.00425 on Apple and 0.00456 on AMD");
    assert!(rate < 0.0002, "{over} of {checked} pixels ({:.4}%) differ by more than 16", rate * 100.0);
    assert!(worst <= 2, "a pixel differs by {worst} of 255; this should be 0");
}

#[test]
fn pixels_arrive_only_when_read() {
    let Some(gpu) = renderer() else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("glyph");
    let geometry = gpu.geometry(&batch).expect("geometry");
    let inst = slot.instance([8.0, 8.0], 64.0, [64.0, 64.0], [1.0, 1.0, 1.0, 1.0]);
    let mut target = gpu.target(96, 96).expect("target");

    assert!(target.pixels().iter().all(|&b| b == 0), "a new target is not blank");

    gpu.draw(&mut target, &geometry, &[inst], &SubpixelParams::default(), Mode::Grayscale)
        .expect("draw");
    let ink = gpu.read_pixels(&mut target).expect("read").iter().filter(|&&b| b > 0).count();
    assert!(ink > 200, "only {ink} non-zero bytes after a read");
}

#[test]
fn draws_beyond_the_ring_depth_stay_correct() {
    let Some(gpu) = renderer() else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 48).expect("glyph");
    let geometry = gpu.geometry(&batch).expect("geometry");
    let mut target = gpu.target(96, 96).expect("target");
    let params = SubpixelParams::default();

    let mut first = None;
    for round in 0..24 {
        let instances: Vec<_> = (0..=(round % 3))
            .map(|i| slot.instance([8.0 + i as f32 * 0.0, 8.0], 64.0, [64.0, 64.0], [1.0, 1.0, 1.0, 1.0]))
            .collect();
        gpu.draw(&mut target, &geometry, &instances, &params, Mode::Grayscale).expect("draw");
        let ink = gpu.read_pixels(&mut target).expect("read").iter().filter(|&&b| b > 0).count();
        match first {
            None => first = Some(ink),
            Some(f) => assert_eq!(ink, f, "round {round} rendered differently from the first"),
        }
    }
}

#[test]
fn subpixel_and_grayscale_both_render_and_differ() {
    let Some(gpu) = renderer() else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 72).expect("glyph");
    let geometry = gpu.geometry(&batch).expect("geometry");
    let inst = slot.instance([8.0, 8.0], 32.0, [32.0, 32.0], [1.0, 1.0, 1.0, 1.0]);
    let mut target = gpu.target(64, 64).expect("target");

    gpu.draw(&mut target, &geometry, &[inst], &SubpixelParams::default(), Mode::Grayscale).expect("draw");
    let gray = gpu.read_pixels(&mut target).expect("read").to_vec();

    let layout = SubpixelLayout::horizontal(StripeOrder::Rgb);
    gpu.draw(&mut target, &geometry, &[inst], &SubpixelParams::from_layout(&layout), Mode::Subpixel).expect("draw");
    let sub = gpu.read_pixels(&mut target).expect("read").to_vec();

    assert!(gray.iter().any(|&b| b > 0), "grayscale drew nothing");
    assert!(sub.iter().any(|&b| b > 0), "subpixel drew nothing");
    assert_ne!(gray, sub, "subpixel and grayscale produced identical pixels");
}

#[test]
fn degenerate_targets_are_refused() {
    let Some(gpu) = renderer() else { return };
    assert!(gpu.target(0, 16).is_err());
    assert!(gpu.target(16, 0).is_err());
    let mut t = gpu.target(8, 8).expect("target");
    assert!(gpu.wait(&mut t).is_ok());
}

#[test]
fn a_second_renderer_on_the_same_device_shares_targets() {
    let Some(gpu) = renderer() else { return };
    let Some(other) = renderer() else { return };

    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("glyph");
    let geometry = other.geometry(&batch).expect("geometry");
    let inst = slot.instance([8.0, 8.0], 48.0, [48.0, 48.0], [1.0; 4]);

    let mut target = gpu.target(64, 64).expect("target");
    other
        .draw(&mut target, &geometry, &[inst], &SubpixelParams::default(), Mode::Grayscale)
        .expect("a target from a renderer on the same device must be accepted");
    let pixels = other.read_pixels(&mut target).expect("read");
    assert!(pixels.iter().any(|&b| b > 0), "drew nothing");
}

#[test]
fn a_target_drawn_by_one_renderer_reads_back_through_another() {
    let Some(drawer) = renderer() else { return };
    let Some(reader) = renderer() else { return };

    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("glyph");
    let geometry = drawer.geometry(&batch).expect("geometry");
    let inst = slot.instance([8.0, 8.0], 48.0, [48.0, 48.0], [1.0; 4]);

    let mut target = drawer.target(64, 64).expect("target");
    drawer
        .draw(&mut target, &geometry, &[inst], &SubpixelParams::default(), Mode::Grayscale)
        .expect("draw");

    let pixels = reader.read_pixels(&mut target).expect("a target on the same device must read back");
    let inked = pixels.iter().filter(|&&b| b > 0).count();
    assert!(inked > 200, "only {inked} bytes took ink, so the read raced the draw or drew nothing");

    assert!(reader.wait(&mut target).is_ok(), "wait after read_pixels must be a no-op");
    assert!(drawer.wait(&mut target).is_ok(), "wait after read_pixels must be a no-op");

    let again = drawer.read_pixels(&mut target).expect("read");
    assert_eq!(again.iter().filter(|&&b| b > 0).count(), inked, "a second read disagreed");
}

#[test]
fn the_native_handle_is_stable_and_distinct() {
    let Some(r) = renderer() else { return };
    let (mut a, b) = (r.target(64, 48).expect("target"), r.target(64, 48).expect("target"));

    let h = unsafe { a.texture() };
    assert!(!h.is_null(), "the target handed out a null texture");
    assert_eq!(h, unsafe { a.texture() }, "the handle changed between two calls");
    assert_ne!(h, unsafe { b.texture() }, "two targets named one texture");

    r.read_pixels(&mut a).expect("read");
    assert_eq!(h, unsafe { a.texture() }, "a read replaced the texture");
}
#[test]
fn a_custom_projection_still_draws_the_glyph() {
    let Some(gpu) = renderer() else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let (w, h) = (128u32, 128u32);
    let Some(slot) = face.glyph(&mut batch, 36u16) else { return };
    let geometry = gpu.geometry(&batch).expect("geometry");
    let inst = slot.instance([16.0, 16.0], 48.0, [48.0, 48.0], [1.0; 4]);
    let params = SubpixelParams::default();

    let mut a = gpu.target(w, h).expect("target");
    gpu.draw(&mut a, &geometry, &[inst], &params, Mode::Grayscale).expect("draw");
    let default_ink = gpu.read_pixels(&mut a).expect("read").iter().filter(|&&b| b > 0).count();

    let mut b = gpu.target(w, h).expect("target");
    let same = daegun::daerizer::daegpu::ffi::ortho(w, h);
    gpu.draw_with(&mut b, &geometry, &[inst], &params, Mode::Grayscale, &same).expect("draw_with");
    let same_ink = gpu.read_pixels(&mut b).expect("read").iter().filter(|&&b| b > 0).count();
    assert_eq!(default_ink, same_ink, "the explicit default projection drew something else");

    let mut c = gpu.target(w, h).expect("target");
    let half = [
        1.0 / w as f32, 0.0, 0.0, 0.0,
        0.0, 1.0 / h as f32, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -1.0, -1.0, 0.0, 1.0,
    ];
    gpu.draw_with(&mut c, &geometry, &[inst], &params, Mode::Grayscale, &half).expect("draw_with");
    let half_ink = gpu.read_pixels(&mut c).expect("read").iter().filter(|&&b| b > 0).count();
    let bbox = |px: &[u8], w: u32| {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (i, c) in px.chunks_exact(4).enumerate() {
            if c[3] > 0 {
                let (x, y) = (i as u32 % w, i as u32 / w);
                x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x); y1 = y1.max(y);
            }
        }
        (x0, y0, x1, y1)
    };
    let da = { let mut t = gpu.target(w, h).expect("t");
        gpu.draw(&mut t, &geometry, &[inst], &params, Mode::Grayscale).expect("d");
        bbox(gpu.read_pixels(&mut t).expect("r"), w) };
    let dh = { let mut t = gpu.target(w, h).expect("t");
        gpu.draw_with(&mut t, &geometry, &[inst], &params, Mode::Grayscale, &half).expect("d");
        bbox(gpu.read_pixels(&mut t).expect("r"), w) };
    std::eprintln!("PROJ default ink {default_ink} box {da:?}");
    std::eprintln!("PROJ half    ink {half_ink} box {dh:?}");
    assert!(half_ink > 0, "a half-scale projection drew nothing at all");

    for (name, got, want) in [
        ("x0", dh.0 as f32, da.0 as f32 / 2.0), ("y0", dh.1 as f32, (da.1 as f32 + 128.0) / 2.0),
        ("x1", dh.2 as f32, da.2 as f32 / 2.0),
    ] {
        assert!(
            (got - want).abs() <= 2.0,
            "under a half-scale projection {name} landed at {got}, not the {want} the matrix asks \
             for — the shader is not honouring the projection it was given",
        );
    }
}
