use daegun::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};
use super::face::Face;
use daegun::daerizer::daegpu::{eval, GpuBatch, SubpixelParams, ffi::{Format, Mode, Renderer}};

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

#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {
    fn MTLCreateSystemDefaultDevice() -> *mut core::ffi::c_void;
}

fn one_glyph(batch: &mut GpuBatch) -> daegun::GlyphInstance {
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let slot = face.glyph(batch, 37).expect("glyph");
    // Three different channels, or a byte swap would be invisible.
    slot.instance([8.0, 8.0], 64.0, [64.0, 64.0], [1.0, 0.15, 0.0, 1.0])
}

// A surface daegun did not create picks its own byte order, and CAMetalLayer only offers BGRA. The
// two pipelines must put the same colors in the same places, swapped.
#[test]
fn the_two_byte_orders_agree_once_swapped() {
    let Some(gpu) = renderer() else { return };
    let mut batch = GpuBatch::new();
    let inst = one_glyph(&mut batch);
    let geometry = gpu.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();
    let (w, h) = (96u32, 96u32);

    let shot = |format| {
        let mut t = gpu.target_with_format(w, h, format).expect("target");
        gpu.draw(&mut t, &geometry, &[inst], &params, Mode::Subpixel).expect("draw");
        gpu.read_pixels(&mut t).expect("read").to_vec()
    };
    let rgba = shot(Format::Rgba8Unorm);
    let bgra = shot(Format::Bgra8Unorm);

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
    let colored = rgba.chunks_exact(4).any(|p| p[0] != p[2]);
    assert!(colored, "every pixel was gray, so a byte swap could not have shown");
}

// Rendering into a caller's swapchain needs the caller's device: a swapchain image belongs to the
// device that made it, and `new` only ever takes the system default.
#[test]
fn a_renderer_can_adopt_a_device_the_caller_made() {
    let Some(default) = renderer() else { return };
    let device = unsafe { MTLCreateSystemDefaultDevice() };
    assert!(!device.is_null(), "no Metal device to adopt");

    let adopted = unsafe { Renderer::from_device(device) }.expect("adopts a live device");
    assert_eq!(adopted.device_name(), default.device_name(), "adopted a different device");

    let mut batch = GpuBatch::new();
    let inst = one_glyph(&mut batch);
    let params = SubpixelParams::default();
    let shot = |r: &Renderer| {
        let geometry = r.geometry(&batch).expect("geometry");
        let mut t = r.target(96, 96).expect("target");
        r.draw(&mut t, &geometry, &[inst], &params, Mode::Subpixel).expect("draw");
        r.read_pixels(&mut t).expect("read").to_vec()
    };
    assert_eq!(shot(&adopted), shot(&default), "the adopted device drew something else");
}

#[test]
fn a_null_device_is_refused_rather_than_crashing() {
    assert!(unsafe { Renderer::from_device(core::ptr::null_mut()) }.is_err());
}

// A borrowed target has no readback of its own, so it is pointed at the texture of an owned one:
// the draw goes through the borrowed path and the pixels come back through the owner.
#[test]
fn a_borrowed_texture_draws_what_an_owned_one_does() {
    let Some(gpu) = renderer() else { return };
    let mut batch = GpuBatch::new();
    let inst = one_glyph(&mut batch);
    let geometry = gpu.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();
    let (w, h) = (96u32, 96u32);

    for format in [Format::Rgba8Unorm, Format::Bgra8Unorm] {
        let mut owned = gpu.target_with_format(w, h, format).expect("owned target");
        gpu.draw(&mut owned, &geometry, &[inst], &params, Mode::Subpixel).expect("draw");
        let direct = gpu.read_pixels(&mut owned).expect("read").to_vec();

        let mut borrowed =
            unsafe { gpu.target_from_texture(owned.texture(), w, h) }.expect("borrowed target");
        assert_eq!(borrowed.format(), format, "the format was not read off the texture");
        assert!(
            gpu.read_pixels(&mut borrowed).is_err(),
            "a borrowed target claimed to have pixels to read back",
        );
        gpu.draw(&mut borrowed, &geometry, &[inst], &params, Mode::Subpixel).expect("draw");
        gpu.wait(&mut borrowed).expect("wait");
        let through_borrow = gpu.read_pixels(&mut owned).expect("read").to_vec();

        assert_eq!(direct, through_borrow, "{format:?}: the borrowed path drew something else");
        assert!(direct.chunks_exact(4).any(|p| p[3] > 0), "{format:?}: nothing was drawn at all");
    }
}

