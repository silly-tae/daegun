use daegun::{
    DeviceKind, DeviceProfile, DrawTarget, DrawnGlyph, Font, HintMode, Policy, Prefer,
    RasterOptions, Refusal, StripeOrder, SubpixelLayout,
};
use daegun::daerizer::daegpu::GpuBatch;

const GARAMOND: &str = "eb-garamond/EBGaramond.ttf";
const COLR: &str = "colr-v1-test-glyphs/test_glyphs.ttf";

fn font(rel: &str) -> Font {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_bytes(&bytes).unwrap_or_else(|e| panic!("{rel} did not parse: {e}"))
}

fn gpu() -> DeviceProfile {
    DeviceProfile::new(DeviceKind::Discrete, "a card")
}

fn which(d: &DrawnGlyph) -> &'static str {
    match d {
        DrawnGlyph::Nothing => "nothing",
        DrawnGlyph::Cpu(_) => "cpu",
        DrawnGlyph::Gpu(_) => "gpu",
        DrawnGlyph::GpuColor(_) => "gpu-color",
        DrawnGlyph::Scene(_) => "scene",
        DrawnGlyph::Reference(_) => "reference",
        DrawnGlyph::BatchFull => "batch-full",
        DrawnGlyph::Refused(_) => "refused",
    }
}

#[test]
fn a_mixed_page_reaches_four_engines_in_one_pass() {
    let f = font(GARAMOND);
    let device = gpu();
    let mut batch = GpuBatch::new();
    let mut target = DrawTarget::new(&mut batch, &device);
    let opts = RasterOptions::default();

    let heading = f.glyph_id('C' as u32).expect("C");
    let body = f.glyph_id('t' as u32).expect("t");
    let space = f.glyph_id(' ' as u32).expect("space");

    let big = f.draw_glyph(&mut target, heading, 48.0, &[], &opts, None);
    assert_eq!(which(&big), "gpu", "48px with a real device did not reach the GPU");

    let small = f.draw_glyph(&mut target, body, 12.0, &[], &opts, None);
    assert_eq!(which(&small), "cpu", "12px did not go where the hinting is");
    let bitmap = small.bitmap().expect("the CPU path produced no pixels");
    assert!(!bitmap.bitmap.is_empty(), "the CPU path produced an empty bitmap");
    assert_eq!(
        bitmap.bitmap.len(),
        bitmap.metrics.width * bitmap.metrics.height,
        "a grayscale bitmap is not one byte per pixel",
    );

    let none = f.draw_glyph(&mut target, space, 12.0, &[], &opts, None);
    assert_eq!(which(&none), "nothing", "a space was not routed as nothing");
    assert!(none.is_ok(), "a space was reported as a failure");

    for d in [&big, &small, &none] {
        assert!(d.is_ok(), "{} was not accounted for", which(d));
    }
}

#[test]
fn cpu_only_arrives_by_three_different_roads() {
    let f = font(GARAMOND);
    let gid = f.glyph_id('B' as u32).expect("B");
    let opts = RasterOptions::default();

    let mut batch = GpuBatch::new();
    let mut headless = DrawTarget::cpu_only(&mut batch);
    let a = f.draw_glyph(&mut headless, gid, 48.0, &[], &opts, None);
    assert_eq!(which(&a), "cpu", "no device did not route to the CPU");

    let warp = DeviceProfile::new(DeviceKind::Software, "Microsoft Basic Render Driver");
    let mut batch = GpuBatch::new();
    let mut software = DrawTarget::new(&mut batch, &warp);
    let b = f.draw_glyph(&mut software, gid, 48.0, &[], &opts, None);
    assert_eq!(which(&b), "cpu", "a software device was treated as a GPU");

    let device = gpu();
    let mut batch = GpuBatch::new();
    let mut stated =
        DrawTarget::new(&mut batch, &device).with_policy(Policy::prefer(Prefer::Cpu));
    let c = f.draw_glyph(&mut stated, gid, 48.0, &[], &opts, None);
    assert_eq!(which(&c), "cpu", "a stated CPU preference was not honoured");

    let (x, y, z) = (a.bitmap().unwrap(), b.bitmap().unwrap(), c.bitmap().unwrap());
    assert_eq!(x, y, "no device and a software device drew differently");
    assert_eq!(y, z, "a software device and a stated preference drew differently");
}

