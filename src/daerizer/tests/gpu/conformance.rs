use daegun::daerizer::daegpu::backend::{Backend, Refusal, Surface, Uploaded};
use daegun::daerizer::daegpu::{GpuBatch, Mode, SubpixelParams};

fn renderer<B: Backend>() -> Option<B> {
    match B::new() {
        Ok(r) => Some(r),
        Err(e) if B::refusal(&e) == Refusal::NoDevice => None,
        Err(e) => panic!("{}: tried and failed rather than reporting no device: {e}", B::NAME),
    }
}

fn one_glyph<B: Backend>(px: f32) -> (GpuBatch, daegun::daerizer::daegpu::GlyphInstance) {
    let face = crate::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("a glyph with an outline");
    let inst = slot.instance([8.0, 8.0], px, [px, px], [1.0; 4]);
    let _ = core::marker::PhantomData::<B>;
    (batch, inst)
}

pub fn no_area<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    for (w, h) in [(0u32, 16u32), (16, 0), (0, 0)] {
        match r.target(w, h) {
            Ok(_) => panic!("{}: target({w}, {h}) was accepted", B::NAME),
            Err(e) => assert_eq!(
                B::refusal(&e),
                Refusal::BadTarget,
                "{}: target({w}, {h}) refused as {:?} rather than BadTarget",
                B::NAME,
                B::refusal(&e),
            ),
        }
    }
}

pub fn a_new_target_is_transparent<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let mut t = r.target(32, 32).expect("target");
    assert_eq!(t.width(), 32);
    assert_eq!(t.height(), 32);
    let px = r.read_pixels(&mut t).expect("read");
    assert_eq!(px.len(), 32 * 32 * 4, "{}: wrong length", B::NAME);
    assert!(px.iter().all(|&b| b == 0), "{}: a new target was not transparent black", B::NAME);
}

pub fn pixel_is_bounded<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let (batch, inst) = one_glyph::<B>(48.0);
    let g = r.geometry(&batch).expect("geometry");
    let mut t = r.target(64, 64).expect("target");
    r.draw(&mut t, &g, &[inst], &SubpixelParams::default(), Mode::Grayscale).expect("draw");
    r.read_pixels(&mut t).expect("read");

    assert!(t.pixel(64, 0).is_none(), "{}: x == width was inside", B::NAME);
    assert!(t.pixel(0, 64).is_none(), "{}: y == height was inside", B::NAME);
    assert!(t.pixel(u32::MAX, u32::MAX).is_none(), "{}: the far corner was inside", B::NAME);

    let flat = t.pixels().to_vec();
    for (x, y) in [(0u32, 0u32), (31, 31), (63, 63)] {
        let i = (y as usize * 64 + x as usize) * 4;
        let want = [flat[i], flat[i + 1], flat[i + 2], flat[i + 3]];
        assert_eq!(t.pixel(x, y), Some(want), "{}: pixel disagreed with pixels at ({x}, {y})", B::NAME);
    }
}

pub fn pixels_arrive_only_when_read<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let (batch, inst) = one_glyph::<B>(48.0);
    let g = r.geometry(&batch).expect("geometry");
    let mut t = r.target(64, 64).expect("target");

    r.draw(&mut t, &g, &[inst], &SubpixelParams::default(), Mode::Grayscale).expect("draw");
    let before = t.pixels().iter().filter(|&&b| b > 0).count();
    assert_eq!(before, 0, "{}: a draw alone put {before} bytes of ink where a read should", B::NAME);

    let after = r.read_pixels(&mut t).expect("read").iter().filter(|&&b| b > 0).count();
    assert!(after > 200, "{}: only {after} bytes took ink after a read", B::NAME);
}

pub fn reads_are_repeatable<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let (batch, inst) = one_glyph::<B>(40.0);
    let g = r.geometry(&batch).expect("geometry");
    let mut t = r.target(48, 48).expect("target");
    r.draw(&mut t, &g, &[inst], &SubpixelParams::default(), Mode::Grayscale).expect("draw");

    let first = r.read_pixels(&mut t).expect("read").to_vec();
    let second = r.read_pixels(&mut t).expect("read again").to_vec();
    assert_eq!(first, second, "{}: two reads of one target disagreed", B::NAME);
}

