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

fn one_glyph(batch: &mut GpuBatch) -> daegun::GlyphInstance {
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let slot = face.glyph(batch, 37).expect("a glyph with an outline");
    // Three different channels, or a byte swap would be invisible.
    slot.instance([8.0, 8.0], 64.0, [64.0, 64.0], [1.0, 0.15, 0.0, 1.0])
}

// A DXGI swapchain is BGRA, so the two byte orders have to put the same colors in the same places.
#[test]
fn the_two_byte_orders_agree_once_swapped() {
    let Some(r) = renderer() else { return };
    let mut batch = GpuBatch::new();
    let inst = one_glyph(&mut batch);
    let geometry = r.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();
    let (w, h) = (96u32, 96u32);

    let shot = |format| {
        let mut t = r.target_with_format(w, h, format).expect("target");
        r.draw(&mut t, &geometry, &[inst], &params, d3d11::Mode::Grayscale).expect("draw");
        r.read_pixels(&mut t).expect("read").to_vec()
    };
    let rgba = shot(d3d11::Format::Rgba8Unorm);
    let bgra = shot(d3d11::Format::Bgra8Unorm);

    assert_eq!(rgba.len(), bgra.len(), "the two targets are different sizes");
    let mut inked = 0usize;
    for (i, (a, b)) in rgba.chunks_exact(4).zip(bgra.chunks_exact(4)).enumerate() {
        assert_eq!(
            [b[0], b[1], b[2], b[3]],
            [a[2], a[1], a[0], a[3]],
            "pixel {i} did not come back byte-swapped",
        );
        if a[3] > 0 {
            inked += 1;
        }
    }
    assert!(inked > 300, "only {inked} pixels took ink, so the comparison proved little");
}

#[test]
fn a_target_clears_to_the_color_it_was_given() {
    let Some(r) = renderer() else { return };
    let mut batch = GpuBatch::new();
    let _ = one_glyph(&mut batch);
    let geometry = r.geometry(&batch).expect("geometry");
    let mut t = r.target(64, 64).expect("target");

    let transparent = daegun::paint::Rgba { r: 0, g: 0, b: 0, a: 0 };
    assert_eq!(t.clear(), Some(transparent), "the default clear is no longer transparent black");

    let slate = daegun::paint::Rgba { r: 20, g: 40, b: 60, a: 255 };
    t.set_clear(Some(slate));
    r.draw(&mut t, &geometry, &[], &SubpixelParams::default(), d3d11::Mode::Grayscale)
        .expect("draw");
    let px = r.read_pixels(&mut t).expect("read");
    assert!(
        px.chunks_exact(4).all(|p| p == [20, 40, 60, 255]),
        "the target did not clear to the color it was given",
    );
}

// Every draw cleared, so a second geometry erased the first. Not clearing is what lets them layer.
#[test]
fn a_second_geometry_can_draw_over_the_first() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let (w, h) = (128u32, 64u32);
    let params = SubpixelParams::default();

    let mut left_batch = GpuBatch::new();
    let left = face
        .glyph(&mut left_batch, 37)
        .expect("glyph")
        .instance([6.0, 6.0], 48.0, [48.0, 48.0], [1.0; 4]);
    let left_geo = r.geometry(&left_batch).expect("geometry");

    let mut right_batch = GpuBatch::new();
    let right = face
        .glyph(&mut right_batch, 50)
        .expect("glyph")
        .instance([72.0, 6.0], 48.0, [48.0, 48.0], [1.0; 4]);
    let right_geo = r.geometry(&right_batch).expect("geometry");

    let halves = |px: &[u8]| {
        let (mut l, mut rt) = (0usize, 0usize);
        for y in 0..h as usize {
            for x in 0..w as usize {
                if px[(y * w as usize + x) * 4 + 3] > 0 {
                    if x < 64 { l += 1 } else { rt += 1 }
                }
            }
        }
        (l, rt)
    };

    let mut layered = r.target(w, h).expect("target");
    r.draw(&mut layered, &left_geo, &[left], &params, d3d11::Mode::Grayscale).expect("first");
    layered.set_clear(None);
    r.draw(&mut layered, &right_geo, &[right], &params, d3d11::Mode::Grayscale).expect("second");
    let (l, rt) = halves(r.read_pixels(&mut layered).expect("read"));
    assert!(l > 0 && rt > 0, "layering lost a draw: left {l}, right {rt}");

    let mut cleared = r.target(w, h).expect("target");
    r.draw(&mut cleared, &left_geo, &[left], &params, d3d11::Mode::Grayscale).expect("first");
    r.draw(&mut cleared, &right_geo, &[right], &params, d3d11::Mode::Grayscale).expect("second");
    let (l, rt) = halves(r.read_pixels(&mut cleared).expect("read"));
    assert_eq!(l, 0, "a clearing draw no longer wipes what came before it");
    assert!(rt > 0, "the second draw did not land");
}