// Rendering into someone else's swapchain means choosing the background, which a hardcoded
// transparent clear never allowed.
#[test]
fn a_target_clears_to_the_color_it_was_given() {
    let Some(gpu) = renderer() else { return };
    let mut batch = GpuBatch::new();
    let _ = one_glyph(&mut batch);
    let geometry = gpu.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();
    let mut t = gpu.target(64, 64).expect("target");

    let transparent = daegun::paint::Rgba { r: 0, g: 0, b: 0, a: 0 };
    assert_eq!(t.clear(), Some(transparent), "the default clear is no longer transparent black");

    let slate = daegun::paint::Rgba { r: 20, g: 40, b: 60, a: 255 };
    t.set_clear(Some(slate));
    gpu.draw(&mut t, &geometry, &[], &params, Mode::Subpixel).expect("draw");
    let px = gpu.read_pixels(&mut t).expect("read");
    assert!(
        px.chunks_exact(4).all(|p| p == [20, 40, 60, 255]),
        "the target did not clear to the color it was given",
    );
}

// Every draw used to open a render pass that clears, so a second geometry erased the first and a
// two-font page needed two targets. Loading instead of clearing is what lets them layer.
#[test]
fn a_second_geometry_can_draw_over_the_first() {
    let Some(gpu) = renderer() else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let params = SubpixelParams::default();
    let (w, h) = (128u32, 64u32);

    let mut left_batch = GpuBatch::new();
    let left_inst = face
        .glyph(&mut left_batch, 37)
        .expect("glyph")
        .instance([6.0, 6.0], 48.0, [48.0, 48.0], [1.0; 4]);
    let left_geo = gpu.geometry(&left_batch).expect("geometry");

    let mut right_batch = GpuBatch::new();
    let right_inst = face
        .glyph(&mut right_batch, 50)
        .expect("glyph")
        .instance([72.0, 6.0], 48.0, [48.0, 48.0], [1.0; 4]);
    let right_geo = gpu.geometry(&right_batch).expect("geometry");

    let halves = |px: &[u8]| {
        let (mut l, mut r) = (0usize, 0usize);
        for y in 0..h as usize {
            for x in 0..w as usize {
                if px[(y * w as usize + x) * 4 + 3] > 0 {
                    if x < 64 { l += 1 } else { r += 1 }
                }
            }
        }
        (l, r)
    };

    let mut layered = gpu.target(w, h).expect("target");
    gpu.draw(&mut layered, &left_geo, &[left_inst], &params, Mode::Subpixel).expect("first");
    layered.set_clear(None);
    gpu.draw(&mut layered, &right_geo, &[right_inst], &params, Mode::Subpixel).expect("second");
    let (l, r) = halves(gpu.read_pixels(&mut layered).expect("read"));
    assert!(l > 0 && r > 0, "layering lost a draw: left {l}, right {r}");

    // and with the clear left in place the old behavior still holds, so nothing changed by default
    let mut cleared = gpu.target(w, h).expect("target");
    gpu.draw(&mut cleared, &left_geo, &[left_inst], &params, Mode::Subpixel).expect("first");
    gpu.draw(&mut cleared, &right_geo, &[right_inst], &params, Mode::Subpixel).expect("second");
    let (l, r) = halves(gpu.read_pixels(&mut cleared).expect("read"));
    assert_eq!(l, 0, "a clearing draw no longer wipes what came before it");
    assert!(r > 0, "the second draw did not land");
}
