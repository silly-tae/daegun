use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn mutate(bytes: &mut Vec<u8>, rng: &mut Rng) {
    let num_tables = |b: &[u8]| -> usize {
        if b.len() < 28 { return 0 }
        let n = u16::from_be_bytes([b[4], b[5]]) as usize;
        if 12 + n * 16 <= b.len() { n } else { 0 }
    };
    let window = |b: &[u8], rng: &mut Rng, w: usize| -> Option<usize> {
        b.len().checked_sub(w).map(|slack| rng.below(slack + 1))
    };

    if bytes.is_empty() { return; }
    match rng.below(8) {
        0 => { let at = rng.below(bytes.len()); bytes.truncate(at); }
        1 => { let at = rng.below(bytes.len()); bytes[at] ^= 1 << rng.below(8); }
        2 => if let Some(at) = window(bytes, rng, 2) {
            bytes[at] = 0xFF; bytes[at + 1] = 0xFF;
        },
        3 => if let Some(at) = window(bytes, rng, 4) {
            for b in &mut bytes[at..at + 4] { *b = 0xFF; }
        },
        4 => if let Some(at) = window(bytes, rng, 4) {
            for b in &mut bytes[at..at + 4] { *b = 0; }
        },
        5 => {
            let n = num_tables(bytes);
            if n >= 2 {
                let (a, b) = (rng.below(n), rng.below(n));
                for k in 0..16 {
                    bytes.swap(12 + a * 16 + k, 12 + b * 16 + k);
                }
            }
        }
        6 => {
            let n = num_tables(bytes);
            if n >= 1 {
                let e = 12 + rng.below(n) * 16 + 8;
                let off = (rng.next() % bytes.len() as u64) as u32;
                bytes[e..e + 4].copy_from_slice(&off.to_be_bytes());
            }
        }
        _ => {
            let at = rng.below(bytes.len());
            let len = rng.below(64).min(bytes.len() - at);
            let fill = (rng.next() & 0xFF) as u8;
            for b in &mut bytes[at..at + len] { *b = fill; }
        }
    }
}

macro_rules! must {
    ($cond:expr, $($msg:tt)*) => {
        if !$cond { panic!("invariant: {}", format!($($msg)*)) }
    };
}

fn bounded(f: &daegun::Font) -> bool {
    f.num_glyphs() > 0
}

fn check_run(f: &daegun::Font, run: &daegun::ShapedRun, text: &str, what: &str) {
    let g = run.glyphs.len();
    must!(run.advances.len() == g, "{what}: advances {} vs glyphs {g}", run.advances.len());
    must!(run.offsets.len() == g, "{what}: offsets {} vs glyphs {g}", run.offsets.len());
    must!(run.clusters.len() == g, "{what}: clusters {} vs glyphs {g}", run.clusters.len());
    must!(run.unsafe_to_break.len() == g, "{what}: flags {} vs glyphs {g}", run.unsafe_to_break.len());
    for (name, v) in [("unsafe_to_concat", &run.unsafe_to_concat),
                      ("safe_to_insert_tatweel", &run.safe_to_insert_tatweel)] {
        must!(v.is_empty() || v.len() == g, "{what}: {name} {} vs glyphs {g}", v.len());
    }

    const SYLLABIC: [&str; 4] = ["indic", "khmer", "myanmar", "universal"];
    must!(
        !run.has_broken_syllable || SYLLABIC.contains(&run.shaper) || run.shaper == "myanmar_zawgyi",
        "{what}({text:?}): shaper {} reported a broken syllable", run.shaper,
    );
    let _ = run.complete;
    let n_chars = text.chars().count() as u32;
    for (i, &a) in run.advances.iter().enumerate() {
        must!(a.is_finite(), "{what}: advance[{i}] is {a}");
    }
    for (i, &(x, y)) in run.offsets.iter().enumerate() {
        must!(x.is_finite() && y.is_finite(), "{what}: offset[{i}] is ({x}, {y})");
    }
    for (i, &c) in run.clusters.iter().enumerate() {
        must!(c <= n_chars, "{what}: cluster[{i}] is {c} for {n_chars} chars");
    }
    let ascends = run.clusters.windows(2).all(|w| w[0] <= w[1]);
    let descends = run.clusters.windows(2).all(|w| w[0] >= w[1]);
    must!(
        ascends || descends,
        "{what}({text:?}): a monotone level turned round mid-run: {:?}", run.clusters,
    );
    for (i, &gid) in run.glyphs.iter().enumerate() {
        must!(gid == 0 || !bounded(f) || gid < f.num_glyphs(),
              "{what}({text:?}): glyph[{i}] is {gid} of {}", f.num_glyphs());
    }
}

