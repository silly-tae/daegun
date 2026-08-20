use daegun::daerizer::daegpu::{GpuBatch, HULL_VERTICES};
use super::fonts_dir;

fn each_glyph(file: &str, mut check: impl FnMut(u16, Vec<[f32; 2]>, Vec<[f32; 2]>)) {
    let bytes = std::fs::read(format!("{}/{file}", fonts_dir())).expect("read font");
    let tables = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("tables");
    let head = tables.get("head").expect("head");
    let format = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("loca format");
    let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
    let count = daegun::daecore::daetype::decoder::read_u16_be(tables.get("maxp").expect("maxp"), 4)
        .expect("num glyphs") as usize;
    let loca = daegun::daecore::daetype::instancer::parse_loca(&tables, format, count).expect("loca");

    let mut seen = 0;
    for gid in 0..count.min(400) as u16 {
        let mut pen = daegun::daerizer::daegpu::collector(upm);
        if daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&tables, &loca, gid, &mut pen)
            .is_err()
        {
            continue;
        }
        let Ok(mut curves) = pen.finish() else { continue };
        let mut batch = GpuBatch::new();
        if batch.append(&mut curves).is_none() {
            continue;
        }
        let pts: Vec<[f32; 2]> = batch.curves().iter().map(|c| [c.x, c.y]).collect();
        let h = batch.hulls();
        assert_eq!(h.len(), HULL_VERTICES, "gid {gid} emitted the wrong vertex count");

        let mut cyclic: Vec<[f32; 2]> = vec![h[0].pos, h[1].pos];
        cyclic.extend((3..HULL_VERTICES).step_by(2).map(|i| h[i].pos));
        let mut back: Vec<[f32; 2]> = (2..HULL_VERTICES).step_by(2).map(|i| h[i].pos).collect();
        back.reverse();
        cyclic.extend(back);
        check(gid, pts, cyclic);
        seen += 1;
    }
    assert!(seen > 50, "only {seen} glyphs reached the check in {file}");
}

fn containment(file: &str) {
    each_glyph(file, |gid, pts, poly| {
        let n = poly.len();
        let live: Vec<[f32; 2]> = {
            let mut v: Vec<[f32; 2]> = Vec::with_capacity(n);
            for p in &poly {
                if v.last().is_none_or(|q: &[f32; 2]| q != p) {
                    v.push(*p);
                }
            }
            while v.len() > 1 && v.first() == v.last() {
                v.pop();
            }
            v
        };
        assert!(live.len() >= 3, "gid {gid} degenerated to {} vertices", live.len());
        let m = live.len();

        let mut area = 0.0f32;
        for i in 0..m {
            let (a, b, c) = (live[i], live[(i + 1) % m], live[(i + 2) % m]);
            area += a[0] * b[1] - b[0] * a[1];
            let turn = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            assert!(turn >= -1e-6, "gid {gid} is not convex: turn {turn} at vertex {i}");
        }
        assert!(area > 0.0, "gid {gid} is wound backwards, area {}", area * 0.5);

        for p in &pts {
            for i in 0..m {
                let (a, b) = (live[i], live[(i + 1) % m]);
                let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
                let len = (ex * ex + ey * ey).sqrt();
                if len < 1e-9 {
                    continue;
                }
                let outside = ((p[0] - a[0]) * ey - (p[1] - a[1]) * ex) / len;
                assert!(
                    outside <= 1e-3,
                    "gid {gid} leaks a control point {outside} em outside edge {i}"
                );
            }
        }
    });
}

#[test]
fn the_drawn_polygon_contains_every_control_point() {
    containment("eb-garamond/EBGaramond.ttf");
    containment("inter/InterVariable.ttf");
}

