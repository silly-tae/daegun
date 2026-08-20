use daegun::daecore::daetype::TableBytes;
use std::time::Instant;

#[test]
fn a_malformed_cff_is_parsed_once() {
    let path = format!("{}/stix-two-math/STIX2Math.otf", crate::FONTS);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    let mut map = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("parses");
    let cff = map.get("CFF ").expect("STIX2Math carries a CFF table").clone();

    let t = Instant::now();
    core::hint::black_box(daegun::daecore::daetype::outline::CffOutlines::parse(&cff).ok());
    let one_parse = t.elapsed();

    map.insert("CFF ".to_string(), TableBytes::from_vec(cff[..cff.len() / 3].to_vec()));
    let cache = daegun::daecore::cache::FontCache::new(map);
    assert!(cache.cff_outlines().is_none(), "the truncated CFF was expected to decline");

    const CALLS: u32 = 200;
    let t = Instant::now();
    for _ in 0..CALLS {
        core::hint::black_box(cache.cff_outlines());
    }
    let repeated = t.elapsed();

    assert!(
        repeated < one_parse,
        "{CALLS} declining calls took {repeated:?}, which is more than the {one_parse:?} a single \
         parse costs – the decline is being recomputed rather than remembered",
    );
}