fn exercise(bytes: &[u8], texts: &[String]) {
    let Ok(f) = daegun::Font::from_bytes(bytes) else { return };

    let n = f.num_glyphs();
    let bound = bounded(&f);
    let _ = (f.upm(), f.ascender(), f.descender(), f.cap_height(), f.bbox(), f.flags());
    let _ = (f.italic_angle(), f.style(), f.is_variable(), f.palette_count());
    let _ = (f.line_metrics(false), f.line_metrics(true), f.default_vertical_origin());
    let _ = (f.os2_info(), f.typographic_metrics(&[]), f.names(), f.stat_info());
    let _ = (f.table_tags(), f.glyph_names(), f.named_instances(), f.axes());
    let _ = (f.tracking(12.0, false), f.math_min_connector_overlap(), f.math_constants());
    for (cp, gid) in f.coverage() {
        must!(!bound || gid < n, "coverage: U+{cp:04X} -> {gid} of {n}");
    }
    let cps = f.codepoints();
    for &cp in cps.iter().take(400) {
        must!(f.has_glyph(cp) == f.glyph_id(cp).is_some(),
              "has_glyph(U+{cp:04X}) is {} but glyph_id is {:?}", f.has_glyph(cp), f.glyph_id(cp));
    }
    must!(f.glyph_names().len() == n as usize,
          "glyph_names has {} entries for {n} glyphs", f.glyph_names().len());
    for vertical in [false, true] {
        let m = f.line_metrics(vertical);
        must!(m.ascent.is_finite() && m.descent.is_finite() && m.line_height().is_finite(),
              "line_metrics(vertical={vertical}) is not finite");
    }

    for tag in ["head", "hhea", "maxp", "OS/2", "post", "cmap", "glyf", "loca", "CFF ", "GSUB", "MATH"] {
        let _ = f.table(tag);
        let _ = f.has_table(tag);
    }

    let axes: Vec<(String, f64)> = f.axes().into_iter().map(|a| (a.tag, a.max)).collect();
    let at: Vec<(&str, f64)> = axes.iter().map(|(t, v)| (t.as_str(), *v)).collect();
    must!(f.normalized_axes(&at) == f.normalized_axes(&at), "normalized_axes not idempotent");
    must!(f.typographic_metrics(&at) == f.typographic_metrics(&at), "typographic_metrics not idempotent");
    let inst = f.instance(&at);
    must!(inst == f.instance(&at), "instance not idempotent");
    if let Some(tables) = f.instance_tables(&at) {
        must!(daegun::build_font(&tables) == inst, "instance_tables + build_font != instance");
    }

    let probes: Vec<u16> = [0u16, 1, n / 2, n.saturating_sub(1), n, n.wrapping_add(1), u16::MAX]
        .into_iter().collect();
    for &g in &probes {
        let _ = (f.glyph_name(g), f.vertical_advance(g, &at));
        must!(
            !bound || g < n || (f.glyph_class(g).is_none() && f.mark_attachment_class(g) == 0),
            "glyph_class({g}) answered for a glyph past {n}",
        );
        if let Some((x0, y0, x1, y1)) = f.glyph_bounds(g, &at) {
            must!(x0 <= x1 && y0 <= y1, "glyph_bounds {g}: inverted ({x0},{y0})-({x1},{y1})");
            must!(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite(),
                  "glyph_bounds {g}: not finite");
        }
        let _ = (f.vertical_origin(g, &[]), f.cff_hints(g));
        let carets = f.ligature_carets(g, &at);
        must!(carets.iter().all(|v| v.is_finite()), "ligature_carets {g} not finite: {carets:?}");
        must!(carets.windows(2).all(|w| w[0] <= w[1]), "ligature_carets {g} not ascending: {carets:?}");
        let _ = f.advance_widths(&[g], &at);
        if let Some(bm) = f.rasterize_glyph(g, 16.0, &at) {
            let m = &bm.metrics;
            must!(
                bm.bitmap.len() == m.width as usize * m.height as usize,
                "raster {g}: {} bytes for {}x{}", bm.bitmap.len(), m.width, m.height,
            );
            must!(m.bounds.width >= 0.0 && m.bounds.height >= 0.0,
                  "raster {g}: negative bounds {:?}", m.bounds);
            must!(m.bounds.xmin.is_finite() && m.bounds.ymin.is_finite(),
                  "raster {g}: non-finite bounds {:?}", m.bounds);
            let again = f.rasterize_glyph(g, 16.0, &at).expect("rasterized once, so again");
            must!(again.bitmap == bm.bitmap, "raster {g}: cache hit changed the pixels");
            must!(
                (again.metrics.width, again.metrics.height, again.metrics.bounds)
                    == (m.width, m.height, m.bounds),
                "raster {g}: cache hit changed the metrics",
            );
        }
        let _ = f.glyph_bitmap(g, 16);
        let _ = (f.colr_layers(g), f.colr_layers_for_palette(g, 0), f.colr_v1_paint(g, &at, 0));
        let _ = (f.math_glyph_variants(g, true), f.math_italics_correction(g));
        let _ = (f.math_top_accent_attachment(g), f.math_is_extended_shape(g));
        let _ = f.math_kern(g, daegun::MathKernCorner::TopRight, 100.0);
        let _ = f.hinted_glyph(g, 16.0, &at, daegun::HintMode::Auto);
    }

    if let Ok(closed) = f.glyph_closure(&probes, &at) {
        for &g in &closed {
            must!(!bound || g < n, "glyph_closure -> {g} of {n}");
        }
        must!(closed.windows(2).all(|w| w[0] < w[1]), "glyph_closure is not ascending and unique");
        must!(n == 0 || closed.first() == Some(&0), "glyph_closure dropped .notdef: {closed:?}");
    }
    if let Ok(sub) = f.subset(&probes, &at)
        && let Ok(out) = daegun::Font::from_bytes(&sub.ttf) {
        for &g in probes.iter().filter(|&&g| g < n) {
            if let Some(new) = sub.new_gid(g) {
                must!(new < out.num_glyphs(), "subset: {g} -> {new} of {}", out.num_glyphs());
            }
        }
    }

    if let Some(g) = (0..n.min(8)).find(|&g| f.rasterize_glyph(g, 18.0, &at).is_some()) {
        let first = f.rasterize_glyph(g, 18.0, &at).expect("just rasterized");
        f.clear_glyph_cache();
        let cold = f.rasterize_glyph(g, 18.0, &at).expect("rasterizes again after a clear");
        must!(cold.bitmap == first.bitmap, "clear_glyph_cache changed the pixels for {g}");
        must!(
            (cold.metrics.width, cold.metrics.height, cold.metrics.bounds)
                == (first.metrics.width, first.metrics.height, first.metrics.bounds),
            "clear_glyph_cache changed the metrics for {g}",
        );
    }

    if !at.is_empty()
        && let Ok(inst_font) = daegun::Font::from_bytes(&inst) {
        for text in texts.iter().take(3) {
            let a = f.shape(text, &at, false).map(|r| r.glyphs.clone());
            let b = inst_font.shape(text, &[], false).map(|r| r.glyphs.clone());
            must!(a == b, "shape at axes != shape of the instance on {text:?}: {a:?} vs {b:?}");
        }
    }

    for text in texts.iter().map(String::as_str) {
        if let Some(r) = f.shape(text, &at, false) { check_run(&f, &r, text, "shape"); }
        if let Some(r) = f.shape(text, &at, true) { check_run(&f, &r, text, "shape vertical"); }
        if let Some(runs) = f.shape_bidi(text, &at, None) {
            let mut seen: Vec<usize> = runs.iter().flat_map(|b| b.chars.iter().copied()).collect();
            seen.sort_unstable();
            let before = seen.len();
            seen.dedup();
            must!(seen.len() == before, "shape_bidi({text:?}): a char index is in two runs");
            must!(
                seen.iter().all(|&i| i < text.chars().count().max(1)),
                "shape_bidi({text:?}): a char index is past the text",
            );
            for b in &runs { check_run(&f, &b.run, text, "shape_bidi"); }
        }
        if let Some(r) = f.shape_with_features(text, &at, false, Some("latn"), &[("liga", 0)]) {
            check_run(&f, &r, text, "shape_with_features");
        }
        if let Some(r) = f.shape_with_options(text, &at, false, &daegun::ShapeOptions {
            before: "x", after: "y", features: &[("kern", 1)], ..Default::default()
        }) { check_run(&f, &r, text, "shape_with_options"); }
        if let Some(r) = f.shape_with_options(text, &at, false, &daegun::ShapeOptions {
            report_unsafe_to_concat: true, report_tatweel_positions: true, ..Default::default()
        }) { check_run(&f, &r, text, "shape_with_flags"); }
        for ig in [daegun::Ignorables::Remove, daegun::Ignorables::Preserve] {
            if let Some(r) = f.shape_with_options(text, &at, false, &daegun::ShapeOptions {
                ignorables: ig, invisible_glyph: Some(1), ..Default::default()
            }) { check_run(&f, &r, text, "shape_ignorables"); }
        }
        let width = f.measure_width(text, &at, 1000.0);
        must!(width.is_finite(), "measure_width({text:?}) is {width}");
        if let Some(r) = f.shape(text, &at, false) {
            let summed: f64 = r.advances.iter().sum();
            must!(
                (width - summed).abs() < 0.05,
                "measure_width({text:?}) is {width}, shaping sums to {summed}",
            );
        }
        if let Some(c) = f.caret_positions(text, &at, false) {
            must!(
                c.len() == text.chars().count() + 1,
                "caret_positions: {} entries for {} chars", c.len(), text.chars().count(),
            );
            for (i, v) in c.iter().enumerate() { must!(v.is_finite(), "caret[{i}] is {v}"); }
        }
        let _ = f.glyph_ids(text);
        let _ = f.subset_text(text, &at);

        for w in [f64::INFINITY, 3000.0, 1.0] {
            let Some(l) = f.layout(text, &at, &daegun::LayoutOptions {
                max_inline_size: w, align: daegun::Align::Justify, ..Default::default()
            }) else { continue };
            let n_chars = text.chars().count();
            must!(l.inline_size.is_finite() && l.block_size.is_finite(), "layout size not finite");
            for pair in l.lines.windows(2) {
                must!(pair[0].chars.1 == pair[1].chars.0,
                      "layout lines do not tile: {:?} then {:?}", pair[0].chars, pair[1].chars);
            }
            for ln in &l.lines {
                must!(ln.chars.0 <= ln.chars.1 && ln.chars.1 <= n_chars,
                      "layout line range {:?} for {n_chars} chars", ln.chars);
                must!(ln.baseline.is_finite() && ln.inline_size.is_finite(),
                      "layout line geometry not finite");
                for r in &ln.runs { check_run(&f, &r.run, text, "layout run"); }
            }
        }
        let _ = f.justify(text, &at, false, &daegun::JustifyOptions {
            script_tag: "latn", lang_sys_tag: None, target_width: 5000.0, tolerance: 1.0,
        });
    }
}

