use daegun::{Font, RasterOptions};

fn font(rel: &str) -> Font {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_bytes(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn embolden_adds_weight() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('H' as u32).expect("Inter maps H");

    let plain = f.rasterize_glyph(gid, 48.0, &[]).expect("H rasterizes");
    let bold = f
        .rasterize_glyph_with(gid, 48.0, &[], &RasterOptions::default().with_embolden(120.0))
        .expect("H rasterizes");

    let ink = |g: &daegun::RasterizedGlyph| g.bitmap.iter().map(|&b| b as u64).sum::<u64>();
    assert!(ink(&bold) > ink(&plain), "bold has no more ink ({} vs {})", ink(&bold), ink(&plain));
    assert!(
        bold.metrics.width > plain.metrics.width,
        "bold is no wider ({} vs {})", bold.metrics.width, plain.metrics.width,
    );
    let grew = bold.metrics.width as f32 - plain.metrics.width as f32;
    let expect = 120.0 * 48.0 / f.upm() as f32;
    assert!(
        (grew - expect).abs() <= 2.0,
        "grew {grew}px, expected about {expect}px for 120 units at 48px",
    );
}

#[test]
fn embolden_widens_the_advance_too() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('n' as u32).expect("Inter maps n");
    let px = 48.0;

    let plain = f.rasterize_glyph(gid, px, &[]).expect("n rasterizes");
    let bold = f
        .rasterize_glyph_with(gid, px, &[], &RasterOptions::default().with_embolden(120.0))
        .expect("n rasterizes");

    let delta = bold.metrics.advance_width - plain.metrics.advance_width;
    let expect = 120.0 * px / f.upm() as f32;
    assert!(
        (delta - expect).abs() < 0.5,
        "the advance grew by {delta}px, expected {expect}px — bolded glyphs would overlap",
    );
}

#[test]
fn oblique_leans_the_glyph() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('I' as u32).expect("Inter maps I");

    let upright = f.rasterize_glyph(gid, 64.0, &[]).expect("I rasterizes");
    let slanted = f
        .rasterize_glyph_with(gid, 64.0, &[], &RasterOptions::default().with_oblique(0.25))
        .expect("I rasterizes");

    assert_ne!(upright.bitmap, slanted.bitmap, "oblique changed nothing");
    assert!(
        slanted.metrics.width > upright.metrics.width,
        "a slanted I must be wider ({} vs {})", slanted.metrics.width, upright.metrics.width,
    );
    assert_eq!(
        slanted.metrics.height, upright.metrics.height,
        "a shear along x must not change the height",
    );
    let (w, h) = (slanted.metrics.width, slanted.metrics.height);
    let rightmost = |row: usize| (0..w).rev().find(|&c| slanted.bitmap[row * w + c] > 64);
    let (top, bottom) = (rightmost(1), rightmost(h - 2));
    assert!(
        top > bottom,
        "the top should lean right of the bottom ({top:?} vs {bottom:?})",
    );
}

#[test]
fn oblique_composes_with_a_transform() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('I' as u32).expect("Inter maps I");
    let at = |o: RasterOptions| f.rasterize_glyph_with(gid, 48.0, &[], &o).expect("rasterizes");

    let half = [0.5f32, 0.0, 0.0, 0.5, 0.0, 0.0];
    let scaled = at(RasterOptions::default().with_transform(half));
    let both = at(RasterOptions::default().with_transform(half).with_oblique(0.25));

    assert_ne!(scaled.bitmap, both.bitmap, "the shear was dropped when a transform was present");
    assert!(both.metrics.width > scaled.metrics.width, "the sheared one must be wider");
    let plain = at(RasterOptions::default().with_oblique(0.25));
    assert!(both.metrics.height < plain.metrics.height, "the half-scale transform was dropped");
}

#[test]
fn the_cache_distinguishes_synthetic_settings_and_bad_input_is_refused() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('H' as u32).expect("Inter maps H");
    let at = |o: RasterOptions| f.rasterize_glyph_with(gid, 48.0, &[], &o).map(|g| g.bitmap);

    let plain = at(RasterOptions::default()).expect("rasterizes");
    let bold = at(RasterOptions::default().with_embolden(120.0)).expect("rasterizes");
    let bolder = at(RasterOptions::default().with_embolden(300.0)).expect("rasterizes");
    let slant = at(RasterOptions::default().with_oblique(0.25)).expect("rasterizes");

    assert_ne!(plain, bold, "a bold request returned the cached plain glyph");
    assert_ne!(bold, bolder, "two weights returned the same bitmap");
    assert_ne!(plain, slant, "a slant request returned the cached upright glyph");
    assert_eq!(bold, at(RasterOptions::default().with_embolden(120.0)).unwrap(), "repeat must be stable");

    assert_eq!(at(RasterOptions::default().with_embolden(0.0)), Some(plain.clone()));
    assert_eq!(at(RasterOptions::default().with_embolden(-50.0)), Some(plain));
    assert!(at(RasterOptions::default().with_oblique(f32::NAN)).is_none());
}
