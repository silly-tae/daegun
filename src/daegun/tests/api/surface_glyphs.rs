use daegun::Font;

fn bytes_of(rel: &str) -> Vec<u8> {
    let path = format!("{}/{}", crate::FONTS, rel);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"))
}

fn font(rel: &str) -> Font {
    Font::from_bytes(&bytes_of(rel)).unwrap_or_else(|e| panic!("{rel} did not parse: {e}"))
}

fn colr_table() -> Vec<u8> {
    let map = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes_of(COLR)).expect("parses");
    map.get("COLR").expect("the COLR fixture carries a COLR table").to_owned_vec()
}

const GARAMOND: &str = "eb-garamond/EBGaramond.ttf";
const CARETS: &str = "test-fixtures/carets.ttf";
const COLR: &str = "colr-v1-test-glyphs/test_glyphs.ttf";

#[test]
fn ligature_carets_come_back_for_a_font_that_declares_them() {
    let f = font(CARETS);
    let three = 4u16;
    let two = 5u16;

    let c3 = f.ligature_carets(three, &[]);
    let c2 = f.ligature_carets(two, &[]);
    assert!(!c3.is_empty(), "the three-part ligature declares carets but none came back");
    assert!(!c2.is_empty(), "the two-part ligature declares carets but none came back");
    assert!(
        c3.len() > c2.len(),
        "a three-part ligature should carry more carets than a two-part one: {} against {}",
        c3.len(),
        c2.len(),
    );
    assert!(c3.iter().all(|v| v.is_finite()), "a caret position is not finite: {c3:?}");
    assert!(
        c3.windows(2).all(|w| w[0] <= w[1]),
        "carets are not in ascending order, so a caller cannot index by division: {c3:?}",
    );

    assert!(f.ligature_carets(1, &[]).is_empty(), "a non-ligature glyph reported carets");
}

#[test]
fn caret_positions_span_the_text_they_are_asked_about() {
    let f = font(CARETS);
    let carets = f.caret_positions("ffi", &[], false).expect("the fixture shapes 'ffi'");
    assert!(
        carets.len() >= "ffi".chars().count(),
        "expected at least one caret per character, got {} for 3 characters",
        carets.len(),
    );
    assert!(carets.iter().all(|v| v.is_finite()), "a caret position is not finite");
    assert!(
        carets.windows(2).all(|w| w[0] <= w[1]),
        "caret positions are not monotonic across the run: {carets:?}",
    );
}

#[test]
fn a_variation_selector_resolves_or_declines() {
    let f = font(GARAMOND);
    if let Some(g) = f.variation_glyph_id('a' as u32, 0xFE00) {
        assert!(g < f.num_glyphs(), "variation_glyph_id returned out-of-range gid {g}");
    }
    assert!(
        f.variation_glyph_id(0x000F_FFFD, 0xFE00).is_none(),
        "a variation sequence resolved for an unmapped base codepoint",
    );
}

#[test]
fn a_color_glyph_renders_to_a_scene() {
    let f = font(COLR);
    let colr = colr_table();

    let off = daegun::daecore::daetype::decoder::read_u32_be(&colr, 14).expect("baseGlyphList offset") as usize;
    let gid = daegun::daecore::daetype::decoder::read_u16_be(&colr, off + 4).expect("first base glyph");

    let scene = f.render_colr_glyph(gid, 64.0, &[], 0).expect("a declared base glyph renders");
    assert!(scene.width > 0 && scene.height > 0, "the rendered scene has an empty box");
    assert_eq!(
        scene.rgba.len(), scene.width * scene.height * 4,
        "rgba is not width * height * 4 bytes",
    );
    assert_eq!(scene.skipped_ops, 0, "the executor skipped ops on a plain COLR v1 glyph");
    assert!(scene.rgba.iter().any(|&b| b != 0), "the scene rendered entirely blank");

    assert!(
        f.render_colr_glyph(0, 64.0, &[], 0).is_none(),
        ".notdef rendered as a color glyph",
    );
}

