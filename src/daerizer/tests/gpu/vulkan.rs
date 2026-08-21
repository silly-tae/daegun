use daegun::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};
use daegun::daerizer::daegpu::{GpuBatch, SubpixelParams, eval, vk};

fn renderer() -> Option<vk::Renderer> {
    match vk::Renderer::new() {
        Ok(r) => Some(r),
        Err(vk::Error::NoDevice) => None,
        Err(e) => panic!("the backend tried and failed rather than reporting no device: {e}"),
    }
}

#[test]
fn a_renderer_is_created_or_absent_and_never_broken() {
    match vk::Renderer::new() {
        Ok(renderer) => {
            let name = renderer.device_name();
            assert!(!name.is_empty(), "a device was created but reports no name");
            assert!(name.len() < 256, "device name is {} bytes, so the NUL was missed", name.len());
            std::eprintln!("vulkan: {name}");
        }
        Err(vk::Error::NoDevice) => {
            std::eprintln!("vulkan: no device on this machine, which is a valid outcome");
        }
        Err(other) => panic!("the backend tried and failed rather than reporting no device: {other}"),
    }
}

#[test]
fn two_renderers_can_exist_at_once() {
    let (a, b) = (vk::Renderer::new(), vk::Renderer::new());
    match (&a, &b) {
        (Ok(a), Ok(b)) => assert_eq!(a.device_name(), b.device_name(), "same machine, same device"),
        (Err(vk::Error::NoDevice), Err(vk::Error::NoDevice)) => {}
        _ => panic!(
            "the two disagreed: first {}, second {}",
            a.as_ref().map(|r| r.device_name()).unwrap_or_else(|e| std::format!("{e}")),
            b.as_ref().map(|r| r.device_name()).unwrap_or_else(|e| std::format!("{e}")),
        ),
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
fn pixel_is_bounded_by_the_target() {
    let Some(r) = renderer() else { return };
    let mut t = r.target(8, 4).expect("target");
    let _ = r.read_pixels(&mut t).expect("read");

    assert_eq!(t.pixel(0, 0), Some([0, 0, 0, 0]));
    assert_eq!(t.pixel(7, 3), Some([0, 0, 0, 0]), "the last pixel is addressable");
    assert_eq!(t.pixel(8, 0), None, "one past the width");
    assert_eq!(t.pixel(0, 4), None, "one past the height");
}

#[test]
fn a_target_with_no_area_is_refused() {
    let Some(r) = renderer() else { return };
    assert!(matches!(r.target(0, 16), Err(vk::Error::BadTarget)));
    assert!(matches!(r.target(16, 0), Err(vk::Error::BadTarget)));
}

#[test]
fn a_target_from_another_renderer_is_refused() {
    let (Some(a), Some(b)) = (renderer(), renderer()) else { return };
    let mut t = a.target(16, 16).expect("target");
    assert!(
        matches!(b.read_pixels(&mut t), Err(vk::Error::BadTarget)),
        "a foreign target was read rather than refused"
    );
    assert!(a.read_pixels(&mut t).is_ok());
}

#[test]
fn reads_are_repeatable_and_targets_are_independent() {
    let Some(r) = renderer() else { return };
    let mut a = r.target(32, 16).expect("a");
    let mut b = r.target(4, 4).expect("b");

    let first: Vec<u8> = r.read_pixels(&mut a).expect("read a").to_vec();
    let _ = r.read_pixels(&mut b).expect("read b");
    let second = r.read_pixels(&mut a).expect("read a again");
    assert_eq!(first.as_slice(), second, "a second read changed the answer");
}

#[test]
fn a_batch_becomes_geometry() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("inter/InterVariable.ttf");
    let mut batch = daegun::daerizer::daegpu::GpuBatch::new();
    let n = face.fill(&mut batch, 1..200, 7);
    assert!(n > 10, "only {n} glyphs made it into the batch");
    assert!(!batch.curves().is_empty());

    let g = r.geometry(&batch).expect("upload");
    assert_eq!(g.revision(), batch.revision(), "geometry records the batch it was built from");

    let empty = daegun::daerizer::daegpu::GpuBatch::new();
    let ge = r.geometry(&empty).expect("an empty batch is not an error");
    assert_eq!(ge.revision(), empty.revision());
}

#[test]
fn geometry_is_independent_of_its_batch() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");

    let mut a = daegun::daerizer::daegpu::GpuBatch::new();
    face.fill(&mut a, 1..40, 3);
    let ga = r.geometry(&a).expect("a");

    let mut b = daegun::daerizer::daegpu::GpuBatch::new();
    face.fill(&mut b, 40..90, 3);
    let gb = r.geometry(&b).expect("b");

    assert_ne!(ga.revision(), gb.revision(), "different batches, different revisions");
    drop(a);
    drop(b);
    assert!(ga.revision() > 0 && gb.revision() > 0, "geometry survived its batch");
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
    r.draw(&mut t, &geometry, &[inst], &SubpixelParams::default(), vk::Mode::Grayscale)
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
    r.draw(&mut t, &geometry, &[], &SubpixelParams::default(), vk::Mode::Grayscale).expect("draw");
    let pixels = r.read_pixels(&mut t).expect("read");
    assert!(pixels.iter().all(|&b| b == 0), "an empty draw put ink on the target");
}

