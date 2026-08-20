use daegun::daecore::daetype::outline::FillRule;
use daegun::daerizer::daecpu::math::{Glyph, OutlineBounds, Point};
use daegun::daerizer::daecpu::rasterize::Raster;

#[test]
fn an_unrepresentable_box_is_empty_rather_than_fatal() {
    for (w, h) in [(usize::MAX, 2), (usize::MAX / 2, 4), (usize::MAX, usize::MAX), (1 << 40, 1 << 40)] {
        let raster = Raster::new(w, h);
        let bitmap = raster.get_bitmap();
        assert!(
            bitmap.len() <= 3,
            "Raster::new({w}, {h}) reported {} bytes, so it kept a size it never allocated",
            bitmap.len(),
        );
    }
}

#[test]
fn resetting_to_an_unrepresentable_box_is_empty_rather_than_fatal() {
    let mut raster = Raster::new(8, 8);
    raster.reset(usize::MAX / 2, 4);
    assert!(raster.get_bitmap().len() <= 3, "reset kept a size it never allocated");

    raster.reset(4, 4);
    let glyph = Glyph {
        v_segments: Vec::new(),
        m_segments: vec![
            [Point::new(0.0, 0.0), Point::new(4.0, 0.5)],
            [Point::new(4.0, 0.5), Point::new(2.0, 4.0)],
            [Point::new(2.0, 4.0), Point::new(0.0, 0.0)],
        ],
        bounds: OutlineBounds { xmin: 0.0, ymin: 0.0, width: 4.0, height: 4.0 },
            ..Default::default()
    };
    raster.draw(&glyph, 1.0, 1.0, 0.0, 0.0);
    let coverage = raster.into_coverage(FillRule::NonZero);
    assert_eq!(coverage.len(), 16, "a reset back to a real box did not give a real box");
    assert!(coverage.iter().any(|v| *v > 0.001), "the raster stopped drawing after a refused reset");
}

#[test]
fn a_zero_side_draws_nothing_and_reads_back_empty() {
    for (w, h) in [(0usize, 8usize), (8, 0), (0, 0)] {
        let mut raster = Raster::new(w, h);
        let glyph = Glyph {
            v_segments: Vec::new(),
            m_segments: vec![[Point::new(0.0, 0.0), Point::new(4.0, 4.0)]],
            bounds: OutlineBounds { xmin: 0.0, ymin: 0.0, width: 4.0, height: 4.0 },
            ..Default::default()
        };
        raster.draw(&glyph, 1.0, 1.0, 0.0, 0.0);
        assert!(raster.get_bitmap().is_empty(), "a {w}x{h} box produced pixels");
        assert!(raster.into_coverage(FillRule::NonZero).is_empty(), "a {w}x{h} box produced coverage");
    }
}
