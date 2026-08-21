use daegun::daerizer::daegpu::eval;
use daegun::{Font, GpuBatch};

fn font(rel: &str) -> Option<Font> {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).ok()?;
    Font::from_bytes(&bytes).ok()
}

// Coverage from both rasterizers at the same points: `(worst pixel, mean over the glyph)`.
fn agreement(f: &Font, gid: u16, px: f32) -> Option<(f64, f64)> {
    let r = f.rasterize_glyph(gid, px, &[])?;
    if r.metrics.width == 0 || r.metrics.height == 0 {
        return None;
    }
    let mut batch = GpuBatch::new();
    let slot = f.gpu_glyph(&mut batch, gid, &[]).ok()?;

    let (mut worst, mut sum, mut n) = (0.0f64, 0.0f64, 0usize);
    for y in 0..r.metrics.height {
        for x in 0..r.metrics.width {
            // The CPU bitmap's own pixel centers, expressed in em units, so both are asked about
            // exactly the same points. `metrics.ymin` is the bottom in y-up, and the bitmap's rows
            // run top down, hence the flip on y.
            let dx = f64::from(r.metrics.xmin) + x as f64 + 0.5;
            let dy = f64::from(r.metrics.ymin) + (r.metrics.height - 1 - y) as f64 + 0.5;
            let em = [(dx / f64::from(px)) as f32, (dy / f64::from(px)) as f32];

            let gpu = f64::from(eval::coverage(&batch, &slot, em, [px, px]));
            let cpu = f64::from(r.bitmap[y * r.metrics.width + x]) / 255.0;
            let d = (cpu - gpu).abs();
            worst = worst.max(d);
            sum += d;
            n += 1;
        }
    }
    Some((worst, sum / n as f64))
}

// daegun rasterizes the same outline two entirely different ways, and until now nothing checked
// that they agree.
//
// `daecpu` accumulates signed area per scanline and reads out exact coverage. `daegpu` casts two
// rays per sample and combines whichever was in a position to know. They are graded separately —
// the shaders against `eval`, `eval` against itself — so a shared misreading of a glyph's geometry
// would pass both suites. This is the only thing that compares their output.
//
// The mean is the signal. Two different algorithms disagree most on the pixels an edge cuts
// through, so a single worst pixel is noisy, while the mean over a whole glyph is stable and tight:
// about 1% at text sizes and a tenth of that at display sizes.
//
// The worst pixel also carries one known disagreement, and the CPU is the side that is right about
// it. `daecpu` resolves overlapping contours before it accumulates, so a pixel that two contours
// both partly cover is measured once. `daegpu` sums per-curve contributions and saturates, so it
// still counts that pixel twice — the same defect `daecpu` had until the contours were unioned.
// Source Serif at 120px is where it shows most, at 0.60 on a single pixel.
#[test]
fn the_cpu_and_gpu_rasterizers_agree_on_the_same_outline() {
    // Two TrueType faces and one CFF, plus a script whose glyphs are nothing like Latin.
    let faces = [
        ("Inter", "inter/InterVariable.ttf"),
        ("EB Garamond", "eb-garamond/EBGaramond.ttf"),
        ("Source Serif", "source-serif/SourceSerif4Variable-Roman.otf"),
        ("Devanagari", "noto-devanagari/NotoSansDevanagari.ttf"),
    ];

    let mut compared = 0usize;
    for (name, rel) in faces {
        let Some(f) = font(rel) else { continue };
        for px in [16.0f32, 48.0, 120.0] {
            let (mut worst, mut mean, mut count) = (0.0f64, 0.0f64, 0usize);
            for gid in 1..f.num_glyphs().min(80) {
                if let Some((w, m)) = agreement(&f, gid, px) {
                    worst = worst.max(w);
                    mean += m;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            compared += count;
            let mean = mean / count as f64;

            assert!(
                mean < 0.035,
                "{name} at {px}px: the two rasterizers disagree by {mean:.4} of coverage on \
                 average over {count} glyphs, which is a systematic difference rather than edge \
                 sampling",
            );
            assert!(
                worst < 0.7,
                "{name} at {px}px: {worst:.3} of coverage on a single pixel between the two \
                 rasterizers, which is past what overlapping contours account for",
            );
        }
    }
    assert!(compared > 500, "only {compared} glyphs compared; the fixtures did not load");
}