#[test]
fn the_box_fallback_is_a_rectangle_that_covers_the_glyph() {
    let mut curves = vec![
        [[0.0, 0.0], [0.5, 0.0], [1.0, 0.0]],
        [[1.0, 0.0], [1.0, 0.5], [1.0, 1.0]],
        [[1.0, 1.0], [0.5, 1.0], [0.0, 1.0]],
        [[0.0, 1.0], [0.0, 0.5], [0.0, 0.0]],
    ];
    let mut batch = GpuBatch::new();
    batch.append(&mut curves).expect("a unit square is drawable");
    let h = batch.hulls();
    assert_eq!(h.len(), HULL_VERTICES);
    for v in h {
        assert!(v.pos[0] == 0.0 || v.pos[0] == 1.0, "corner off the square: {:?}", v.pos);
        assert!(v.pos[1] == 0.0 || v.pos[1] == 1.0, "corner off the square: {:?}", v.pos);
        let [xx, xy, yx, yy] = v.dilate;
        assert!(xy.abs() < 1e-6 && yx.abs() < 1e-6, "a square dilates on-axis, got {:?}", v.dilate);
        assert!(
            (xx.abs() - 1.0).abs() < 1e-6 && (yy.abs() - 1.0).abs() < 1e-6,
            "a square corner travels one pad per axis, got {:?}",
            v.dilate
        );
        assert_eq!(xx < 0.0, v.pos[0] == 0.0, "corner {:?} dilates inward in x", v.pos);
        assert_eq!(yy < 0.0, v.pos[1] == 0.0, "corner {:?} dilates inward in y", v.pos);
    }
}

#[test]
fn each_glyph_in_a_shared_batch_indexes_its_own_hull() {
    let bytes = std::fs::read(format!("{}/eb-garamond/EBGaramond.ttf", fonts_dir())).expect("font");
    let tables = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("tables");
    let head = tables.get("head").expect("head");
    let format = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("loca format");
    let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
    let count = daegun::daecore::daetype::decoder::read_u16_be(tables.get("maxp").expect("maxp"), 4)
        .expect("num glyphs") as usize;
    let loca = daegun::daecore::daetype::instancer::parse_loca(&tables, format, count).expect("loca");

    let curves_of = |gid: u16| {
        let mut pen = daegun::daerizer::daegpu::collector(upm);
        daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&tables, &loca, gid, &mut pen)
            .ok()?;
        pen.finish().ok()
    };

    let mut shared = GpuBatch::new();
    let mut expected: Vec<(u16, Vec<daegun::daerizer::daegpu::HullVertex>)> = Vec::new();
    let mut slots = Vec::new();

    for gid in 1..120u16 {
        let Some(mut curves) = curves_of(gid) else { continue };
        let mut alone = GpuBatch::new();
        let mut solo_curves = curves.clone();
        let Some(solo) = alone.append(&mut solo_curves) else { continue };
        let Some(slot) = shared.append(&mut curves) else { continue };

        assert_eq!(solo.hull_base, 0, "gid {gid}: the first glyph of a batch must start at zero");
        expected.push((gid, alone.hulls().to_vec()));
        slots.push((gid, slot));
    }
    assert!(slots.len() > 60, "only {} glyphs entered the shared batch", slots.len());

    assert_eq!(
        shared.hulls().len(), slots.len() * HULL_VERTICES,
        "the shared batch holds {} vertices for {} glyphs", shared.hulls().len(), slots.len(),
    );

    for (i, ((gid, slot), (egid, want))) in slots.iter().zip(expected.iter()).enumerate() {
        assert_eq!(gid, egid, "the two runs disagreed about glyph order");
        assert_eq!(
            slot.hull_base as usize, i * HULL_VERTICES,
            "gid {gid} is glyph {i} of the batch and claims hull_base {}", slot.hull_base,
        );

        let base = slot.hull_base as usize;
        let got = &shared.hulls()[base..base + HULL_VERTICES];
        for (v, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(
                (g.pos[0].to_bits(), g.pos[1].to_bits()),
                (w.pos[0].to_bits(), w.pos[1].to_bits()),
                "gid {gid} vertex {v}: shared batch has {:?} at hull_base {base}, alone it is {:?}",
                g.pos, w.pos,
            );
            assert_eq!(g.dilate, w.dilate, "gid {gid} vertex {v}: dilation differs in a shared batch");
        }
    }
}
