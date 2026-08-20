use super::FONTS;

fn font(rel: &str) -> daegun::Font {
    let bytes = std::fs::read(format!("{FONTS}/{rel}")).expect("read font");
    daegun::Font::from_bytes(&bytes).expect("parse font")
}

#[test]
fn prewarmed_replay_matches_the_streaming_decode() {
    for (name, rel, glyphs) in [
        ("glyf", "eb-garamond/EBGaramond.ttf", 900u16),
        ("CFF", "stix-two-math/STIX2Math.otf", 900),
        ("CFF2 variable", "source-han-sans/SourceHanSansJP-VF.otf", 900),
    ] {
        let plain = font(rel);
        let warmed = font(rel);
        let n = plain.num_glyphs().min(glyphs);
        let added = warmed.prewarm(0..n, &[]);
        assert!(added > 0, "{name}: prewarm cached nothing, so this test proves nothing");

        let mut compared = 0usize;
        for px in [7.0f32, 13.0, 16.0, 31.0, 64.0] {
            for gid in 0..n {
                let a = plain.rasterize_glyph(gid, px, &[]);
                let b = warmed.rasterize_glyph(gid, px, &[]);
                compared += 1;
                match (a, b) {
                    (None, None) => {}
                    (Some(a), Some(b)) => {
                        assert_eq!(a.bitmap, b.bitmap, "{name}: gid {gid} at {px}px replayed to different pixels");
                        assert_eq!(
                            (a.metrics.width, a.metrics.height, a.metrics.xmin, a.metrics.ymin),
                            (b.metrics.width, b.metrics.height, b.metrics.xmin, b.metrics.ymin),
                            "{name}: gid {gid} at {px}px replayed to a different box",
                        );
                    }
                    _ => panic!("{name}: gid {gid} at {px}px rendered in one path and not the other"),
                }
            }
        }
        assert!(compared > 1000, "{name}: only {compared} renders compared");
    }
}

#[test]
fn prewarmed_replay_matches_under_a_transform() {
    let plain = font("eb-garamond/EBGaramond.ttf");
    let warmed = font("eb-garamond/EBGaramond.ttf");
    warmed.prewarm(0..400, &[]);

    let (s, c) = (0.4f32.sin(), 0.4f32.cos());
    for t in [[c, s, -s, c, 0.0, 0.0], [1.0, 0.0, 0.25, 1.0, 0.0, 0.0], [1.5, 0.0, 0.0, 0.75, 0.0, 0.0]] {
        let mut opts = daegun::RasterOptions::default();
        opts.transform = Some(t);
        for gid in 1..400u16 {
            let a = plain.rasterize_glyph_with(gid, 24.0, &[], &opts);
            let b = warmed.rasterize_glyph_with(gid, 24.0, &[], &opts);
            assert_eq!(
                a.map(|r| (r.bitmap, r.metrics.width, r.metrics.height)),
                b.map(|r| (r.bitmap, r.metrics.width, r.metrics.height)),
                "gid {gid} differed under transform {t:?}",
            );
        }
    }
}

#[test]
fn prewarmed_outlines_are_keyed_by_axis_location() {
    let plain = font("inter/InterVariable.ttf");
    let warmed = font("inter/InterVariable.ttf");
    warmed.prewarm(1..400, &[("wght", 100.0)]);

    let mut differed_across_weights = 0usize;
    for gid in 1..400u16 {
        for w in [100.0f64, 900.0] {
            let a = plain.rasterize_glyph(gid, 20.0, &[("wght", w)]);
            let b = warmed.rasterize_glyph(gid, 20.0, &[("wght", w)]);
            assert_eq!(a.map(|r| r.bitmap), b.map(|r| r.bitmap),
                "gid {gid} at wght {w} was served the wrong prewarmed outline");
        }
        let light = plain.rasterize_glyph(gid, 20.0, &[("wght", 100.0)]).map(|r| r.bitmap);
        let bold = plain.rasterize_glyph(gid, 20.0, &[("wght", 900.0)]).map(|r| r.bitmap);
        if light.is_some() && light != bold { differed_across_weights += 1 }
    }
    assert!(differed_across_weights > 100,
        "only {differed_across_weights} glyphs differ between wght 100 and 900, so the keying is untested");
}

#[test]
fn clearing_prewarm_restores_the_decode_path() {
    let f = font("eb-garamond/EBGaramond.ttf");
    let before: Vec<_> = (1..200u16).map(|g| f.rasterize_glyph(g, 18.0, &[]).map(|r| r.bitmap)).collect();

    assert!(f.prewarm(1..200, &[]) > 0);
    f.clear_glyph_cache();
    let warmed: Vec<_> = (1..200u16).map(|g| f.rasterize_glyph(g, 18.0, &[]).map(|r| r.bitmap)).collect();

    f.clear_prewarm();
    f.clear_glyph_cache();
    let after: Vec<_> = (1..200u16).map(|g| f.rasterize_glyph(g, 18.0, &[]).map(|r| r.bitmap)).collect();

    assert_eq!(before, warmed, "prewarming changed the pixels");
    assert_eq!(before, after, "clearing the prewarm changed the pixels");
}

#[test]
fn prewarm_counts_only_what_it_added() {
    let f = font("eb-garamond/EBGaramond.ttf");
    let n = f.num_glyphs();

    let inkable = (0..n)
        .filter(|&gid| {
            let mut p = daegun::daecore::daetype::outline::Path::default();
            f.outline_glyph_instanced(gid, &[], &mut p).is_some() && !p.is_empty()
        })
        .count();

    let first = f.prewarm(0..n, &[]);
    assert_eq!(first, inkable, "prewarm cached {first} outlines where {inkable} are non-empty");
    assert!(first < usize::from(n), "every gid cached, so the empty-outline skip never fired");

    f.clear_prewarm();
    assert!(f.prewarm(0..500, &[]) > 400);
    assert_eq!(f.prewarm(0..500, &[]), 0, "the second prewarm re-added already-cached outlines");

    assert_eq!(f.prewarm(n..n.saturating_add(50), &[]), 0);
}
