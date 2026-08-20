use daegun::daerizer::daegpu::{eval, GpuBatch};

#[test]
fn winding_is_a_magnitude_and_never_negative() {
    for (path, gids) in [("eb-garamond/EBGaramond.ttf", 1..160u16), ("inter/InterVariable.ttf", 1..160)] {
        let face = super::face::Face::load(path);
        let mut batch = GpuBatch::new();
        let mut sampled = 0usize;
        let mut glyphs = 0usize;
        let mut peak = 0.0f32;

        for gid in gids {
            let Some(slot) = face.glyph(&mut batch, gid) else { continue };
            if slot.box_max[0] <= slot.box_min[0] || slot.box_max[1] <= slot.box_min[1] {
                continue;
            }
            glyphs += 1;

            let pad = 0.1;
            for i in 0..24 {
                for j in 0..24 {
                    let fx = (i as f32 + 0.5) / 24.0 * (1.0 + 2.0 * pad) - pad;
                    let fy = (j as f32 + 0.5) / 24.0 * (1.0 + 2.0 * pad) - pad;
                    let x = slot.box_min[0] + (slot.box_max[0] - slot.box_min[0]) * fx;
                    let y = slot.box_min[1] + (slot.box_max[1] - slot.box_min[1]) * fy;

                    let w = eval::winding(&batch, &slot, [x, y], [32.0, 32.0]);
                    assert!(
                        w >= 0.0,
                        "{path} gid {gid}: winding is {w} at ({x}, {y}), and winding is documented \
                         as a magnitude — a negative one clamps to zero and the glyph vanishes",
                    );
                    assert!(w.is_finite(), "{path} gid {gid}: winding is {w} at ({x}, {y})");
                    peak = peak.max(w);
                    sampled += 1;
                }
            }
        }

        assert!(glyphs >= 50, "{path}: only {glyphs} glyphs had outlines, so this proved nothing");
        assert!(sampled > 25_000, "{path}: only {sampled} samples");

        assert!(peak > 1.0, "{path}: winding never exceeded 1 ({peak}), so it is being clamped \
                             somewhere it should not be and `degenerate.rs` is watching a lie");
    }
}

#[test]
fn coverage_is_winding_with_a_ceiling() {
    let face = super::face::Face::load("eb-garamond/EBGaramond.ttf");
    let mut batch = GpuBatch::new();
    let mut checked = 0usize;

    for gid in 1..120u16 {
        let Some(slot) = face.glyph(&mut batch, gid) else { continue };
        if slot.box_max[0] <= slot.box_min[0] || slot.box_max[1] <= slot.box_min[1] {
            continue;
        }
        for i in 0..20 {
            for j in 0..20 {
                let x = slot.box_min[0]
                    + (slot.box_max[0] - slot.box_min[0]) * (i as f32 + 0.5) / 20.0;
                let y = slot.box_min[1]
                    + (slot.box_max[1] - slot.box_min[1]) * (j as f32 + 0.5) / 20.0;
                let w = eval::winding(&batch, &slot, [x, y], [32.0, 32.0]);
                let c = eval::coverage(&batch, &slot, [x, y], [32.0, 32.0]);
                assert_eq!(
                    c.to_bits(), w.min(1.0).to_bits(),
                    "gid {gid} at ({x}, {y}): coverage {c} is not winding {w} clamped",
                );
                assert!((0.0..=1.0).contains(&c), "gid {gid}: coverage {c} left 0..=1");
                checked += 1;
            }
        }
    }
    assert!(checked > 20_000, "only {checked} samples");
}
