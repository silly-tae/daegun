use daegun::daecore::daetype::outline::FillRule;
use daegun::daerizer::daecpu::math::{Glyph, OutlineBounds, Point};
use daegun::daerizer::daecpu::rasterize::Raster;

fn signed_zero_triangle() -> Vec<[Point; 2]> {
    vec![
        [Point::new(0.0, 0.0), Point::new(4.0, -0.0)],
        [Point::new(4.0, -0.0), Point::new(2.0, 4.0)],
        [Point::new(2.0, 4.0), Point::new(0.0, 0.0)],
    ]
}

#[test]
fn the_baseline_is_the_case_the_guard_is_for() {
    let segs = signed_zero_triangle();
    let base = segs[0];
    assert_ne!(
        base[0].y.to_bits(),
        base[1].y.to_bits(),
        "the baseline's y values are bit-identical, so `push` would drop the segment entirely",
    );
    assert_eq!(base[1].y - base[0].y, 0.0, "the baseline has a non-zero dy, so no guard is involved");
    assert_ne!(
        base[0].x.to_bits(),
        base[1].x.to_bits(),
        "the baseline would be classified vertical, and `v_line` never touches the reciprocal",
    );
}

#[test]
fn a_signed_zero_baseline_still_renders() {
    for scale in [1.0f32, 0.5, 0.25, 0.016, 0.0078] {
        let glyph = Glyph {
            v_segments: Vec::new(),
            m_segments: signed_zero_triangle(),
            bounds: OutlineBounds { xmin: 0.0, ymin: 0.0, width: 4.0, height: 4.0 },
            ..Default::default()
        };
        let mut raster = Raster::new(8, 8);
        raster.draw(&glyph, scale, scale, 0.0, 0.0);
        let coverage = raster.into_coverage(FillRule::NonZero);

        let nan = coverage.iter().filter(|v| v.is_nan()).count();
        assert_eq!(
            nan, 0,
            "scale {scale}: {nan} of {} cells are NaN — a NaN delta poisons every later sample on \
             its row, and `as u8` turns those into zero, so the glyph renders empty",
            coverage.len(),
        );
        assert!(
            coverage.iter().all(|v| v.is_finite()),
            "scale {scale}: a coverage cell is infinite",
        );

        if scale * 4.0 >= 1.0 {
            let inked = coverage.iter().filter(|v| **v > 0.001).count();
            assert!(inked > 0, "scale {scale}: no cell took ink, so the triangle vanished");
        }
    }
}

#[test]
fn the_order_of_a_signed_zero_baseline_does_not_matter() {
    let mut segs = signed_zero_triangle();
    segs[0] = [segs[0][1], segs[0][0]];
    let glyph = Glyph {
        v_segments: Vec::new(),
        m_segments: segs,
        bounds: OutlineBounds { xmin: 0.0, ymin: 0.0, width: 4.0, height: 4.0 },
            ..Default::default()
    };
    let mut raster = Raster::new(8, 8);
    raster.draw(&glyph, 1.0, 1.0, 0.0, 0.0);
    let coverage = raster.into_coverage(FillRule::NonZero);
    assert!(coverage.iter().all(|v| v.is_finite()), "a reversed baseline produced a non-finite cell");
    assert!(coverage.iter().any(|v| *v > 0.001), "a reversed baseline drew nothing");
}

#[test]
fn a_signed_zero_on_x_alone_is_still_a_vertical_segment() {
    let segs = vec![
        [Point::new(0.0, 0.0), Point::new(-0.0, 4.0)],
        [Point::new(-0.0, 4.0), Point::new(3.0, 4.0)],
        [Point::new(3.0, 4.0), Point::new(0.0, 0.0)],
    ];
    assert_ne!(segs[0][0].x.to_bits(), segs[0][1].x.to_bits());
    assert_eq!(segs[0][1].x - segs[0][0].x, 0.0);

    let glyph = Glyph {
        v_segments: Vec::new(),
        m_segments: segs,
        bounds: OutlineBounds { xmin: 0.0, ymin: 0.0, width: 3.0, height: 4.0 },
            ..Default::default()
    };
    let mut raster = Raster::new(8, 8);
    raster.draw(&glyph, 1.0, 1.0, 0.0, 0.0);
    let coverage = raster.into_coverage(FillRule::NonZero);
    assert!(coverage.iter().all(|v| v.is_finite()), "a zero-dx segment produced a non-finite cell");
    assert!(coverage.iter().any(|v| *v > 0.001), "a zero-dx triangle drew nothing");
}