#[test]
fn draws_beyond_the_ring_reuse_their_slots() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("inter/InterVariable.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 36).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();

    let mut t = r.target(64, 64).expect("target");
    for n in 1..=8usize {
        let insts: Vec<_> = (0..n)
            .map(|i| slot.instance([4.0 + i as f32, 4.0], 32.0, [32.0, 32.0], [1.0, 1.0, 1.0, 1.0]))
            .collect();
        r.draw(&mut t, &geometry, &insts, &params, vk::Mode::Grayscale).expect("draw");
        r.wait(&mut t).expect("wait");
    }
    let pixels = r.read_pixels(&mut t).expect("read");
    assert!(pixels.iter().any(|&b| b > 0), "eight draws left the target empty");
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
    r.draw(&mut t, &geometry, &[inst], &SubpixelParams::from_layout(&layout), vk::Mode::Subpixel)
        .expect("draw");
    let pixels = r.read_pixels(&mut t).expect("read");

    let differing = pixels
        .chunks_exact(4)
        .filter(|p| p[0] != p[1] || p[1] != p[2])
        .count();
    assert!(differing > 20, "only {differing} pixels have per-channel coverage, so dual-source did nothing");
}

#[test]
fn a_draw_into_a_foreign_target_is_refused() {
    let (Some(a), Some(b)) = (renderer(), renderer()) else { return };
    let batch = GpuBatch::new();
    let g = a.geometry(&batch).expect("geometry");
    let mut t = a.target(16, 16).expect("target");
    assert!(matches!(
        b.draw(&mut t, &g, &[], &SubpixelParams::default(), vk::Mode::Grayscale),
        Err(vk::Error::BadTarget)
    ));
}

#[test]
fn vulkan_grayscale_matches_the_reference_evaluator() {
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

        gpu.draw(&mut target, &geometry, &[inst], &params, vk::Mode::Grayscale).expect("draw");
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
    std::eprintln!("vulkan-vs-eval: mean {mean:.5}  over-16 rate {:.4}%  worst {worst}", rate * 100.0);
    assert!(mean < 0.003, "mean |delta| is {mean:.5} of 255; the interpolated coordinate scored 0.00425 on Apple and 0.00456 on AMD");
    assert!(rate < 0.0002, "{over} of {checked} pixels ({:.4}%) differ by more than 16", rate * 100.0);
    assert!(worst <= 2, "a pixel differs by {worst} of 255; this should be 0");
}

#[test]
fn a_target_borrows_its_renderer() {
    let Some(r) = renderer() else { return };
    let mut t = r.target(32, 32).expect("target");
    assert!(r.read_pixels(&mut t).is_ok());
    drop(t);
    drop(r);
}

#[test]
fn a_vulkan_device_reports_what_kind_it_is() {
    use daegun::daerizer::draw::{DeviceKind, Policy, Rendered, Request, route};

    let Some(r) = renderer() else { return };
    let profile = r.profile();

    assert_eq!(profile.name, r.device_name(), "the profile renamed the device");
    assert!(!profile.name.is_empty(), "the driver named the device as nothing");
    assert!(
        matches!(
            profile.kind,
            DeviceKind::Discrete | DeviceKind::Integrated | DeviceKind::Virtual
                | DeviceKind::Software | DeviceKind::Unknown
        ),
        "an unclassifiable device kind: {:?}", profile.kind,
    );

    let decided = route(Ok(()), &Request::at(32.0), Some(&profile), &Policy::default());
    let expected = if profile.kind.is_software() { Rendered::Cpu } else { Rendered::Gpu };
    assert_eq!(
        decided, expected,
        "a {:?} device named {:?} routed to {decided:?}", profile.kind, profile.name,
    );
}

