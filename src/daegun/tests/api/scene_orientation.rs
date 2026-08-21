use daegun::paint::{DisplayList, Op, Paint, Rgba};
use daegun::{FillRule, Font, Path};

fn font() -> Font {
    let path = format!("{}/inter/InterVariable.ttf", crate::FONTS);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_bytes(&bytes).expect("Inter parses")
}

const PX: f32 = 80.0;

// Trimmed to the ink and resampled, so two rasters of slightly different canvas size compare on
// shape alone rather than on where the antialiasing margin fell.
fn profile(w: usize, h: usize, ink: &dyn Fn(usize, usize) -> bool) -> Vec<i32> {
    let rows: Vec<usize> = (0..h).filter(|&y| (0..w).any(|x| ink(x, y))).collect();
    let cols: Vec<usize> = (0..w).filter(|&x| (0..h).any(|y| ink(x, y))).collect();
    assert!(!rows.is_empty() && !cols.is_empty(), "no ink at all");
    let (y0, y1) = (rows[0], rows[rows.len() - 1]);
    let (x0, x1) = (cols[0], cols[cols.len() - 1]);
    let (ih, iw) = (y1 - y0 + 1, x1 - x0 + 1);
    (0..12)
        .map(|k| {
            let y = y0 + k * (ih - 1) / 11;
            ((x0..=x1).filter(|&x| ink(x, y)).count() * 100 / iw) as i32
        })
        .collect()
}

fn through_scene(f: &Font, gid: u16) -> Vec<i32> {
    let mut p = Path::default();
    f.outline_glyph_instanced(gid, &[], &mut p).expect("outline");
    let mut list = DisplayList::default();
    let id = list.push_path(p);
    list.push(Op::Fill {
        path:      id,
        paint:     Paint::Solid(Rgba { r: 255, g: 255, b: 255, a: 255 }),
        rule:      FillRule::NonZero,
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    });
    let s = daegun::paint::render(&list, PX, f32::from(f.upm())).expect("renders");
    profile(s.width, s.height, &|x, y| s.rgba[(y * s.width + x) * 4 + 3] > 128)
}

fn through_rasterizer(f: &Font, gid: u16) -> Vec<i32> {
    let r = f.rasterize_glyph(gid, PX, &[]).expect("rasterizes");
    profile(r.metrics.width, r.metrics.height, &|x, y| {
        r.bitmap[y * r.metrics.width + x] > 128
    })
}

// The two renderers must agree which way up a glyph is.
//
// They did not. `Geometry::finalize` puts a path's ymax at raster row 0, which is the flip into
// raster order the CPU rasterizer wants because it is handed y-up font outlines — but the paint
// stage had already flipped through `to_device`, so every filled path came out mirrored. Nothing
// caught it: the COLR fixtures are circles and gradients, symmetric top to bottom, and a flip is
// invisible on them. `Font::render_colr_glyph` builds exactly this scene, so every color glyph
// daegun drew was upside down.
#[test]
fn a_filled_path_faces_the_same_way_the_rasterizer_draws_it() {
    let f = font();
    // Every one of these is asymmetric top to bottom, which is the whole point: on an `H` or an `O`
    // the defect this guards against cannot be seen.
    for ch in ["T", "L", "E", "7", "J", "p", "g", "y"] {
        let gid = f.glyph_ids(ch)[0].expect("Inter has it");
        let raster = through_rasterizer(&f, gid);
        let scene = through_scene(&f, gid);
        let worst = raster
            .iter()
            .zip(&scene)
            .map(|(a, b)| (a - b).abs())
            .max()
            .expect("twelve samples");
        assert!(
            worst <= 15,
            "{ch} differs by {worst}% between the rasterizer and the paint stage\n  \
             rasterize_glyph {raster:?}\n  display list    {scene:?}",
        );
    }
}

// The cheapest statement of the same thing, in case the profile above is ever relaxed.
//
// A `T` carries almost all its ink in the top fifth and almost none in the bottom fifth. Mirrored,
// that reverses, which no tolerance can absorb.
#[test]
fn a_t_is_top_heavy_through_the_paint_stage() {
    let f = font();
    let gid = f.glyph_ids("T")[0].expect("Inter has T");
    let mut p = Path::default();
    f.outline_glyph_instanced(gid, &[], &mut p).expect("outline");
    let mut list = DisplayList::default();
    let id = list.push_path(p);
    list.push(Op::Fill {
        path:      id,
        paint:     Paint::Solid(Rgba { r: 255, g: 255, b: 255, a: 255 }),
        rule:      FillRule::NonZero,
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    });
    let s = daegun::paint::render(&list, PX, f32::from(f.upm())).expect("renders");

    let ink = |y: usize| (0..s.width).filter(|&x| s.rgba[(y * s.width + x) * 4 + 3] > 128).count();
    let top: usize = (0..s.height / 5).map(ink).sum();
    let bottom: usize = (s.height * 4 / 5..s.height).map(ink).sum();
    assert!(
        top > bottom * 3,
        "T should be top heavy: {top} ink in the top fifth against {bottom} in the bottom",
    );
}

// The two renderers must also agree how round a curve is, not merely which way up it faces.
//
// The paint stage flattens a path that is already in device pixels, so its tolerance has to be
// stated in pixels. It passed `Geometry::new(self.px, self.px)`, an area bound of 6 square pixels,
// where the CPU rasterizer effectively uses about 0.05 — a hundred times tighter. Curves came out
// visibly faceted, worst by 0.63 of full coverage on a single pixel of an `8`.
#[test]
fn a_curve_is_as_round_through_the_paint_stage_as_through_the_rasterizer() {
    let f = font();
    for px in [62.0f32, 200.0] {
        for ch in ["8", "O", "S", "e"] {
            let gid = f.glyph_ids(ch)[0].expect("Inter has it");
            let r = f.rasterize_glyph(gid, px, &[]).expect("rasterizes");

            let mut p = Path::default();
            f.outline_glyph_instanced(gid, &[], &mut p).expect("outline");
            let mut list = DisplayList::default();
            let id = list.push_path(p);
            list.push(Op::Fill {
                path:      id,
                paint:     Paint::Solid(Rgba { r: 255, g: 255, b: 255, a: 255 }),
                rule:      FillRule::NonZero,
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            });
            let s = daegun::paint::render(&list, px, f32::from(f.upm())).expect("renders");

            // Aligned on the ink so the antialiasing margin does not offset the comparison.
            let sa = |x: usize, y: usize| f64::from(s.rgba[(y * s.width + x) * 4 + 3]) / 255.0;
            let ox = (0..s.width).find(|&x| (0..s.height).any(|y| sa(x, y) > 0.02)).expect("ink");
            let oy = (0..s.height).find(|&y| (0..s.width).any(|x| sa(x, y) > 0.02)).expect("ink");

            let mut worst = 0.0f64;
            for y in 0..r.metrics.height.min(s.height - oy) {
                for x in 0..r.metrics.width.min(s.width - ox) {
                    let a = f64::from(r.bitmap[y * r.metrics.width + x]) / 255.0;
                    worst = worst.max((a - sa(x + ox, y + oy)).abs());
                }
            }
            assert!(
                worst < 0.25,
                "{ch} at {px}px differs by {worst:.2} of coverage on a pixel between the \
                 rasterizer and the paint stage, which is a flattening difference, not antialiasing",
            );
        }
    }
}