#[test]
fn the_cpu_only_options_pull_the_glyph_off_the_gpu() {
    let f = font(GARAMOND);
    let gid = f.glyph_id('B' as u32).expect("B");
    let device = gpu();

    let cases: [(&str, RasterOptions); 5] = [
        ("hinting", RasterOptions::default().with_hinting(HintMode::Auto)),
        ("gamma", RasterOptions::default().with_gamma(2.2)),
        ("embolden", RasterOptions::default().with_embolden(0.02)),
        ("oblique", RasterOptions::default().with_oblique(0.25)),
        (
            "stroke",
            RasterOptions::default().with_stroke(daegun::StrokeStyle {
                width: 10.0,
                ..Default::default()
            }),
        ),
    ];
    for (name, opts) in cases {
        let mut batch = GpuBatch::new();
        let mut target = DrawTarget::new(&mut batch, &device);
        let d = f.draw_glyph(&mut target, gid, 64.0, &[], &opts, None);
        assert_eq!(which(&d), "cpu", "{name} was sent to a path that cannot do it");

        let mut batch = GpuBatch::new();
        let mut strict = DrawTarget::new(&mut batch, &device)
            .with_policy(Policy::prefer(Prefer::Gpu).strictly());
        let d = f.draw_glyph(&mut strict, gid, 64.0, &[], &opts, None);
        assert_eq!(
            d,
            DrawnGlyph::Refused(Refusal::PreferenceUnmet),
            "{name} under a strict GPU preference was substituted instead of refused",
        );
        assert!(!d.is_ok(), "a refusal reported itself as accounted for");
    }
}

#[test]
fn the_gpu_path_batches_and_reuses() {
    let f = font(GARAMOND);
    let device = gpu();
    let mut batch = GpuBatch::new();
    let mut target = DrawTarget::new(&mut batch, &device);
    let opts = RasterOptions::default();

    let gid = f.glyph_id('B' as u32).expect("B");
    let first = f.draw_glyph(&mut target, gid, 48.0, &[], &opts, None);
    let again = f.draw_glyph(&mut target, gid, 48.0, &[], &opts, None);
    assert_eq!(first, again, "the same glyph twice produced two different slots");

    let bigger = f.draw_glyph(&mut target, gid, 96.0, &[], &opts, None);
    assert_eq!(first, bigger, "the same glyph at another size rebuilt its curves");

    let other = f.glyph_id('C' as u32).expect("C");
    let second = f.draw_glyph(&mut target, other, 48.0, &[], &opts, None);
    assert_ne!(first, second, "two glyphs landed in one slot");
    assert_eq!(which(&second), "gpu");
}

#[test]
fn the_size_rule_decides_at_its_stated_boundary() {
    let f = font(GARAMOND);
    let device = gpu();
    let gid = f.glyph_id('B' as u32).expect("B");
    let opts = RasterOptions::default();

    for (px, want) in [(8.0, "cpu"), (15.9, "cpu"), (16.0, "gpu"), (64.0, "gpu")] {
        let mut batch = GpuBatch::new();
        let mut target = DrawTarget::new(&mut batch, &device);
        let d = f.draw_glyph(&mut target, gid, px, &[], &opts, None);
        assert_eq!(which(&d), want, "{px}px went to the wrong engine");
    }

    let mut batch = GpuBatch::new();
    let mut any = DrawTarget::new(&mut batch, &device).with_policy(Policy::default().at_any_size());
    let d = f.draw_glyph(&mut any, gid, 8.0, &[], &opts, None);
    assert_eq!(which(&d), "gpu", "the size rule still applied after being turned off");
}

