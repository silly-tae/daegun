use daegun::{Font, HintMode, RasterOptions};

fn font(rel: &str) -> Font {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_bytes(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn raster(f: &Font, gid: u16, px: f32, mode: HintMode) -> Option<(Vec<u8>, usize, usize)> {
    let opts = RasterOptions::default().with_hinting(mode);
    let g = f.rasterize_glyph_with(gid, px, &[], &opts)?;
    let (w, h) = (g.metrics.width, g.metrics.height);
    Some((g.bitmap, w, h))
}

#[test]
fn auto_changes_an_unhinted_truetype_font_where_bytecode_cannot() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('H' as u32).expect("Inter maps H");

    let plain = raster(&f, gid, 13.0, HintMode::None).expect("unhinted H rasterizes");
    let bytecode = raster(&f, gid, 13.0, HintMode::Subpixel).expect("H rasterizes");
    let auto = raster(&f, gid, 13.0, HintMode::Auto).expect("H rasterizes");

    assert_eq!(
        plain, bytecode,
        "InterVariable ships no hinting at all, so Subpixel must be identical to None — \
         if this fails the fixture changed and the rest of this test proves nothing",
    );
    assert_ne!(
        plain, auto,
        "HintMode::Auto produced byte-identical output to unhinted on a font with no bytecode, \
         so the autohinter did not run",
    );
}

#[test]
fn auto_reaches_a_cff_font() {
    let f = font("source-serif/SourceSerif4Variable-Roman.otf");
    let gid = f.glyph_id('H' as u32).expect("Source Serif maps H");

    let plain = raster(&f, gid, 13.0, HintMode::None).expect("unhinted H rasterizes");
    let auto = raster(&f, gid, 13.0, HintMode::Auto).expect("H rasterizes");
    assert_ne!(
        plain, auto,
        "a CFF font reached HintMode::Auto and came back unchanged; \
         CFF has no glyf, so this is the path that only the autohinter can serve",
    );
}

#[test]
fn auto_force_overrides_the_fonts_own_bytecode() {
    let f = font("test-fixtures/hinted.ttf");
    let gid = 1;

    let bytecode = raster(&f, gid, 16.0, HintMode::Subpixel);
    let auto = raster(&f, gid, 16.0, HintMode::Auto);
    let forced = raster(&f, gid, 16.0, HintMode::AutoForce);

    if let (Some(b), Some(a)) = (&bytecode, &auto) {
        assert_eq!(b, a, "Auto must defer to a font that ships real bytecode");
    }
    if let (Some(b), Some(fo)) = (&bytecode, &forced) {
        assert_ne!(b, fo, "AutoForce must ignore the font's own bytecode");
    }
}

#[test]
fn hinting_shifts_the_box_by_at_most_a_couple_of_pixels_at_any_size() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('o' as u32).expect("Inter maps o");

    for px in [12.0, 64.0, 500.0, 2000.0] {
        let plain = raster(&f, gid, px, HintMode::None).expect("unhinted o rasterizes");
        let auto = raster(&f, gid, px, HintMode::Auto).expect("o rasterizes");
        let dw = plain.1.abs_diff(auto.1);
        let dh = plain.2.abs_diff(auto.2);
        assert!(
            dw <= 2 && dh <= 2,
            "at {px}ppem hinting changed the box by {dw}x{dh} pixels              ({}x{} -> {}x{}); a grid snap cannot move an edge more than half a pixel",
            plain.1, plain.2, auto.1, auto.2,
        );
    }
}

#[test]
fn a_non_latin_font_declines_rather_than_mis_hinting() {
    let f = font("colr-v1-test-glyphs/test_glyphs.ttf");
    let gid = 5u16;
    let plain = raster(&f, gid, 13.0, HintMode::None);
    let auto = raster(&f, gid, 13.0, HintMode::AutoForce);
    assert_eq!(
        plain, auto,
        "a font with no Latin coverage was hinted anyway; the writing-system test is not holding",
    );
}

#[test]
fn a_sweep_of_glyphs_all_still_rasterize_under_auto() {
    let f = font("inter/InterVariable.ttf");
    let mut hinted_differs = 0usize;
    let mut checked = 0usize;
    for gid in (1..400u16).step_by(7) {
        let Some(plain) = raster(&f, gid, 12.0, HintMode::None) else { continue };
        let auto = raster(&f, gid, 12.0, HintMode::Auto)
            .unwrap_or_else(|| panic!("gid {gid} rasterizes unhinted but not under Auto"));
        checked += 1;
        if plain != auto {
            hinted_differs += 1;
        }
    }
    assert!(checked > 20, "swept only {checked} glyphs, too few to mean anything");
    assert!(
        hinted_differs * 4 >= checked,
        "only {hinted_differs} of {checked} glyphs changed under Auto; \
         the hinter is running but barely touching anything",
    );
}

