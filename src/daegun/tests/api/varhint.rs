use daegun::{Font, HintMode, RasterOptions};

fn inter() -> Vec<u8> {
    let path = format!("{}/inter/InterVariable.ttf", crate::FONTS);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"))
}

#[test]
fn variable_axes_reach_the_hinted_shape() {
    let bytes = inter();
    let font = Font::from_bytes(&bytes).expect("parses");
    let gid = font.glyph_id('H' as u32).expect("H");

    for hinting in [HintMode::None, HintMode::Auto, HintMode::AutoForce] {
        let opts = RasterOptions::default().with_hinting(hinting);
        let thin = font.rasterize_glyph_with(gid, 48.0, &[("wght", 100.0)], &opts).expect("thin");
        let bold = font.rasterize_glyph_with(gid, 48.0, &[("wght", 900.0)], &opts).expect("bold");
        assert_ne!(
            (thin.metrics.width, &thin.bitmap),
            (bold.metrics.width, &bold.bitmap),
            "{hinting:?}: wght 100 and wght 900 rasterized identically, so the axes never reached the outline",
        );
    }
}

#[test]
fn repeated_axis_values_are_stable() {
    let bytes = inter();
    let font = Font::from_bytes(&bytes).expect("parses");
    let gid = font.glyph_id('H' as u32).expect("H");
    let opts = RasterOptions::default().with_hinting(HintMode::AutoForce);

    let a = font.rasterize_glyph_with(gid, 48.0, &[("wght", 300.0)], &opts).expect("a");
    let _ = font.rasterize_glyph_with(gid, 48.0, &[("wght", 800.0)], &opts).expect("interleaved");
    let b = font.rasterize_glyph_with(gid, 48.0, &[("wght", 300.0)], &opts).expect("b");
    assert_eq!(a.bitmap, b.bitmap, "the same axes gave different pixels either side of another location");
}
