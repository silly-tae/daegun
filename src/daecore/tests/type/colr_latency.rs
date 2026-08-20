use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use daegun::daecore::daetype::TableBytes;

fn table_map_of(rel: &str) -> BTreeMap<String, TableBytes> {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes)
        .unwrap_or_else(|e| panic!("{path} did not parse: {e}"))
}

fn report(name: &str, detail: &str, samples: &mut [Duration]) {
    samples.sort();
    let n = samples.len();
    let sum: Duration = samples.iter().sum();
    eprintln!("{name}");
    eprintln!("  {detail}");
    eprintln!(
        "  n {n}  min {:?}  mean {:?}  median {:?}  p95 {:?}  max {:?}",
        samples[0], sum / n as u32, samples[n / 2], samples[(n as f64 * 0.95) as usize], samples[n - 1],
    );
}

fn base_glyphs(colr: &[u8]) -> Vec<u16> {
    let off = daegun::daecore::daetype::decoder::read_u32_be(colr, 14).unwrap_or(0) as usize;
    if off == 0 { return Vec::new(); }
    let n = daegun::daecore::daetype::decoder::read_u32_be(colr, off).unwrap_or(0) as usize;
    (0..n)
        .filter_map(|i| daegun::daecore::daetype::decoder::read_u16_be(colr, off + 4 + i * 6))
        .collect()
}

fn bench(name: &str, rel: &str, location: &[f64]) {
    let map = table_map_of(rel);
    let colr = map.get("COLR").expect("COLR present").clone();
    let gids = base_glyphs(&colr);
    assert!(!gids.is_empty(), "{name}: font declares no v1 base glyphs");
    let var_data = daegun::daecore::daetype::colr_v1::parse_colr_v1_var_data(&colr);

    let mut expect = 0usize;
    for &g in &gids {
        if daegun::daecore::daetype::colr_v1::colr_v1_paint_graph_cached(&map, g, location, 0, &var_data).is_some() {
            expect += 1;
        }
    }
    assert!(expect > 0, "{name}: no glyph resolved to a paint graph");

    let iters = (400_000 / gids.len().max(1)).max(20);
    let mut samples = Vec::with_capacity(iters);
    let mut proof = 0usize;
    for _ in 0..iters {
        let t = Instant::now();
        let mut hit = 0usize;
        for &g in &gids {
            let p = daegun::daecore::daetype::colr_v1::colr_v1_paint_graph_cached(&map, g, location, 0, &var_data);
            if p.is_some() { hit += 1; }
            core::hint::black_box(&p);
        }
        samples.push(t.elapsed());
        proof += hit;
    }
    assert_eq!(proof, iters * expect, "{name}: a run resolved a different number of graphs");
    report(name, &format!("{} base glyphs, whole sweep per sample", gids.len()), &mut samples);
}

#[test]
#[ignore]
fn colr_v1_paint_graph_static() {
    bench("colr_v1_paint_graph_static", "colr-v1-test-glyphs/test_glyphs.ttf", &[]);
}

#[test]
#[ignore]
fn colr_v1_paint_graph_variable() {
    bench(
        "colr_v1_paint_graph_variable",
        "colr-v1-test-glyphs/test_glyphs_variable.ttf",
        &[0.5, 0.5, 0.5],
    );
}

#[test]
#[ignore]
fn cache_colr_variable() {
    let map = table_map_of("colr-v1-test-glyphs/test_glyphs_variable.ttf");
    let colr = map.get("COLR").expect("COLR present").clone();
    let gids = base_glyphs(&colr);
    let cache = daegun::daecore::cache::FontCache::new(map);
    let location = [0.5, 0.5, 0.5];

    let mut expect = 0usize;
    for &g in &gids {
        if cache.colr_v1_paint(g, &location, 0).is_some() { expect += 1; }
    }
    assert!(expect > 0, "no glyph resolved to a paint graph");

    let iters = (400_000 / gids.len().max(1)).max(20);
    let mut samples = Vec::with_capacity(iters);
    let mut proof = 0usize;
    for _ in 0..iters {
        let t = Instant::now();
        let mut hit = 0usize;
        for &g in &gids {
            let p = cache.colr_v1_paint(g, &location, 0);
            if p.is_some() { hit += 1; }
            core::hint::black_box(&p);
        }
        samples.push(t.elapsed());
        proof += hit;
    }
    assert_eq!(proof, iters * expect, "a run resolved a different number of graphs");
    report("cache_colr_variable", &format!("{} base glyphs through FontCache", gids.len()), &mut samples);
}