#[test]
fn the_reference_engine_draws_without_a_device() {
    let f = font(GARAMOND);
    let gid = f.glyph_id('B' as u32).expect("B");
    let opts = RasterOptions::default();

    let mut batch = GpuBatch::new();
    let mut target =
        DrawTarget::cpu_only(&mut batch).with_policy(Policy::prefer(Prefer::Reference));
    let drawn = f.draw_glyph(&mut target, gid, 64.0, &[], &opts, None);
    assert_eq!(which(&drawn), "reference", "the reference engine was not used");

    let r = drawn.bitmap().expect("the reference engine produced no pixels");
    assert!(r.metrics.width > 8 && r.metrics.height > 8, "the reference box is implausibly small");
    assert_eq!(
        r.bitmap.len(),
        r.metrics.width * r.metrics.height,
        "a grayscale reference bitmap is not one byte per pixel",
    );
    assert!(r.bitmap.iter().any(|&b| b > 200), "the reference engine drew nothing solid");
    assert!(r.bitmap.contains(&0), "the reference engine drew no background");

    let (w, h) = (r.metrics.width, r.metrics.height);
    let at = |x: usize, y: usize| r.bitmap[y * w + x];
    for x in 0..w {
        assert_eq!(at(x, 0), 0, "the top margin row has ink at x={x}: the box is too low");
        assert_eq!(at(x, h - 1), 0, "the bottom margin row has ink at x={x}: the box is too high");
    }
    for y in 0..h {
        assert_eq!(at(0, y), 0, "the left margin column has ink at y={y}: the box is too far right");
        assert_eq!(at(w - 1, y), 0, "the right margin column has ink at y={y}: the box is too far left");
    }

    let mut cpu_batch = GpuBatch::new();
    let mut cpu = DrawTarget::cpu_only(&mut cpu_batch);
    let c = f
        .draw_glyph(&mut cpu, gid, 64.0, &[], &opts, None)
        .bitmap()
        .expect("the CPU path produced no pixels")
        .clone();

    let ink = |g: &daegun::RasterizedGlyph| {
        let total: f64 = g.bitmap.iter().map(|&b| f64::from(b)).sum();
        let (mut cx, mut cy) = (0.0f64, 0.0f64);
        for (i, &b) in g.bitmap.iter().enumerate() {
            let (x, y) = ((i % g.metrics.width) as f64, (i / g.metrics.width) as f64);
            cx += x * f64::from(b);
            cy += y * f64::from(b);
        }
        (
            total / 255.0,
            cx / total + f64::from(g.metrics.xmin),
            f64::from(g.metrics.ymin) + (g.metrics.height as f64 - 1.0) - cy / total,
        )
    };
    let (ri, rx, ry) = ink(r);
    let (ci, cx, cy) = ink(&c);
    assert!(
        (ri - ci).abs() / ci < 0.06,
        "the two engines disagree about how much ink the glyph has: {ri:.1} against {ci:.1}",
    );
    assert!(
        (rx - cx).abs() < 1.0 && (ry - cy).abs() < 1.0,
        "the two engines drew the glyph in different places: ({rx:.2}, {ry:.2}) against ({cx:.2}, {cy:.2})",
    );
}

#[test]
fn the_reference_engine_resolves_colour_channels() {
    let f = font(GARAMOND);
    let gid = f.glyph_id('B' as u32).expect("B");
    let opts = RasterOptions::default().with_layout(SubpixelLayout::horizontal(StripeOrder::Rgb));

    let mut batch = GpuBatch::new();
    let mut target =
        DrawTarget::cpu_only(&mut batch).with_policy(Policy::prefer(Prefer::Reference));
    let drawn = f.draw_glyph(&mut target, gid, 48.0, &[], &opts, None);
    let r = drawn.bitmap().expect("no pixels");
    assert_eq!(
        r.bitmap.len(),
        r.metrics.width * r.metrics.height * 3,
        "a subpixel reference bitmap is not three bytes per pixel",
    );
    let differs = r
        .bitmap
        .chunks_exact(3)
        .any(|p| p[0] != p[1] || p[1] != p[2]);
    assert!(differs, "every pixel had three equal channels, so no filtering happened");
}