fn texts_for(rng: &mut Rng) -> Vec<String> {
    const ALPHABET: &[char] = &[
        'A', 'a', 'f', 'i', ' ', '\n', '\t',
        'ا', 'ل', 'م', 'ه',                        // Arabic, all joining
        'ि', 'क', '्', 'ा',                          // Devanagari, with a virama
        'ก', '่', '้',                                // Thai, with tone marks
        '日', '本',                                   // CJK
        '\u{0301}', '\u{0327}', '\u{064B}',          // combining marks
        '\u{200D}', '\u{200C}',                      // ZWJ, ZWNJ
        '\u{200E}', '\u{200F}', '\u{061C}',          // LRM, RLM, ALM
        '\u{2066}', '\u{2069}',                      // isolates
        '(', ')', '«', '»', '.', '1', '2',
        '\u{0}', '\u{FFFD}', '\u{FEFF}',             // the ones that are not text
    ];
    let mut out: Vec<String> = ["", "A", "fi", "سلام (a) سلام", "हिन्दी", "日本語", "  \n  "]
        .into_iter().map(String::from).collect();
    for _ in 0..4 {
        let len = match rng.below(4) { 0 => rng.below(3), 1 | 2 => rng.below(12), _ => rng.below(120) };
        out.push((0..len).map(|_| ALPHABET[rng.below(ALPHABET.len())]).collect());
    }
    out
}