// A swapchain backbuffer belongs to the device its swapchain was made on, so drawing into one needs
// the caller's device rather than the one daegun would have created.
#[test]
fn a_renderer_can_adopt_a_device_it_did_not_create() {
    let Some(owner) = renderer() else { return };
    let mut batch = GpuBatch::new();
    let inst = one_glyph(&mut batch);
    let params = SubpixelParams::default();

    let shot = |r: &d3d11::Renderer| {
        let geometry = r.geometry(&batch).expect("geometry");
        let mut t = r.target(96, 96).expect("target");
        r.draw(&mut t, &geometry, &[inst], &params, d3d11::Mode::Grayscale).expect("draw");
        r.read_pixels(&mut t).expect("read").to_vec()
    };
    let from_owner = shot(&owner);

    let (device, context) = unsafe { owner.handles() };
    let adopted = unsafe { d3d11::Renderer::from_device(device, context) }.expect("adopts");
    assert_eq!(adopted.device_name(), owner.device_name(), "adopted a different device");
    assert_eq!(shot(&adopted), from_owner, "the adopted device drew something else");
    assert!(from_owner.chunks_exact(4).any(|p| p[3] > 0), "neither renderer drew anything");
}

#[test]
fn adopting_a_null_device_is_refused_rather_than_crashing() {
    assert!(
        unsafe { d3d11::Renderer::from_device(core::ptr::null_mut(), core::ptr::null_mut()) }
            .is_err(),
        "a null device was accepted",
    );
}

// A borrowed target has no staging of its own, so it is pointed at the texture of an owned one: the
// draw goes through the borrowed path and the pixels come back through the owner.
#[test]
fn a_borrowed_texture_draws_what_an_owned_one_does() {
    let Some(r) = renderer() else { return };
    let mut batch = GpuBatch::new();
    let inst = one_glyph(&mut batch);
    let geometry = r.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();
    let (w, h) = (96u32, 96u32);

    for format in [d3d11::Format::Rgba8Unorm, d3d11::Format::Bgra8Unorm] {
        let mut owned = r.target_with_format(w, h, format).expect("owned target");
        r.draw(&mut owned, &geometry, &[inst], &params, d3d11::Mode::Grayscale).expect("draw");
        let direct = r.read_pixels(&mut owned).expect("read").to_vec();

        let mut borrowed = unsafe { r.target_from_texture(owned.texture(), w, h, format) }
            .expect("a target over the owned texture");
        assert!(
            r.read_pixels(&mut borrowed).is_err(),
            "a borrowed target claimed to have pixels to read back",
        );
        r.draw(&mut borrowed, &geometry, &[inst], &params, d3d11::Mode::Grayscale).expect("draw");
        drop(borrowed);

        let through_borrow = r.read_pixels(&mut owned).expect("read").to_vec();
        assert_eq!(direct, through_borrow, "{format:?}: the borrowed path drew something else");
        assert!(direct.chunks_exact(4).any(|p| p[3] > 0), "{format:?}: nothing was drawn at all");
    }
}