#[test]
fn a_device_without_dual_source_blending_still_draws_grayscale() {
    if std::env::var("DAEGUN_VK_NO_DUAL_SRC").is_err() {
        let exe = std::env::current_exe().expect("this test binary");
        let out = std::process::Command::new(exe)
            .args(["a_device_without_dual_source_blending_still_draws_grayscale", "--nocapture"])
            .env("DAEGUN_VK_NO_DUAL_SRC", "1")
            .output()
            .expect("re-running this binary");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            out.status.success(),
            "the degraded path failed:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(
            stdout.contains("1 passed"),
            "the child ran no test, so nothing was proved:\n{stdout}",
        );
        return;
    }

    let Some(r) = renderer() else { return };

    assert!(!r.supports_subpixel(), "the flag did not take effect, so this proves nothing");

    let mut batch = GpuBatch::new();
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let Some(slot) = (1..80u16).find_map(|g| face.glyph(&mut batch, g)) else {
        panic!("no glyph in the fixture produced an outline")
    };
    let geometry = r.geometry(&batch).expect("geometry");
    let mut target = r.target(64, 64).expect("target");
    let inst = slot.instance([8.0, 8.0], 48.0, [48.0, 48.0], [1.0, 1.0, 1.0, 1.0]);
    let params = SubpixelParams::default();
    r.draw(&mut target, &geometry, &[inst], &params, vk::Mode::Grayscale)
        .expect("grayscale draw on a device with no dual-source blending");
    let pixels = r.read_pixels(&mut target).expect("read").to_vec();
    assert!(pixels.iter().any(|&b| b != 0), "the grayscale draw produced nothing");

    let err = r
        .draw(&mut target, &geometry, &[inst], &params, vk::Mode::Subpixel)
        .expect_err("a subpixel draw was accepted by a device that cannot blend it");
    assert!(
        matches!(err, vk::Error::Unsupported(_)),
        "the refusal was {err:?} rather than Unsupported",
    );
}

#[test]
fn the_native_handle_is_stable_and_distinct() {
    let Ok(r) = vk::Renderer::new() else { return };
    let (mut a, b) = (r.target(64, 48).expect("target"), r.target(64, 48).expect("target"));

    let h = unsafe { a.image() };
    assert!(h != 0, "the target handed out a null texture");
    assert_eq!(h, unsafe { a.image() }, "the handle changed between two calls");
    assert_ne!(h, unsafe { b.image() }, "two targets named one texture");

    r.read_pixels(&mut a).expect("read");
    assert_eq!(h, unsafe { a.image() }, "a read replaced the texture");
}

// Rendering into a surface daegun did not create means choosing the background, which a hardcoded
// transparent clear never allowed.
#[test]
fn a_target_clears_to_the_color_it_was_given() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let _ = face.glyph(&mut batch, 36).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");
    let mut t = r.target(64, 64).expect("target");

    let transparent = daegun::paint::Rgba { r: 0, g: 0, b: 0, a: 0 };
    assert_eq!(t.clear(), Some(transparent), "the default clear is no longer transparent black");

    let slate = daegun::paint::Rgba { r: 20, g: 40, b: 60, a: 255 };
    t.set_clear(Some(slate));
    r.draw(&mut t, &geometry, &[], &SubpixelParams::default(), vk::Mode::Grayscale)
        .expect("draw");
    let px = r.read_pixels(&mut t).expect("read");
    assert!(
        px.chunks_exact(4).all(|p| p == [20, 40, 60, 255]),
        "the target did not clear to the color it was given",
    );
}

// A surface daegun did not create picks its own byte order. The two pipelines must put the same
// colors in the same places, swapped.
#[test]
fn the_two_byte_orders_agree_once_swapped() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");
    let (w, h) = (96u32, 96u32);
    // Three different channels, or a byte swap would be invisible.
    let inst = slot.instance([8.0, 8.0], 64.0, [64.0, 64.0], [1.0, 0.15, 0.0, 1.0]);

    let shot = |format| {
        let mut t = r.target_with_format(w, h, format).expect("target");
        r.draw(&mut t, &geometry, &[inst], &SubpixelParams::default(), vk::Mode::Grayscale)
            .expect("draw");
        r.read_pixels(&mut t).expect("read").to_vec()
    };
    let rgba = shot(vk::Format::Rgba8Unorm);
    let bgra = shot(vk::Format::Bgra8Unorm);

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

