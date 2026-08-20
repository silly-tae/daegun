use daegun::{Font, Metrics};

fn font(rel: &str) -> Option<Font> {
    let bytes = std::fs::read(format!("{}/{}", crate::FONTS, rel)).ok()?;
    Font::from_bytes(&bytes).ok()
}

// Coverage from a 16x render, box filtered back down, as the thing to be right about.
//
// Both bitmaps are placed by their own metrics: `xmin`/`ymin` are the ink box in device pixels with
// the glyph origin at zero and y up, while rows run top down. Everything here goes through that.
fn reference(f: &Font, gid: u16, px: f32, m: &Metrics) -> Option<Vec<f32>> {
    const K: i32 = 16;
    let big = f.rasterize_glyph(gid, px * K as f32, &[])?;
    let (bw, bh) = (big.metrics.width as i32, big.metrics.height as i32);
    let (w, h) = (m.width as i32, m.height as i32);
    let mut out = vec![0.0f32; (w * h) as usize];

    for y in 0..h {
        for x in 0..w {
            let x0 = (m.xmin + x) * K - big.metrics.xmin;
            let yup0 = (m.ymin + (h - 1 - y)) * K - big.metrics.ymin;
            let mut sum = 0.0f32;
            for sy in 0..K {
                let by = bh - 1 - (yup0 + sy);
                if by < 0 || by >= bh {
                    continue;
                }
                for sx in 0..K {
                    let bx = x0 + sx;
                    if bx >= 0 && bx < bw {
                        sum += f32::from(big.bitmap[(by * bw + bx) as usize]) / 255.0;
                    }
                }
            }
            out[(y * w + x) as usize] = sum / (K * K) as f32;
        }
    }
    Some(out)
}

// How much more ink the glyph has than it should, at its worst pixel.
fn over_coverage(f: &Font, ch: &str, px: f32) -> f32 {
    let gid = f.glyph_ids(ch)[0].expect("the font has it");
    let r = f.rasterize_glyph(gid, px, &[]).expect("rasterizes");
    let reference = reference(f, gid, px, &r.metrics).expect("reference rasterizes");
    (0..r.metrics.width * r.metrics.height)
        .map(|i| f32::from(r.bitmap[i]) / 255.0 - reference[i])
        .fold(0.0f32, f32::max)
}

/// A glyph whose contours overlap must not be drawn with more ink than it has.
///
/// The coverage accumulator holds the integral of winding over each pixel, so where two contours
/// both partly cover one pixel it adds their shares and clamps. Interiors survive that; antialiased
/// edges do not. Inter's "4" is a diagonal and a stem that share a flat top at y = 1490 and overlap
/// in x from 822 to 887, and its top row read `45 188 255 255 188 188 143` — a single straight edge
/// cannot be 255 in one column and 188 in the next. `A` is the same story where its crossbar meets
/// the diagonals.
///
/// `Geometry::finalize` now unions the contours first, which leaves one boundary and one count.
#[test]
fn overlapping_contours_are_not_drawn_twice() {
    let Some(f) = font("inter/InterVariable.ttf") else { return };
    for ch in ["4", "A"] {
        for px in [17.0f32, 34.0, 61.0] {
            let over = over_coverage(&f, ch, px);
            assert!(
                over < 0.1,
                "{ch} at {px}px carries {over:.2} more coverage than it should on a pixel, which is \
                 an overlap counted twice rather than edge sampling",
            );
        }
    }
}

/// The controls, which say the resolver is not simply running everywhere and flattening detail out.
///
/// `l`, `I` and `H` are single contours. `o` and `8` are nested ones, wound opposite so the counter
/// is a hole — an arrangement a winding accumulator already gets right, and one the resolver must
/// leave alone. They read the same before and after the fix, which is the point.
#[test]
fn glyphs_without_overlap_are_untouched() {
    let Some(f) = font("inter/InterVariable.ttf") else { return };
    for ch in ["l", "I", "H", "o", "8"] {
        let over = over_coverage(&f, ch, 34.0);
        assert!(over < 0.1, "{ch} moved by {over:.2}, and nothing should have touched it");
    }
}

/// Across four faces and three scripts, rather than the two glyphs the defect was found on.
///
/// Before the contours were unioned this stood at 45 of 398 glyphs for Inter and 310 of 398 for
/// Source Serif. What is left is glyphs the resolver declined — its guard withholds a union it
/// cannot verify, and the fallback is the old rendering.
#[test]
fn few_glyphs_anywhere_carry_extra_ink() {
    let faces = [
        ("Inter", "inter/InterVariable.ttf", 4usize),
        ("EB Garamond", "eb-garamond/EBGaramond.ttf", 8),
        ("Source Serif", "source-serif/SourceSerif4Variable-Roman.otf", 12),
        ("Devanagari", "noto-devanagari/NotoSansDevanagari.ttf", 20),
    ];
    let mut checked = 0usize;
    for (name, rel, budget) in faces {
        let Some(f) = font(rel) else { continue };
        let mut over = 0usize;
        let mut count = 0usize;
        for gid in 1..f.num_glyphs().min(400) {
            let Some(r) = f.rasterize_glyph(gid, 34.0, &[]) else { continue };
            if r.metrics.width == 0 || r.metrics.height == 0 {
                continue;
            }
            let Some(reference) = reference(&f, gid, 34.0, &r.metrics) else { continue };
            let worst = (0..r.metrics.width * r.metrics.height)
                .map(|i| f32::from(r.bitmap[i]) / 255.0 - reference[i])
                .fold(0.0f32, f32::max);
            count += 1;
            if worst > 0.15 {
                over += 1;
            }
        }
        checked += count;
        assert!(
            over <= budget,
            "{name}: {over} of {count} glyphs carry more than 0.15 of extra coverage, over a budget \
             of {budget}",
        );
    }
    assert!(checked > 1000, "only {checked} glyphs checked; the fixtures did not load");
}
