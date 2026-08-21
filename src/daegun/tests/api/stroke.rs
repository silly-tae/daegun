use daegun::{Cap, Font, Join, RasterOptions, StrokeStyle};

fn font(rel: &str) -> Font {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_bytes(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn stroking_outlines_the_glyph_instead_of_filling_it() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('O' as u32).expect("Inter maps O");

    let filled = f.rasterize_glyph(gid, 64.0, &[]).expect("O fills");
    let style = StrokeStyle { width: 200.0, join: Join::Round, cap: Cap::Butt };
    let stroked = f
        .rasterize_glyph_with(gid, 64.0, &[], &RasterOptions::default().with_stroke(style))
        .expect("O strokes");

    assert_ne!(filled.bitmap, stroked.bitmap, "stroking produced the filled bitmap");
    assert!(
        stroked.metrics.width > filled.metrics.width,
        "a stroke must reach outside the fill ({} vs {})",
        stroked.metrics.width, filled.metrics.width,
    );
    let mid = |g: &daegun::RasterizedGlyph| {
        g.bitmap[(g.metrics.height / 2) * g.metrics.width + g.metrics.width / 2]
    };
    let thin = StrokeStyle { width: 60.0, join: Join::Round, cap: Cap::Butt };
    let outline = f
        .rasterize_glyph_with(gid, 64.0, &[], &RasterOptions::default().with_stroke(thin))
        .expect("O strokes");
    assert!(mid(&outline) < 128, "the center of a thin outlined O should be paper, got {}", mid(&outline));
}

#[test]
fn the_glyph_cache_distinguishes_stroke_settings() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('H' as u32).expect("Inter maps H");
    let at = |o: &RasterOptions| f.rasterize_glyph_with(gid, 48.0, &[], o).expect("H rasterizes").bitmap;

    let fill = at(&RasterOptions::default());
    let thin = at(&RasterOptions::default().with_stroke(StrokeStyle { width: 240.0, join: Join::Bevel, cap: Cap::Butt }));
    let thick = at(&RasterOptions::default().with_stroke(StrokeStyle { width: 600.0, join: Join::Bevel, cap: Cap::Butt }));
    let round = at(&RasterOptions::default().with_stroke(StrokeStyle { width: 240.0, join: Join::Round, cap: Cap::Butt }));

    assert_ne!(fill, thin, "a stroke request returned the cached fill");
    assert_ne!(thin, thick, "two widths returned the same bitmap");
    assert_ne!(thin, round, "two joins returned the same bitmap");
    assert_eq!(thin, at(&RasterOptions::default().with_stroke(StrokeStyle { width: 240.0, join: Join::Bevel, cap: Cap::Butt })));
}