// Every draw opened a clearing pass, so a second geometry erased the first. Loading instead is what
// lets them layer.
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
    r.draw(&mut layered, &left_geo, &[left], &params, vk::Mode::Grayscale).expect("first");
    layered.set_clear(None);
    r.draw(&mut layered, &right_geo, &[right], &params, vk::Mode::Grayscale).expect("second");
    let (l, rt) = halves(r.read_pixels(&mut layered).expect("read"));
    assert!(l > 0 && rt > 0, "layering lost a draw: left {l}, right {rt}");

    // with the clear left in place the old behavior still holds, so nothing changed by default
    let mut cleared = r.target(w, h).expect("target");
    r.draw(&mut cleared, &left_geo, &[left], &params, vk::Mode::Grayscale).expect("first");
    r.draw(&mut cleared, &right_geo, &[right], &params, vk::Mode::Grayscale).expect("second");
    let (l, rt) = halves(r.read_pixels(&mut cleared).expect("read"));
    assert_eq!(l, 0, "a clearing draw no longer wipes what came before it");
    assert!(rt > 0, "the second draw did not land");
}

// Rendering into a caller's swapchain needs the caller's device, because a swapchain image belongs
// to the device that made it. Adoption is proved by drawing the same thing on a device daegun did
// not create and getting the same pixels.
#[test]
fn a_renderer_can_adopt_a_device_it_did_not_create() {
    let Some(owner) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("a glyph with an outline");
    let inst = slot.instance([8.0, 8.0], 64.0, [64.0, 64.0], [1.0; 4]);
    let params = SubpixelParams::default();

    let shot = |r: &vk::Renderer| {
        let geometry = r.geometry(&batch).expect("geometry");
        let mut t = r.target(96, 96).expect("target");
        r.draw(&mut t, &geometry, &[inst], &params, vk::Mode::Grayscale).expect("draw");
        r.read_pixels(&mut t).expect("read").to_vec()
    };
    let from_owner = shot(&owner);

    let (instance, physical, device, family) = unsafe { owner.handles() };
    let adopted = unsafe { vk::Renderer::from_device(instance, physical, device, family, true) }
        .expect("adopts a live device");
    assert_eq!(adopted.device_name(), owner.device_name(), "adopted a different device");
    let from_adopted = shot(&adopted);

    assert_eq!(from_owner, from_adopted, "the adopted device drew something else");
    assert!(from_owner.chunks_exact(4).any(|p| p[3] > 0), "neither renderer drew anything");

    // The adopted renderer must go first: it does not own the device, and dropping the owner would
    // destroy it out from under this one.
    drop(adopted);
    drop(owner);
}

#[test]
fn adopting_a_null_device_is_refused_rather_than_crashing() {
    let Some(owner) = renderer() else { return };
    let (instance, physical, _, family) = unsafe { owner.handles() };
    assert!(
        unsafe { vk::Renderer::from_device(instance, physical, core::ptr::null_mut(), family, true) }
            .is_err(),
        "a null device was accepted",
    );
}

// A borrowed target has no staging of its own, so it is pointed at the image of an owned one: the
// draw goes through the borrowed path and the pixels come back through the owner.
#[test]
fn a_borrowed_image_draws_what_an_owned_one_does() {
    let Some(r) = renderer() else { return };
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("a glyph with an outline");
    let geometry = r.geometry(&batch).expect("geometry");
    let params = SubpixelParams::default();
    let (w, h) = (96u32, 96u32);
    let inst = slot.instance([8.0, 8.0], 64.0, [64.0, 64.0], [1.0; 4]);

    for format in [vk::Format::Rgba8Unorm, vk::Format::Bgra8Unorm] {
        let mut owned = r.target_with_format(w, h, format).expect("owned target");
        r.draw(&mut owned, &geometry, &[inst], &params, vk::Mode::Grayscale).expect("draw");
        let direct = r.read_pixels(&mut owned).expect("read").to_vec();

        let mut borrowed = unsafe { r.target_from_image(owned.image(), w, h, format) }
            .expect("a target over the owned image");
        assert!(
            r.read_pixels(&mut borrowed).is_err(),
            "a borrowed target claimed to have pixels to read back",
        );
        r.draw(&mut borrowed, &geometry, &[inst], &params, vk::Mode::Grayscale).expect("draw");
        r.wait(&mut borrowed).expect("wait");
        drop(borrowed);

        let through_borrow = r.read_pixels(&mut owned).expect("read").to_vec();
        assert_eq!(direct, through_borrow, "{format:?}: the borrowed path drew something else");
        assert!(direct.chunks_exact(4).any(|p| p[3] > 0), "{format:?}: nothing was drawn at all");
    }
}