#[test]
fn gpu_upload_places_a_glyph_and_reuses_its_slot() {
    let f = font(GARAMOND);
    let gid = f.glyph_id('B' as u32).expect("B");
    let mut batch = daegun::daerizer::daegpu::GpuBatch::new();

    let first = f.gpu_glyph(&mut batch, gid, &[]).expect("B uploads");
    let again = f.gpu_glyph(&mut batch, gid, &[]).expect("B uploads again");
    assert_eq!(
        first.band_base, again.band_base,
        "the same glyph in the same batch was uploaded twice instead of reusing its slot",
    );

    let other = f.glyph_id('C' as u32).expect("C");
    let second = f.gpu_glyph(&mut batch, other, &[]).expect("C uploads");
    assert_ne!(first.band_base, second.band_base, "two different glyphs landed in the same slot");

    let space = f.glyph_id(' ' as u32).expect("space");
    assert!(f.gpu_glyph(&mut batch, space, &[]).is_err(), "the space glyph uploaded curves");
}

#[test]
fn a_short_strip_drops_glyph_coverage() {
    fn inside(a: [f32; 2], b: [f32; 2], c: [f32; 2], p: [f32; 2]) -> bool {
        let side = |u: [f32; 2], v: [f32; 2]| {
            (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])
        };
        let (x, y, z) = (side(a, b), side(b, c), side(c, a));
        (x >= 0.0 && y >= 0.0 && z >= 0.0) || (x <= 0.0 && y <= 0.0 && z <= 0.0)
    }
    fn covered(verts: &[daegun::HullVertex], n: usize, p: [f32; 2]) -> bool {
        (0..n.saturating_sub(2)).any(|i| inside(verts[i].pos, verts[i + 1].pos, verts[i + 2].pos, p))
    }

    let f = font(GARAMOND);
    let gid = f.glyph_id('x' as u32).expect("x");
    let mut batch = daegun::daerizer::daegpu::GpuBatch::new();
    f.gpu_glyph(&mut batch, gid, &[]).expect("x uploads");

    let hull = batch.hulls();
    assert_eq!(
        hull.len(), daegun::HULL_VERTICES,
        "one glyph did not produce HULL_VERTICES hull entries, which is what GpuBatch::hulls promises",
    );
    let mut distinct: Vec<[u32; 2]> =
        hull.iter().map(|v| [v.pos[0].to_bits(), v.pos[1].to_bits()]).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), daegun::HULL_VERTICES, "x fell back to the box, so this proves nothing");

    let points: Vec<[f32; 2]> = batch.curves().iter().map(|c| [c.x, c.y]).collect();
    let full = points.iter().filter(|p| covered(hull, daegun::HULL_VERTICES, **p)).count();
    let short = points.iter().filter(|p| covered(hull, daegun::HULL_VERTICES - 1, **p)).count();
    assert!(
        full > short,
        "a strip one vertex short covered as much as the full one ({full} vs {short}), so the count \
         the doc gives a caller stopped mattering",
    );
    assert!(
        full - short > points.len() / 5,
        "only {} of {} control points needed the last vertex; the measured figure was {}",
        full - short, points.len(), 128,
    );
}

#[test]
fn gpu_color_upload_refuses_what_a_tinted_outline_cannot_express() {
    let f = font(COLR);
    let colr = colr_table();
    let off = daegun::daecore::daetype::decoder::read_u32_be(&colr, 14).expect("baseGlyphList offset") as usize;
    let n = daegun::daecore::daetype::decoder::read_u32_be(&colr, off).expect("count") as usize;

    let mut flat = 0;
    let mut refused = 0;
    for i in 0..n {
        let gid = daegun::daecore::daetype::decoder::read_u16_be(&colr, off + 4 + i * 6).expect("base glyph");
        let mut batch = daegun::daerizer::daegpu::GpuBatch::new();
        match f.gpu_color_glyph(&mut batch, gid, &[], 0) {
            Ok(slots) => {
                assert!(!slots.is_empty(), "gid {gid} succeeded with no slots");
                assert!(
                    slots.iter().all(|s| s.tint.iter().all(|c| (0.0..=1.0).contains(c))),
                    "gid {gid} produced a tint outside 0..=1",
                );
                flat += 1;
            }
            Err(_) => refused += 1,
        }
    }
    assert_eq!(flat, 3, "the fixture has exactly three flat-color base glyphs");
    assert!(refused > 0, "no glyph was refused, so NotFlatColor is never reached on this fixture");
}