pub fn geometry_outlives_its_batch<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let (batch, inst) = one_glyph::<B>(40.0);
    let revision = batch.revision();
    let g = r.geometry(&batch).expect("geometry");
    assert_eq!(g.revision(), revision, "{}: geometry reported a different revision", B::NAME);
    drop(batch);

    let mut t = r.target(48, 48).expect("target");
    r.draw(&mut t, &g, &[inst], &SubpixelParams::default(), Mode::Grayscale).expect("draw");
    let inked = r.read_pixels(&mut t).expect("read").iter().filter(|&&b| b > 0).count();
    assert!(inked > 100, "{}: only {inked} bytes of ink after the batch was dropped", B::NAME);
}

pub fn instance_counts_grow<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let face = crate::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("a glyph with an outline");
    let g = r.geometry(&batch).expect("geometry");
    let mut t = r.target(160, 48).expect("target");

    let mut previous = 0usize;
    for n in [1usize, 2, 5, 9] {
        let instances: Vec<_> = (0..n)
            .map(|i| slot.instance([4.0 + i as f32 * 16.0, 8.0], 24.0, [24.0, 24.0], [1.0; 4]))
            .collect();
        r.draw(&mut t, &g, &instances, &SubpixelParams::default(), Mode::Grayscale).expect("draw");
        let inked = r.read_pixels(&mut t).expect("read").iter().filter(|&&b| b > 0).count();
        assert!(
            inked > previous,
            "{}: {n} instances inked {inked} bytes, no more than {previous} for fewer",
            B::NAME,
        );
        previous = inked;
    }
}

pub fn a_draw_is_the_right_way_up<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let (batch, _) = one_glyph::<B>(0.0);
    let face = crate::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut b2 = GpuBatch::new();
    let slot = face.glyph(&mut b2, 37).expect("glyph");
    let g = r.geometry(&b2).expect("geometry");
    let _ = batch;

    let mut t = r.target(64, 64).expect("target");
    let inst = slot.instance([16.0, 4.0], 40.0, [40.0, 40.0], [1.0; 4]);
    r.draw(&mut t, &g, &[inst], &SubpixelParams::default(), Mode::Grayscale).expect("draw");
    let px = r.read_pixels(&mut t).expect("read").to_vec();

    let ink_in = |rows: core::ops::Range<usize>| -> usize {
        rows.map(|y| px[y * 64 * 4..(y + 1) * 64 * 4].iter().filter(|&&b| b > 0).count()).sum()
    };
    let (top, bottom) = (ink_in(0..32), ink_in(32..64));
    assert!(
        bottom > top,
        "{}: a glyph placed near the bottom inked {top} above and {bottom} below — it is upside down",
        B::NAME,
    );
}

pub fn a_target_drawn_by_one_renderer_is_read_correctly_by_another<B: Backend>() {
    let (Some(drawer), Some(reader)) = (renderer::<B>(), renderer::<B>()) else { return };
    let face = crate::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let slot = face.glyph(&mut batch, 37).expect("a glyph with an outline");
    let g = drawer.geometry(&batch).expect("geometry");
    let inst = slot.instance([8.0, 8.0], 48.0, [48.0, 48.0], [1.0; 4]);

    let mut t = drawer.target(64, 64).expect("target");
    drawer
        .draw(&mut t, &g, &[inst], &SubpixelParams::default(), Mode::Grayscale)
        .expect("a renderer must accept the target it made");

    match reader.read_pixels(&mut t) {
        Ok(px) => {
            let inked = px.iter().filter(|&&b| b > 0).count();
            assert!(
                inked > 200,
                "{}: the read was accepted and returned {inked} inked bytes — it raced the draw",
                B::NAME,
            );
            assert!(reader.wait(&mut t).is_ok(), "{}: wait after read must be a no-op", B::NAME);
            assert!(drawer.wait(&mut t).is_ok(), "{}: wait after read must be a no-op", B::NAME);
        }
        Err(e) => {
            assert_eq!(
                B::refusal(&e),
                Refusal::BadTarget,
                "{}: a foreign read was refused as {:?} rather than BadTarget",
                B::NAME,
                B::refusal(&e),
            );
            let inked =
                drawer.read_pixels(&mut t).expect("the owner must read").iter().filter(|&&b| b > 0).count();
            assert!(inked > 200, "{}: the owning renderer read {inked} inked bytes", B::NAME);
        }
    }
}

