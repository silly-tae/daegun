use daegun::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};

fn alloc_fmt(r: &d3d11::Renderer) -> String {
    let kind = if r.is_software() { "software" } else { "hardware" };
    format!("{} ({kind}), feature level {}", r.device_name(), r.feature_level())
}
use daegun::daerizer::daegpu::{GpuBatch, SubpixelParams, d3d11, eval};

fn renderer() -> Option<d3d11::Renderer> {
    match d3d11::Renderer::new() {
        Ok(r) => Some(r),
        Err(d3d11::Error::NoDevice) => None,
        Err(e) => panic!("the backend tried and failed rather than reporting no device: {e}"),
    }
}

#[test]
fn a_renderer_is_created_or_absent_and_never_broken() {
    match d3d11::Renderer::new() {
        Ok(renderer) => {
            let name = alloc_fmt(&renderer);
            assert!(!name.is_empty(), "a device was created but reports no name");
            std::eprintln!(
                "d3d11: {name}{}",
                if renderer.is_software() { "  <- a conformant reference, not a driver" } else { "" }
            );
        }
        Err(d3d11::Error::NoDevice) => {
            std::eprintln!("d3d11: no device on this machine, which is a valid outcome");
        }
        Err(other) => panic!("the backend tried and failed rather than reporting no device: {other}"),
    }
}

#[test]
fn a_new_target_reads_back_transparent_black() {
    let Some(r) = renderer() else { return };
    let mut t = r.target(64, 48).expect("target");
    assert_eq!(t.width(), 64);
    assert_eq!(t.height(), 48);

    let pixels = r.read_pixels(&mut t).expect("read");
    assert_eq!(pixels.len(), 64 * 48 * 4, "RGBA8, tightly packed");
    assert!(pixels.iter().all(|&b| b == 0), "a cleared target read back non-zero");
}

#[test]
fn an_awkward_width_is_repacked_rather_than_sheared() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 36).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");

    let (w, h) = (100u32, 40u32);
    let mut t = r.target(w, h).expect("target");
    let px = 32.0;
    let inst = slot.instance([6.0, 4.0], px, [px, px], [1.0, 1.0, 1.0, 1.0]);
    r.draw(&mut t, &geometry, &[inst], &SubpixelParams::default(), d3d11::Mode::Grayscale)
        .expect("draw");
    let pixels = r.read_pixels(&mut t).expect("read");
    assert_eq!(pixels.len() as u32, w * h * 4);

    let mut first = w;
    let mut last = 0u32;
    for y in 0..h {
        for x in 0..w {
            if pixels[((y * w + x) * 4 + 3) as usize] > 0 {
                first = first.min(x);
                last = last.max(x);
            }
        }
    }
    assert!(last >= first, "the draw produced no ink at all");
    assert!(
        last - first < 48,
        "ink spans columns {first}..={last}, which is wider than the glyph — the rows are sheared"
    );
}

#[test]
fn a_target_with_no_area_is_refused() {
    let Some(r) = renderer() else { return };
    assert!(matches!(r.target(0, 16), Err(d3d11::Error::BadTarget)));
    assert!(matches!(r.target(16, 0), Err(d3d11::Error::BadTarget)));
}

#[test]
fn a_target_may_outlive_its_renderer() {
    let Some(r) = renderer() else { return };
    let mut t = r.target(32, 32).expect("target");
    assert!(r.read_pixels(&mut t).is_ok());
    drop(r);
    drop(t);
}

#[test]
fn a_batch_becomes_geometry() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("inter/InterVariable.ttf");
    let mut batch = GpuBatch::new();
    let n = face.fill(&mut batch, 1..200, 7);
    assert!(n > 10, "only {n} glyphs made it into the batch");

    let g = r.geometry(&batch).expect("upload");
    assert_eq!(g.revision(), batch.revision(), "geometry records the batch it was built from");

    let empty = GpuBatch::new();
    let ge = r.geometry(&empty).expect("an empty batch is not an error");
    assert_eq!(ge.revision(), empty.revision());
}

