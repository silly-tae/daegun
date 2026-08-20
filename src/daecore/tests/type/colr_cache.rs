use daegun::daecore::cache::FontCache;

const LOCATIONS: [&[f64]; 4] = [&[], &[0.5, -0.25, 1.0], &[1.0, 1.0, 1.0], &[0.5, 0.5, 0.5]];

#[test]
fn cached_paint_matches_uncached_across_locations() {
    let rel = "colr-v1-test-glyphs/test_glyphs_variable.ttf";
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    let map = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("parses");
    let colr = map.get("COLR").expect("COLR present").clone();
    let var = daegun::daecore::daetype::colr_v1::parse_colr_v1_var_data(&colr);
    let n = daegun::daecore::daetype::decoder::read_u16_be(map.get("maxp").expect("maxp"), 4).expect("n");
    let cache = FontCache::new(map.clone());

    let order = [0, 1, 2, 3, 1, 0, 3, 2, 3, 3, 0, 1];
    let mut compared = 0usize;
    let mut resolved = 0usize;
    for &i in &order {
        let loc = LOCATIONS[i];
        for gid in 0..n {
            let want = daegun::daecore::daetype::colr_v1::colr_v1_paint_graph_cached(&map, gid, loc, 0, &var);
            let got = cache.colr_v1_paint(gid, loc, 0);
            assert_eq!(
                format!("{want:?}"),
                format!("{got:?}"),
                "gid {gid} at {loc:?} differs between the cached and uncached paths",
            );
            compared += 1;
            if want.is_some() {
                resolved += 1;
            }
        }
    }
    assert!(compared > 0, "compared nothing");
    assert!(resolved > 0, "no glyph resolved to a paint graph, so nothing was really compared");
}
