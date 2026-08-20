use std::time::{Duration, Instant};

use daegun::daecore::cache::FontCache;
use daegun::daecore::daeshaper::{buffer::Buffer, face::Face, plan::ShapePlan, shape};

fn cache_of(rel: &str) -> FontCache {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    let map = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes)
        .unwrap_or_else(|e| panic!("{path} did not parse: {e}"));
    FontCache::new(map)
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

fn bench(name: &str, rel: &str, text: &str, iters: usize, detail: &str) {
    let iters = iters * std::env::var("DAEGUN_BENCH_SCALE").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
    let fc = cache_of(rel);
    let face = Face::new(&fc, &[]);

    let mut probe = Buffer::new();
    probe.push_str(text);
    let direction = shape::guess_segment_properties(&mut probe);
    let tags = probe.script.map(daegun::daecore::daeshaper::ot::tag::script_tags);
    let plan = ShapePlan::with_script(
        probe.script,
        &face,
        direction,
        tags.as_ref().map_or(&[][..], |t| t.as_slice()),
        &[],
        &[],
        &[],
        &[],
    );

    let mut warm = Buffer::new();
    warm.push_str(text);
    let warm_dir = shape::guess_segment_properties(&mut warm);
    shape::shape(&face, &plan, &mut warm, warm_dir);
    let first = shape::shaped_glyphs(&warm);
    let expect = first.len();
    assert!(expect > 0, "{name}: shaped nothing at all");
    assert!(
        first.iter().all(|g| g.glyph_id != 0),
        "{name}: a .notdef means the fixture stopped mapping this text and the bench times a notdef pass",
    );
    assert!(
        first.iter().map(|g| g.x_advance as i64).sum::<i64>() != 0,
        "{name}: every advance is zero, so positioning produced nothing to measure",
    );

    let mut samples = Vec::with_capacity(iters);
    let mut proof = 0usize;
    for _ in 0..iters {
        let mut buffer = Buffer::new();
        buffer.push_str(text);
        let target = shape::guess_segment_properties(&mut buffer);
        let t = Instant::now();
        shape::shape(&face, &plan, &mut buffer, target);
        samples.push(t.elapsed());
        let r = shape::shaped_glyphs(&buffer);
        proof += r.iter().filter(|g| g.glyph_id != 0).count();
        core::hint::black_box(&r);
    }
    assert_eq!(
        proof,
        iters * expect,
        "{name}: every iteration must yield {expect} non-notdef glyphs; a short count means a run bailed \
         out early and the median is timing a failure path",
    );
    report(name, detail, &mut samples);
}

#[test]
#[ignore]
fn shape_latin_sentence_inter() {
    bench(
        "shape_latin_sentence_inter",
        "inter/InterVariable.ttf",
        "Typography is the craft of endowing human language with a durable visual form.",
        20_000,
        "InterVariable.ttf, default instance, Latin LTR, 78-character English sentence",
    );
}

#[test]
#[ignore]
fn shape_latin_ligatures_eb_garamond() {
    bench(
        "shape_latin_ligatures_eb_garamond",
        "eb-garamond/EBGaramond.ttf",
        "The office staff shuffled a fistful of waffles.",
        20_000,
        "EBGaramond.ttf, Latin LTR, a sentence chosen so liga fires repeatedly (ffi, ffl, fi, fl)",
    );
}

#[test]
#[ignore]
fn shape_arabic_joined_run_scheherazade() {
    bench(
        "shape_arabic_joined_run_scheherazade",
        "scheherazade-new/ScheherazadeNew-Regular.ttf",
        "السلام عليكم ورحمة الله وبركاته",
        15_000,
        "ScheherazadeNew-Regular.ttf, Arabic RTL, one fully joined run: init/medi/fina plus marks",
    );
}

#[test]
#[ignore]
fn shape_devanagari_conjuncts_noto() {
    bench(
        "shape_devanagari_conjuncts_noto",
        "noto-devanagari/NotoSansDevanagari.ttf",
        "हिन्दी भाषा में स्वागत है",
        8_000,
        "NotoSansDevanagari.ttf, Devanagari LTR, conjuncts and matra reordering",
    );
}

#[test]
#[ignore]
fn shape_cjk_sentence_source_han() {
    bench(
        "shape_cjk_sentence_source_han",
        "source-han-sans/SourceHanSansJP-VF.otf",
        "日本語の組版では、文字の間隔と行の組み方が読みやすさを決める。",
        120_000,
        "SourceHanSansJP-VF.otf, Japanese LTR, 30 characters, no GSUB substitution to speak of",
    );
}
