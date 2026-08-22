use daegun::Font;

fn font() -> Font {
    let path = format!("{}/inter/InterVariable.ttf", crate::FONTS);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_vec(bytes).expect("parses")
}

fn fill_glyphs(f: &Font, sizes: u16) {
    for px in 0..sizes {
        for gid in 0..120u16 {
            std::hint::black_box(f.rasterize_glyph(gid, 16.0 + f32::from(px) * 6.0, &[]));
        }
    }
}

fn fill_shapes(f: &Font, n: usize) {
    for i in 0..n {
        std::hint::black_box(f.shape(&format!("a budget worth measuring {i}"), &[], false));
    }
}

#[test]
fn a_tightened_glyph_budget_evicts_down_to_it() {
    let f = font();
    fill_glyphs(&f, 12);
    assert!(f.glyph_cache_stats().1 > 512 * 1024, "the cache never filled");

    f.set_glyph_cache_bytes(256 * 1024);
    let (_, bytes) = f.glyph_cache_stats();
    assert!(bytes <= 256 * 1024, "held {bytes} B against a 256 KB budget");
}

#[test]
fn the_shape_budget_bounds_the_shape_cache() {
    let f = font();
    f.set_shape_cache_bytes(64 * 1024);
    fill_shapes(&f, 4000);
    let (_, bytes) = f.shape_cache_stats();
    assert!(bytes <= 64 * 1024, "shape cache held {bytes} B against a 64 KB budget");
}

#[test]
fn the_outline_budget_bounds_the_outline_cache() {
    let f = font();
    f.set_outline_cache_bytes(32 * 1024);
    fill_glyphs(&f, 4);
    let (_, bytes) = f.outline_cache_stats();
    assert!(bytes <= 32 * 1024, "outline cache held {bytes} B against a 32 KB budget");
}

#[test]
fn the_instance_budget_bounds_instanced_fonts() {
    let f = font();
    f.set_instance_cache_bytes(2 * 1024 * 1024);
    for w in 0..40 {
        let axes: &[(&str, f64)] = &[("wght", 200.0 + f64::from(w) * 10.0)];
        std::hint::black_box(f.shape("weight", axes, false));
        std::hint::black_box(f.rasterize_glyph(40, 24.0, axes));
    }
    let (locations, tables) = f.instance_cache_stats();
    let held = locations + tables;
    assert!(held <= 2 * 1024 * 1024, "instance caches held {held} B against a 2 MB budget");
}

// The allowance is spent down rather than capped, so what matters is that setting it grants more.
#[test]
fn the_cmap_allowance_is_readable_and_settable() {
    let f = font();
    f.set_cmap_index_allowance(1234);
    assert_eq!(f.cmap_index_allowance(), 1234);
}

// A budget of zero has to turn caching off without changing a single pixel.
#[test]
fn caching_nothing_renders_the_same() {
    let f = font();
    let gid = f.glyph_id('g' as u32).expect("font has g");
    let warm = f.rasterize_glyph(gid, 28.0, &[]).expect("rasterizes");

    f.set_glyph_cache_bytes(0);
    f.set_outline_cache_bytes(0);
    f.set_shape_cache_bytes(0);
    let cold = f.rasterize_glyph(gid, 28.0, &[]).expect("rasterizes");

    assert_eq!(f.glyph_cache_stats().0, 0, "a zero budget still cached something");
    assert_eq!(warm.bitmap, cold.bitmap, "output changed when caching was turned off");
    assert_eq!(warm.metrics.width, cold.metrics.width);
    assert_eq!(warm.metrics.height, cold.metrics.height);
}