#[test]
fn a_colour_glyph_goes_where_its_description_needs() {
    let f = font(COLR);
    let device = gpu();
    let opts = RasterOptions::default();

    let mut seen_gpu_color = 0usize;
    let mut seen_scene = 0usize;
    let mut seen_other = 0usize;
    for gid in 0..f.num_glyphs() {
        let mut batch = GpuBatch::new();
        let mut target = DrawTarget::new(&mut batch, &device);
        match f.draw_glyph(&mut target, gid, 48.0, &[], &opts, Some(0)) {
            DrawnGlyph::GpuColor(slots) => {
                assert!(!slots.is_empty(), "a flat-colour glyph produced no slots");
                seen_gpu_color += 1;
            }
            DrawnGlyph::Scene(scene) => {
                assert!(scene.width > 0 && scene.height > 0, "an empty scene");
                assert_eq!(
                    scene.rgba.len(),
                    scene.width * scene.height * 4,
                    "a scene's pixels are not width * height * 4",
                );
                seen_scene += 1;
            }
            _ => seen_other += 1,
        }
    }
    assert!(seen_gpu_color > 0, "no glyph took the instanced colour path");
    assert!(seen_scene > 0, "no glyph took the scene path");
    let _ = seen_other;

    let plain = font(GARAMOND);
    let gid = plain.glyph_id('B' as u32).expect("B");
    let mut batch = GpuBatch::new();
    let mut target = DrawTarget::new(&mut batch, &device);
    let d = plain.draw_glyph(&mut target, gid, 48.0, &[], &opts, Some(0));
    assert_eq!(which(&d), "gpu", "a monochrome glyph asked for in colour did not fall through");
}

#[test]
fn a_colour_glyph_routed_off_the_gpu_stays_colour() {
    let f = font(COLR);
    let opts = RasterOptions::default();
    let flat = (0..f.num_glyphs()).find(|&gid| {
        let mut b = GpuBatch::new();
        let device = gpu();
        let mut t = DrawTarget::new(&mut b, &device);
        match f.draw_glyph(&mut t, gid, 48.0, &[], &opts, Some(0)) {
            DrawnGlyph::GpuColor(slots) => slots.iter().any(|s| s.tint[3] > 0.0),
            _ => false,
        }
    });
    let Some(gid) = flat else { panic!("the fixture has no flat-colour glyph") };

    let mut batch = GpuBatch::new();
    let mut headless = DrawTarget::cpu_only(&mut batch);
    let d = f.draw_glyph(&mut headless, gid, 48.0, &[], &opts, Some(0));
    assert_eq!(which(&d), "scene", "a colour glyph lost its colour when routed off the GPU");
    let DrawnGlyph::Scene(scene) = d else { unreachable!() };
    assert!(scene.rgba.iter().any(|&b| b != 0), "the scene rendered blank");
}

#[test]
fn the_variants_a_caller_must_handle_all_occur() {
    let mono = font(GARAMOND);
    let colr = font(COLR);
    let device = gpu();
    let opts = RasterOptions::default();
    let mut seen: Vec<&'static str> = Vec::new();

    let push = |d: &DrawnGlyph, seen: &mut Vec<&'static str>| {
        let w = which(d);
        if !seen.contains(&w) {
            seen.push(w);
        }
    };

    let mut batch = GpuBatch::new();
    let mut t = DrawTarget::new(&mut batch, &device);
    push(&mono.draw_glyph(&mut t, mono.glyph_id('B' as u32).unwrap(), 48.0, &[], &opts, None), &mut seen);
    push(&mono.draw_glyph(&mut t, mono.glyph_id('B' as u32).unwrap(), 8.0, &[], &opts, None), &mut seen);
    push(&mono.draw_glyph(&mut t, mono.glyph_id(' ' as u32).unwrap(), 12.0, &[], &opts, None), &mut seen);

    let mut batch = GpuBatch::new();
    let mut t = DrawTarget::cpu_only(&mut batch).with_policy(Policy::prefer(Prefer::Reference));
    push(&mono.draw_glyph(&mut t, mono.glyph_id('B' as u32).unwrap(), 32.0, &[], &opts, None), &mut seen);

    let mut batch = GpuBatch::new();
    let mut t = DrawTarget::new(&mut batch, &device)
        .with_policy(Policy::prefer(Prefer::Gpu).strictly());
    let hinted = RasterOptions::default().with_hinting(HintMode::Auto);
    push(&mono.draw_glyph(&mut t, mono.glyph_id('B' as u32).unwrap(), 48.0, &[], &hinted, None), &mut seen);

    for gid in 0..colr.num_glyphs() {
        let mut batch = GpuBatch::new();
        let mut t = DrawTarget::new(&mut batch, &device);
        push(&colr.draw_glyph(&mut t, gid, 48.0, &[], &opts, Some(0)), &mut seen);
    }

    for want in ["cpu", "gpu", "nothing", "reference", "refused", "gpu-color", "scene"] {
        assert!(seen.contains(&want), "the variant {want:?} never occurred: saw {seen:?}");
    }
}