#[test]
fn capitals_share_one_baseline_and_one_cap_height() {
    let f = font("inter/InterVariable.ttf");
    let opts = RasterOptions::default().with_hinting(HintMode::Auto);

    let mut boxes = Vec::new();
    for c in "HEZLOCUST".chars() {
        let gid = f.glyph_id(c as u32).unwrap_or_else(|| panic!("Inter maps {c}"));
        let g = f
            .rasterize_glyph_with(gid, 13.0, &[], &opts)
            .unwrap_or_else(|| panic!("{c} rasterizes"));
        boxes.push((c, g.metrics.ymin, g.metrics.ymin + g.metrics.height as i32));
    }

    let (_, bottom, top) = boxes[0];
    for &(c, b, t) in &boxes {
        assert_eq!(b, bottom, "{c} sits on a different baseline row than H ({b} vs {bottom})");
        assert_eq!(t, top, "{c} reaches a different cap height than H ({t} vs {top})");
    }
}

#[test]
fn the_crossbar_of_h_survives_small_sizes() {
    let f = font("inter/InterVariable.ttf");
    let gid = f.glyph_id('H' as u32).expect("Inter maps H");
    let opts = RasterOptions::default().with_hinting(HintMode::Auto);

    for px in [8.0, 10.0, 12.0, 14.0] {
        let g = f.rasterize_glyph_with(gid, px, &[], &opts).expect("H rasterizes");
        let (w, h) = (g.metrics.width, g.metrics.height);
        assert!(w >= 3 && h >= 3, "H at {px}px is {w}x{h}, too small to inspect");

        let ends = |r: usize| g.bitmap[r * w] as u32 + g.bitmap[r * w + w - 1] as u32;
        let middle = |r: usize| g.bitmap[r * w + w / 2] as u32;
        let bar = (1..h - 1).any(|r| middle(r) > 100 && ends(r) > 100);
        assert!(
            bar,
            "H at {px}px has no row inked across its middle — the crossbar was lost, \
             which is the failure stem fitting exists to prevent",
        );
    }
}

#[test]
fn cff_declared_hints_are_used_and_differ_from_the_autohinter() {
    let f = font("stix-two-math/STIX2Math.otf");
    let mut differ = 0usize;
    let mut checked = 0usize;

    for c in "HEZLOCUSTABDFGMNPR".chars() {
        let gid = f.glyph_id(c as u32).unwrap_or_else(|| panic!("Source Serif maps {c}"));
        let plain = raster(&f, gid, 13.0, HintMode::None).expect("rasterizes");
        let declared = raster(&f, gid, 13.0, HintMode::Auto).expect("rasterizes");
        let forced = raster(&f, gid, 13.0, HintMode::AutoForce).expect("rasterizes");
        checked += 1;
        assert_ne!(plain, declared, "{c}: Auto matched unhinted, so no hinting ran at all");
        if declared != forced {
            differ += 1;
        }
    }

    assert!(checked >= 15, "only {checked} glyphs checked");
    assert!(
        differ * 2 >= checked,
        "only {differ} of {checked} glyphs differ between the font's declared hints and the \
         autohinter; the CFF hint path is probably not running",
    );
}

#[test]
fn serifs_survive_on_both_outline_formats() {
    for (label, rel) in [
        ("STIX2Math (CFF)", "stix-two-math/STIX2Math.otf"),
        ("EBGaramond (TrueType)", "eb-garamond/EBGaramond.ttf"),
    ] {
        let f = font(rel);
        let gid = f.glyph_id('H' as u32).unwrap_or_else(|| panic!("{label} maps H"));
        let opts = RasterOptions::default().with_hinting(HintMode::AutoForce);
        let g = f.rasterize_glyph_with(gid, 13.0, &[], &opts).expect("H rasterizes");
        let (w, h) = (g.metrics.width, g.metrics.height);
        assert!(w >= 5 && h >= 5, "{label}: {w}x{h} is too small to inspect");

        let inked = |r: usize| (0..w).filter(|&c| g.bitmap[r * w + c] > 60).count();
        let stems = inked(h / 2 + 1);
        let top = inked(0).max(inked(1));
        let bottom = inked(h - 1).max(inked(h - 2));
        assert!(
            top > stems,
            "{label}: the top serif is not wider than the stems ({top} vs {stems}) — flattened away",
        );
        assert!(
            bottom > stems,
            "{label}: the bottom serif is not wider than the stems ({bottom} vs {stems}) — flattened away",
        );
    }
}
