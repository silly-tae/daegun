use daegun::{Font, GpuBatch};

fn font(rel: &str) -> Font {
    let path = format!("{}/{rel}", crate::FONTS);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_vec(bytes).expect("parses")
}

fn bungee() -> Font {
    font("bungee-tint/BungeeTint-Regular.ttf")
}

fn d_of(f: &Font) -> u16 {
    f.shape("D", &[], false).expect("shapes").glyphs[0]
}

fn colored(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4).filter(|p| p[3] > 0 && (p[0] != p[1] || p[1] != p[2])).count()
}

#[test]
fn a_colr_v0_glyph_renders_to_a_color_scene() {
    let f = bungee();
    let gid = d_of(&f);

    assert!(f.colr_v1_paint(gid, &[], 0).is_none(), "the fixture has to be v0, or this proves nothing");
    assert_eq!(f.colr_layers(gid).expect("a v0 color glyph").len(), 2, "a base and a tint layer");

    let scene = f.render_colr_glyph(gid, 48.0, &[], 0).expect("a v0 glyph renders");
    assert!(scene.width > 0 && scene.height > 0, "empty scene");
    let hues = colored(&scene.rgba);
    assert!(hues > 100, "only {hues} pixels carry a hue; the palette never reached the fills");
}

#[test]
fn the_gpu_path_takes_colr_v0_layers_too() {
    let f = bungee();
    let gid = d_of(&f);
    let layers = f.colr_layers(gid).expect("layers");

    let mut batch = GpuBatch::new();
    let slots = f.gpu_color_glyph(&mut batch, gid, &[], 0).expect("a v0 glyph reaches the GPU path");
    assert_eq!(slots.len(), layers.len(), "one slot per layer");

    for (slot, &(_, r, g, b, a, _)) in slots.iter().zip(&layers) {
        let want = [r, g, b, a].map(|c| f32::from(c) / 255.0);
        assert_eq!(slot.tint, want, "a layer's tint did not come from the palette");
    }
}

#[test]
fn a_palette_changes_the_tints_and_not_the_geometry() {
    let f = bungee();
    let gid = d_of(&f);
    let mut batch = GpuBatch::new();

    let first = f.gpu_color_glyph(&mut batch, gid, &[], 0).expect("palette 0");
    let mut differed = false;
    for palette in 1..f.palette_count() {
        let other = f.gpu_color_glyph(&mut batch, gid, &[], palette).expect("another palette");
        assert_eq!(other.len(), first.len(), "palette {palette} changed the layer count");
        for (a, b) in first.iter().zip(&other) {
            assert_eq!(a.slot, b.slot, "palette {palette} re-uploaded the same outline");
            differed |= a.tint != b.tint;
        }
    }
    assert!(differed, "every palette gave identical tints, so nothing was read from CPAL");
}

// The only two foreground solids in the fixture: layers that defer to the caller's text color.
#[test]
fn a_foreground_layer_is_visible() {
    let f = font("colr-v1-test-glyphs/test_glyphs.ttf");
    for gid in [154u16, 155] {
        assert!(f.colr_v1_paint(gid, &[], 0).is_some(), "gid {gid} is no longer a color glyph");
        let scene = f.render_colr_glyph(gid, 64.0, &[], 0).expect("renders");
        let inked = scene.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(inked > 0, "gid {gid} rendered a {}x{} scene with no ink at all", scene.width, scene.height);
    }
}

#[test]
fn a_glyph_with_no_color_description_is_still_refused() {
    let f = font("inter/InterVariable.ttf");
    let gid = d_of(&f);
    assert!(f.colr_layers(gid).is_none(), "Inter has no COLR table");
    assert!(f.render_colr_glyph(gid, 48.0, &[], 0).is_none(), "a plain glyph must not render as color");

    let mut batch = GpuBatch::new();
    assert!(f.gpu_color_glyph(&mut batch, gid, &[], 0).is_err(), "draw_glyph relies on this refusing");
}

// The most covered pixel, by color alone. A COLR layer carries its own alpha and multiplies the
// color by it, so the inked pixels of a half-transparent layer are never fully opaque.
fn strongest_rgb(rgba: &[u8]) -> Option<[u8; 3]> {
    rgba.chunks_exact(4).filter(|p| p[3] > 0).max_by_key(|p| p[3]).map(|p| [p[0], p[1], p[2]])
}

// A layer that defers to the caller's text color has to take the color the caller names, not the
// opaque black the no-argument entry points fall back to.
#[test]
fn a_caller_can_choose_the_foreground_color() {
    let f = font("colr-v1-test-glyphs/test_glyphs.ttf");
    let red = daegun::paint::Rgba { r: 255, g: 0, b: 0, a: 255 };

    for gid in [154u16, 155] {
        let fallback = f.render_colr_glyph(gid, 64.0, &[], 0).expect("renders");
        let chosen = f.render_colr_glyph_with(gid, 64.0, &[], 0, red).expect("renders");

        assert_eq!(
            (fallback.width, fallback.height),
            (chosen.width, chosen.height),
            "gid {gid}: the foreground color changed the geometry",
        );
        assert_eq!(strongest_rgb(&fallback.rgba), Some([0, 0, 0]), "gid {gid}: default is not black");
        assert_eq!(strongest_rgb(&chosen.rgba), Some([255, 0, 0]), "gid {gid}: the color was ignored");
    }
}

#[test]
fn the_gpu_path_takes_the_foreground_color_too() {
    let f = font("colr-v1-test-glyphs/test_glyphs.ttf");
    let red = daegun::paint::Rgba { r: 255, g: 0, b: 0, a: 255 };
    let mut batch = GpuBatch::new();

    let mut moved = 0;
    for gid in 0..f.num_glyphs() {
        let (Ok(fallback), Ok(chosen)) = (
            f.gpu_color_glyph(&mut batch, gid, &[], 0),
            f.gpu_color_glyph_with(&mut batch, gid, &[], 0, red),
        ) else {
            continue;
        };
        assert_eq!(fallback.len(), chosen.len(), "gid {gid}: the layer count changed");
        for (a, b) in fallback.iter().zip(&chosen) {
            assert_eq!(a.slot, b.slot, "gid {gid}: the geometry was re-uploaded");
            if a.tint != b.tint {
                assert_eq!(b.tint[..3], [1.0, 0.0, 0.0], "gid {gid}: a layer took some other color");
                assert_eq!(b.tint[3], a.tint[3], "gid {gid}: the layer's own alpha was not kept");
                moved += 1;
            }
        }
    }
    assert!(moved > 0, "no layer in the fixture deferred to the text color, so nothing was proved");
}

// The color has to reach the router as well, or it is available from most entry points and not all.
#[test]
fn the_router_carries_the_foreground_color() {
    let f = font("colr-v1-test-glyphs/test_glyphs.ttf");
    let red = daegun::paint::Rgba { r: 255, g: 0, b: 0, a: 255 };
    let opts = daegun::RasterOptions::default();
    let mut batch = GpuBatch::new();
    let mut target = daegun::DrawTarget::cpu_only(&mut batch);

    let drawn = f.draw_glyph_with(&mut target, 154, 64.0, &[], &opts, Some(0), red);
    let daegun::DrawnGlyph::Scene(scene) = drawn else {
        panic!("a color glyph did not come back as a scene: {drawn:?}");
    };
    assert_eq!(strongest_rgb(&scene.rgba), Some([255, 0, 0]), "the router dropped the color");
}