#[test]
fn a_draw_inks_the_target_the_right_way_up() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 36).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");

    let (w, h) = (96u32, 128u32);
    let mut t = r.target(w, h).expect("target");
    let px = 48.0;
    let inst = slot.instance([8.0, 8.0], px, [px, px], [1.0, 1.0, 1.0, 1.0]);
    r.draw(&mut t, &geometry, &[inst], &SubpixelParams::default(), d3d11::Mode::Grayscale)
        .expect("draw");
    let pixels = r.read_pixels(&mut t).expect("read");

    let (mut inked, mut top, mut bottom) = (0usize, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if pixels[((y * w + x) * 4 + 3) as usize] > 0 {
                inked += 1;
                if y < h / 2 { top += 1 } else { bottom += 1 }
            }
        }
    }
    assert!(inked > 200, "only {inked} pixels took ink, so the draw produced nothing");
    assert!(
        bottom > top * 4,
        "{top} inked pixels above the midline against {bottom} below: the projection is flipped"
    );
}

#[test]
fn a_draw_with_no_instances_leaves_the_target_empty() {
    let Some(r) = renderer() else { return };
    let batch = GpuBatch::new();
    let geometry = r.geometry(&batch).expect("geometry");
    let mut t = r.target(32, 32).expect("target");
    r.draw(&mut t, &geometry, &[], &SubpixelParams::default(), d3d11::Mode::Grayscale)
        .expect("draw");
    let pixels = r.read_pixels(&mut t).expect("read");
    assert!(pixels.iter().all(|&b| b == 0), "an empty draw put ink on the target");
}

#[test]
fn growing_instance_counts_regrow_the_buffer() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("inter/InterVariable.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 36).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();

    let (w, h) = (256u32, 64u32);
    let mut t = r.target(w, h).expect("target");
    let mut previous = 0usize;
    for n in 1..=8usize {
        let insts: Vec<_> = (0..n)
            .map(|i| {
                slot.instance([4.0 + i as f32 * 28.0, 8.0], 24.0, [24.0, 24.0], [1.0, 1.0, 1.0, 1.0])
            })
            .collect();
        r.draw(&mut t, &geometry, &insts, &params, d3d11::Mode::Grayscale).expect("draw");
        let inked = r.read_pixels(&mut t).expect("read")
            .chunks_exact(4)
            .filter(|p| p[3] > 0)
            .count();
        assert!(inked > previous, "{n} glyphs inked {inked} pixels, no more than {n} minus one did");
        previous = inked;
    }
}

#[test]
fn subpixel_mode_gives_the_channels_different_coverage() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 36).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");

    let mut t = r.target(64, 64).expect("target");
    let inst = slot.instance([8.0, 8.0], 40.0, [40.0, 40.0], [1.0, 1.0, 1.0, 1.0]);
    let layout = SubpixelLayout::horizontal(StripeOrder::Rgb);
    r.draw(&mut t, &geometry, &[inst], &SubpixelParams::from_layout(&layout), d3d11::Mode::Subpixel)
        .expect("draw");
    let pixels = r.read_pixels(&mut t).expect("read");

    let differing = pixels.chunks_exact(4).filter(|p| p[0] != p[1] || p[1] != p[2]).count();
    assert!(
        differing > 20,
        "only {differing} pixels have per-channel coverage, so dual-source did nothing"
    );
}

#[test]
fn a_foreign_target_is_refused() {
    let (Some(a), Some(b)) = (renderer(), renderer()) else { return };
    let batch = GpuBatch::new();
    let g = a.geometry(&batch).expect("geometry");
    let mut t = a.target(16, 16).expect("target");
    assert!(matches!(
        b.draw(&mut t, &g, &[], &SubpixelParams::default(), d3d11::Mode::Grayscale),
        Err(d3d11::Error::BadTarget)
    ));
    assert!(matches!(b.read_pixels(&mut t), Err(d3d11::Error::BadTarget)));
    assert!(a.read_pixels(&mut t).is_ok());
}

#[test]
fn d3d11_grayscale_matches_the_reference_evaluator() {
    let Some(gpu) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
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

        gpu.draw(&mut target, &geometry, &[inst], &params, d3d11::Mode::Grayscale).expect("draw");
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
    std::eprintln!("d3d11-vs-eval: mean {mean:.5}  over-16 rate {:.4}%  worst {worst}", rate * 100.0);
    assert!(mean < 0.003, "mean |delta| is {mean:.5} of 255; the interpolated coordinate scored 0.00425 on Apple and 0.00456 on AMD");
    assert!(rate < 0.0002, "{over} of {checked} pixels ({:.4}%) differ by more than 16", rate * 100.0);
    assert!(worst <= 2, "a pixel differs by {worst} of 255; this should be 0");
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
