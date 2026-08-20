use daegun::daerizer::daegpu::{eval, GlyphSlot, GpuBatch};

type Quad = [[f32; 2]; 3];

fn box_with_flat_top(clockwise: bool) -> Vec<Quad> {
    let (lo, hi, l, r) = (0.10f32, 0.50f32, 0.20f32, 0.80f32);
    let tip = hi + 2.0e-6;
    let cw: Vec<Quad> = vec![
        [[l, hi], [(l + r) * 0.5, hi], [r, tip]],
        [[r, tip], [r, (lo + hi) * 0.5], [r, lo]],
        [[r, lo], [(l + r) * 0.5, lo], [l, lo]],
        [[l, lo], [l, (lo + hi) * 0.5], [l, hi]],
    ];
    if clockwise {
        return cw;
    }
    cw.iter().rev().map(|c| [c[2], c[1], c[0]]).collect()
}

fn degenerate_batch() -> (GpuBatch, GlyphSlot, f32) {
    for clockwise in [true, false] {
        let mut curves = box_with_flat_top(clockwise);
        let mut batch = GpuBatch::new();
        let Some(slot) = batch.append(&mut curves) else { continue };

        let pts = batch.curves();
        let shared = (0..pts.len() / 3).find_map(|i| {
            let (a, b, c) = (pts[3 * i], pts[3 * i + 1], pts[3 * i + 2]);
            (a.y == b.y && b.y != c.y).then_some(a.y)
        });
        if let Some(y) = shared {
            return (batch, slot, y);
        }
    }
    panic!("neither orientation left a curve whose first two control points share a y");
}

#[test]
fn a_curve_with_two_points_on_the_ray_does_not_divide_by_zero() {
    let (batch, slot, y) = degenerate_batch();

    for i in 0..64 {
        let t = f64::from(i) / 63.0;
        let x = slot.box_min[0] + (slot.box_max[0] - slot.box_min[0]) * t as f32;
        let w = eval::winding(&batch, &slot, [x, y], [64.0, 64.0]);
        assert!(w.is_finite(), "winding is {w} at x = {x} on the shared row y = {y}");
        let c = eval::coverage(&batch, &slot, [x, y], [64.0, 64.0]);
        assert!(c.is_finite(), "coverage is {c} at x = {x} on the shared row y = {y}");
        assert!((0.0..=1.0).contains(&c), "coverage {c} out of range at x = {x}");
    }
}

#[test]
fn the_shared_row_path_survives_the_same_curve() {
    use daegun::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};
    let (batch, slot, y) = degenerate_batch();
    let params = daegun::daerizer::daegpu::SubpixelParams::from_layout(&SubpixelLayout::horizontal(
        StripeOrder::Rgb,
    ));

    for i in 0..64 {
        let t = f64::from(i) / 63.0;
        let x = slot.box_min[0] + (slot.box_max[0] - slot.box_min[0]) * t as f32;
        let ch = eval::coverage_channels(&batch, &slot, [x, y], [64.0, 64.0], &params);
        for (name, v) in [("r", ch[0]), ("g", ch[1]), ("b", ch[2])] {
            assert!(v.is_finite(), "channel {name} is {v} at x = {x} on the shared row");
            assert!((0.0..=1.0).contains(&v), "channel {name} is {v}, out of range at x = {x}");
        }
    }
}

#[test]
fn sampling_exactly_on_control_point_rows_stays_finite() {
    let bytes = std::fs::read(format!("{}/inter/InterVariable.ttf", super::fonts_dir())).expect("read");
    let tables = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("tables");
    let head = tables.get("head").expect("head");
    let format = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("loca format");
    let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
    let count = daegun::daecore::daetype::decoder::read_u16_be(tables.get("maxp").expect("maxp"), 4)
        .expect("num glyphs") as usize;
    let loca = daegun::daecore::daetype::instancer::parse_loca(&tables, format, count).expect("loca");

    let mut sampled = 0;
    for gid in 1..300u16.min(count as u16) {
        let mut pen = daegun::daerizer::daegpu::collector(upm);
        if daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&tables, &loca, gid, &mut pen)
            .is_err()
        {
            continue;
        }
        let Ok(mut curves) = pen.finish() else { continue };
        let mut batch = GpuBatch::new();
        let Some(slot) = batch.append(&mut curves) else { continue };

        let ys: Vec<f32> = batch.curves().iter().map(|c| c.y).take(48).collect();
        for y in ys {
            for i in 0..8 {
                let t = f64::from(i) / 7.0;
                let x = slot.box_min[0] + (slot.box_max[0] - slot.box_min[0]) * t as f32;
                let w = eval::winding(&batch, &slot, [x, y], [48.0, 48.0]);
                assert!(w.is_finite(), "gid {gid}: winding {w} at ({x}, {y})");
                sampled += 1;
            }
        }
    }
    assert!(sampled > 10_000, "only {sampled} samples reached the check");
}
