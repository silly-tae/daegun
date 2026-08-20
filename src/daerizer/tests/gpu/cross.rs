use super::face::Face;
use daegun::daerizer::daegpu::{GpuBatch, SubpixelParams, ffi, vk};

#[test]
fn metal_and_vulkan_draw_the_same_glyphs() {
    let (Ok(metal), Ok(vulkan)) = (ffi::Renderer::new(), vk::Renderer::new()) else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let params = SubpixelParams::default();
    let (w, h) = (128u32, 128u32);
    let px = 96.0f32;
    let mut mt = metal.target(w, h).expect("metal target");
    let mut vt = vulkan.target(w, h).expect("vulkan target");

    let (mut checked, mut worst, mut inked, mut differing) = (0usize, 0i32, 0usize, 0usize);
    let mut sum = 0f64;
    let mut batch = GpuBatch::new();
    for gid in [36u16, 50, 37, 74, 90, 25] {
        let Some(slot) = face.glyph(&mut batch, gid) else { continue };
        let mg = metal.geometry(&batch).expect("metal geometry");
        let vg = vulkan.geometry(&batch).expect("vulkan geometry");
        let inst = slot.instance([16.0, 16.0], px, [px, px], [1.0, 1.0, 1.0, 1.0]);

        metal.draw(&mut mt, &mg, &[inst], &params, ffi::Mode::Grayscale).expect("metal draw");
        vulkan.draw(&mut vt, &vg, &[inst], &params, vk::Mode::Grayscale).expect("vulkan draw");
        let a = metal.read_pixels(&mut mt).expect("metal read").to_vec();
        let b = vulkan.read_pixels(&mut vt).expect("vulkan read");
        assert_eq!(a.len(), b.len(), "the two targets are not the same size");

        for i in (3..a.len()).step_by(4) {
            let delta = (i32::from(a[i]) - i32::from(b[i])).abs();
            worst = worst.max(delta);
            sum += f64::from(delta);
            if delta > 0 { differing += 1 }
            if a[i] > 0 { inked += 1 }
            checked += 1;
        }
    }
    assert!(checked > 50_000, "only {checked} pixels compared");
    assert!(inked > 2_000, "only {inked} pixels took ink, so there was nothing to compare");

    let mean = sum / checked as f64;
    let rate = differing as f64 / checked as f64;
    assert!(mean < 0.001, "mean |Metal - Vulkan| is {mean:.5} of 255; both agree with `eval` exactly, so they must agree with each other");
    assert!(rate < 0.01, "{differing} of {checked} pixels ({:.2}%) differ at all", rate * 100.0);
    assert!(worst <= 2, "a pixel differs by {worst} of 255; this should be 0");
}

#[test]
fn metal_and_vulkan_describe_the_same_gpu_the_same_way() {
    let (Ok(m), Ok(v)) = (ffi::Renderer::new(), vk::Renderer::new()) else { return };
    let (pm, pv) = (m.profile(), v.profile());
    eprintln!("metal  profile: name={:?} kind={:?}", pm.name, pm.kind);
    eprintln!("vulkan profile: name={:?} kind={:?}", pv.name, pv.kind);

    assert_eq!(
        pm.kind, pv.kind,
        "the two backends disagree about a GPU they are both drawing with",
    );

    use daegun::daerizer::draw::DeviceKind;
    assert!(
        matches!(pm.kind, DeviceKind::Integrated | DeviceKind::Discrete),
        "Metal reported {:?}, so `hasUnifiedMemory` was not answered on a platform that has it \
         since macOS 10.15",
        pm.kind,
    );
}