fn fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue }
            if matches!(p.extension().and_then(|x| x.to_str()), Some("ttf" | "otf" | "ttc")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str, default: u64| -> u64 {
        args.iter().position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let count = flag("--count", 2000);
    let seed = flag("--seed", 0);
    let only = args.iter().any(|a| a == "--seed");
    let dump: Option<String> = args.iter().position(|a| a == "--dump")
        .and_then(|i| args.get(i + 1)).cloned();

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/test-fonts");
    let corpus = fixtures(&root);
    if corpus.is_empty() {
        eprintln!("no fixtures under {}", root.display());
        std::process::exit(2);
    }

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let mut failures = Vec::new();
    let range: Box<dyn Iterator<Item = u64>> =
        if only { Box::new(seed..=seed) } else { Box::new(0..count) };

    for s in range {
        let mut rng = Rng(s.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let pick = &corpus[rng.below(corpus.len())];
        let Ok(bytes) = std::fs::read(pick) else { continue };
        let caught = catch_unwind(AssertUnwindSafe(|| {
            let mut bytes = bytes;
            for _ in 0..=rng.below(4) {
                mutate(&mut bytes, &mut rng);
            }
            if let Some(path) = &dump {
                let _ = std::fs::write(path, &bytes);
            }
            let texts = texts_for(&mut rng);
            exercise(&bytes, &texts);
        }));
        if let Err(payload) = caught {
            let why = payload.downcast_ref::<String>().map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("(no message)")
                .to_string();
            failures.push((s, pick.file_name().unwrap_or_default().to_string_lossy().to_string(), why));
        }
    }

    std::panic::set_hook(hook);

    if failures.is_empty() {
        println!("{} inputs, no panics", if only { 1 } else { count });
        return;
    }
    println!("{} panics", failures.len());
    for (s, f, why) in failures.iter().take(20) {
        println!("  seed {s} ({f}): {why}");
    }
    std::process::exit(1);
}