pub fn subpixel_is_served_or_refused<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let (batch, inst) = one_glyph::<B>(40.0);
    let g = r.geometry(&batch).expect("geometry");
    let mut t = r.target(48, 48).expect("target");

    let params = SubpixelParams::from_layout(&daegun::daecore::daemachine::subpixel::SubpixelLayout::horizontal(
        daegun::daecore::daemachine::subpixel::StripeOrder::Rgb,
    ));
    let drew = r.draw(&mut t, &g, &[inst], &params, Mode::Subpixel);
    match drew {
        Ok(()) => {
            assert!(r.supports_subpixel(), "{}: drew subpixel while reporting it unsupported", B::NAME);
            let px = r.read_pixels(&mut t).expect("read").to_vec();
            let differs = px
                .chunks_exact(4)
                .any(|p| p[3] > 0 && (p[0] != p[1] || p[1] != p[2]));
            assert!(differs, "{}: subpixel drew no channel difference at all", B::NAME);
        }
        Err(e) => {
            assert!(!r.supports_subpixel(), "{}: refused subpixel while reporting support", B::NAME);
            assert_eq!(
                B::refusal(&e),
                Refusal::Unsupported,
                "{}: subpixel refused as {:?} rather than Unsupported",
                B::NAME,
                B::refusal(&e),
            );
        }
    }
}

pub fn the_profile_names_the_device_it_describes<B: Backend>() {
    let Some(r) = renderer::<B>() else { return };
    let name = r.device_name();
    let profile = r.profile();
    assert_eq!(profile.name, name, "{}: the profile renamed the device", B::NAME);
    assert!(!name.is_empty(), "{}: the device has no name at all", B::NAME);
    assert!(
        !name.contains("(hardware)") && !name.contains("(software)"),
        "{}: the name carries the kind, which is what `DeviceProfile::kind` is for: {name:?}",
        B::NAME,
    );
    assert!(
        !name.contains("feature level"),
        "{}: the name carries the feature level: {name:?}",
        B::NAME,
    );
}

macro_rules! conformance {
    ($name:ident, $backend:ty) => {
        mod $name {
            #[test] fn a_target_with_no_area_is_refused() { super::no_area::<$backend>() }
            #[test] fn a_new_target_is_transparent() { super::a_new_target_is_transparent::<$backend>() }
            #[test] fn pixel_is_bounded_by_the_target() { super::pixel_is_bounded::<$backend>() }
            #[test] fn pixels_arrive_only_when_read() { super::pixels_arrive_only_when_read::<$backend>() }
            #[test] fn reads_are_repeatable() { super::reads_are_repeatable::<$backend>() }
            #[test] fn geometry_outlives_its_batch() { super::geometry_outlives_its_batch::<$backend>() }
            #[test] fn instance_counts_grow() { super::instance_counts_grow::<$backend>() }
            #[test] fn a_draw_is_the_right_way_up() { super::a_draw_is_the_right_way_up::<$backend>() }
            #[test] fn subpixel_is_served_or_refused() { super::subpixel_is_served_or_refused::<$backend>() }
            #[test]
            fn the_profile_names_the_device_it_describes() {
                super::the_profile_names_the_device_it_describes::<$backend>()
            }
            #[test]
            fn a_target_drawn_by_one_renderer_is_read_correctly_by_another() {
                super::a_target_drawn_by_one_renderer_is_read_correctly_by_another::<$backend>()
            }
        }
    };
}

conformance!(vulkan, daegun::daerizer::daegpu::vk::Renderer);

#[cfg(target_vendor = "apple")]
conformance!(metal, daegun::daerizer::daegpu::ffi::Renderer);

#[cfg(windows)]
conformance!(d3d11, daegun::daerizer::daegpu::d3d11::Renderer);

#[cfg(windows)]
conformance!(d3d12, daegun::daerizer::daegpu::d3d12::Renderer);
