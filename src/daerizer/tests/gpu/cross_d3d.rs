use super::face::Face;
use daegun::daerizer::daegpu::{GpuBatch, SubpixelParams, d3d11, d3d12};

#[test]
fn d3d11_and_d3d12_draw_the_same_glyphs() {
    let (Ok(eleven), Ok(twelve)) = (d3d11::Renderer::new(), d3d12::Renderer::new()) else { return };
    let face = Face::load("eb-garamond/EBGaramond.ttf");
    let params = SubpixelParams::default();
    let (w, h) = (128u32, 128u32);
    let px = 96.0f32;
    let mut t11 = eleven.target(w, h).expect("d3d11 target");
    let mut t12 = twelve.target(w, h).expect("d3d12 target");

    let (mut checked, mut worst, mut inked, mut differing) = (0usize, 0i32, 0usize, 0usize);
    let mut sum = 0f64;
    let mut batch = GpuBatch::new();
    for gid in [36u16, 50, 37, 74, 90, 25] {
        let Some(slot) = face.glyph(&mut batch, gid) else { continue };
        let g11 = eleven.geometry(&batch).expect("d3d11 geometry");
        let g12 = twelve.geometry(&batch).expect("d3d12 geometry");
        let inst = slot.instance([16.0, 16.0], px, [px, px], [1.0, 1.0, 1.0, 1.0]);

        eleven.draw(&mut t11, &g11, &[inst], &params, d3d11::Mode::Grayscale).expect("d3d11 draw");
        twelve.draw(&mut t12, &g12, &[inst], &params, d3d12::Mode::Grayscale).expect("d3d12 draw");
        let a = eleven.read_pixels(&mut t11).expect("d3d11 read").to_vec();
        let b = twelve.read_pixels(&mut t12).expect("d3d12 read");
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
    std::eprintln!("d3d11-vs-d3d12: mean {mean:.5}  rate {:.4}%  worst {worst}", rate * 100.0);
    assert!(mean < 0.001, "mean |D3D11 - D3D12| is {mean:.5} of 255; both agree with `eval` exactly, so they must agree with each other");
    assert!(rate < 0.01, "{differing} of {checked} pixels ({:.2}%) differ at all", rate * 100.0);
    assert!(worst <= 2, "a pixel differs by {worst} of 255; this should be 0");
}

#[test]
fn the_two_backends_describe_the_same_adapter_the_same_way() {
    let (Ok(a), Ok(b)) = (d3d11::Renderer::new(), d3d12::Renderer::new()) else { return };
    let (p11, p12) = (a.profile(), b.profile());
    eprintln!("d3d11 profile: name={:?} kind={:?}", p11.name, p11.kind);
    eprintln!("d3d12 profile: name={:?} kind={:?}", p12.name, p12.kind);

    assert_eq!(
        p11.name, p12.name,
        "the two backends name the same adapter differently",
    );
    assert_eq!(
        p11.kind, p12.kind,
        "the two backends disagree about what kind of adapter this is, which is a fact about the \
         hardware rather than about the API asking",
    );
    assert_eq!(
        a.is_software(), b.is_software(),
        "the two backends disagree about whether this adapter renders in software",
    );

    use daegun::daerizer::draw::DeviceKind;
    if !a.is_software() {
        assert!(
            matches!(p12.kind, DeviceKind::Integrated | DeviceKind::Discrete),
            "a hardware adapter reported {:?}, so neither CheckFeatureSupport query answered",
            p12.kind,
        );
    }
}
