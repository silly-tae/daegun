use std::time::{Duration, Instant};

use daegun::daecore::daetype::outline::OutlinePen;
use daegun::daerizer::daecpu::math::{Geometry, Glyph};
use daegun::daerizer::daecpu::rasterize::{metrics_raw, Raster};
use daegun::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};
use daegun::daecore::daetype::TableBytes;

const WARMUP: usize = 40;
const ROUNDS: usize = 60;

struct Face {
    map: std::collections::BTreeMap<String, TableBytes>,
    loca: Vec<usize>,
    upm: f32,
}

fn face(rel: &str) -> Face {
    let path = format!("{}/{}", crate::FONTS, rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    let map = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("parses");
    let head = map.get("head").expect("head");
    let fmt = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("locaFmt");
    let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
    let n = daegun::daecore::daetype::decoder::read_u16_be(map.get("maxp").expect("maxp"), 4).expect("n");
    let loca = daegun::daecore::daetype::instancer::parse_loca(&map, fmt, n as usize).expect("loca");
    Face { map, loca, upm }
}

fn gid_for(f: &Face, c: char) -> u16 {
    let cmap = f.map.get("cmap").expect("cmap");
    daegun::daecore::daetype::subsetter::cmap_glyph_id(cmap, c as u32).unwrap_or_else(|| panic!("no {c}"))
}

fn pen_ops(f: &Face, gid: u16) -> usize {
    #[derive(Default)]
    struct Count(usize);
    impl OutlinePen for Count {
        fn move_to(&mut self, _: f32, _: f32) { self.0 += 1; }
        fn line_to(&mut self, _: f32, _: f32) { self.0 += 1; }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) { self.0 += 1; }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) { self.0 += 1; }
        fn close(&mut self) { self.0 += 1; }
    }
    let mut c = Count::default();
    daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&f.map, &f.loca, gid, &mut c).ok();
    c.0
}

fn flatten(f: &Face, gid: u16, px: f32) -> Glyph {
    let mut g = Geometry::new(px, f.upm);
    daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&f.map, &f.loca, gid, &mut g)
        .expect("the glyph draws");
    let mut out = Glyph::default();
    g.finalize(&mut out);
    out
}

fn median(v: &mut [Duration]) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn stages(name: &str, f: &Face, gid: u16, px: f32, layout: &SubpixelLayout, gamma: Option<&[u8; 256]>) {
    let glyph = flatten(f, gid, px);
    let scale = px / f.upm;
    let (metrics, ox, oy) = metrics_raw(scale, glyph.bounds, 0.0, 0.0, 0.0);
    let (pad_x, pad_y) = layout.pad();
    let w = metrics.width + pad_x * 2;
    let h = metrics.height + pad_y * 2;
    let (sx, sy) = layout.oversample();
    if w == 0 || h == 0 {
        eprintln!("  {name}: empty box, skipped");
        return;
    }

    #[derive(Default)]
    struct NullPen(usize);
    impl OutlinePen for NullPen {
        fn move_to(&mut self, _: f32, _: f32) { self.0 += 1; }
        fn line_to(&mut self, _: f32, _: f32) { self.0 += 1; }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) { self.0 += 1; }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) { self.0 += 1; }
        fn close(&mut self) { self.0 += 1; }
    }

    let (mut t_dec, mut t_flat, mut t_alloc, mut t_draw, mut t_res) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut proof = 0usize;
    for round in 0..WARMUP + ROUNDS {
        let t = Instant::now();
        let mut np = NullPen::default();
        daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&f.map, &f.loca, gid, &mut np).ok();
        let e_dec = t.elapsed();
        core::hint::black_box(&np.0);

        let t = Instant::now();
        let g = flatten(f, gid, px);
        let e_flat = t.elapsed();

        let t = Instant::now();
        let mut raster = Raster::new(w * sx as usize, h * sy as usize);
        let e_alloc = t.elapsed();

        let t = Instant::now();
        raster.draw(&g, scale * sx as f32, scale * sy as f32,
                    (ox + pad_x as f32) * sx as f32, (oy + pad_y as f32) * sy as f32);
        let e_draw = t.elapsed();

        let t = Instant::now();
        let bytes = raster.resolve(w, h, layout, gamma);
        let e_res = t.elapsed();

        proof += bytes.len() + g.v_segments.len();
        core::hint::black_box(&bytes);
        if round >= WARMUP {
            t_dec.push(e_dec);
            t_flat.push(e_flat.saturating_sub(e_dec));
            t_alloc.push(e_alloc);
            t_draw.push(e_draw);
            t_res.push(e_res);
        }
    }
    assert!(proof > 0, "{name}: the pipeline produced nothing");

    let dec = median(&mut t_dec);
    let f_ = median(&mut t_flat);
    let a_ = median(&mut t_alloc);
    let d_ = median(&mut t_draw);
    let r_ = median(&mut t_res);
    let total = f_ + a_ + d_ + r_;
    let pct = |d: Duration| d.as_secs_f64() / total.as_secs_f64() * 100.0;
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    eprintln!(
        "  {name:26} {w:>4}x{h:<4} raster {:>7.3}us | flatten {:>6.3} ({:>4.1}%)  alloc {:>6.3} ({:>4.1}%)  draw {:>6.3} ({:>4.1}%)  resolve {:>7.3} ({:>4.1}%)   [decode {:>5.3}us, daetype's]",
        us(total), us(f_), pct(f_), us(a_), pct(a_), us(d_), pct(d_), us(r_), pct(r_), us(dec),
    );
}

#[test]
#[ignore]
fn cpu_pipeline_by_size() {
    let f = face("eb-garamond/EBGaramond.ttf");
    let gray = SubpixelLayout::grayscale();
    eprintln!("cpu_pipeline_by_size – EBGaramond 'B', grayscale, no gamma");
    for gid_char in ['B'] {
        let gid = gid_for(&f, gid_char);
        eprintln!("    ({gid_char} is {} pen operations)", pen_ops(&f, gid));
        for px in [12.0f32, 16.0, 24.0, 32.0, 64.0, 128.0, 256.0] {
            stages(&format!("{gid_char} at {px}px"), &f, gid, px, &gray, None);
        }
    }
}

#[test]
#[ignore]
fn cpu_pipeline_by_glyph() {
    let f = face("eb-garamond/EBGaramond.ttf");
    let gray = SubpixelLayout::grayscale();
    eprintln!("cpu_pipeline_by_glyph – 16px, grayscale, no gamma");
    for c in ['.', 'l', 'o', 'B', 'g', 'W', '@'] {
        let gid = gid_for(&f, c);
        stages(&format!("{c} ({} ops)", pen_ops(&f, gid)), &f, gid, 16.0, &gray, None);
    }
}

#[test]
#[ignore]
fn cpu_pipeline_by_layout() {
    let f = face("eb-garamond/EBGaramond.ttf");
    let gid = gid_for(&f, 'B');
    let lut = daegun::daerizer::daecpu::platform::gamma_lut(2.2);
    eprintln!("cpu_pipeline_by_layout – EBGaramond 'B' at 16px");
    for (name, layout, gamma) in [
        ("grayscale", SubpixelLayout::grayscale(), None),
        ("grayscale + gamma", SubpixelLayout::grayscale(), Some(&lut)),
        ("subpixel RGB", SubpixelLayout::horizontal(StripeOrder::Rgb), None),
        ("subpixel RGB + gamma", SubpixelLayout::horizontal(StripeOrder::Rgb), Some(&lut)),
    ] {
        stages(name, &f, gid, 16.0, &layout, gamma);
    }
    eprintln!("  (the same at 64px)");
    for (name, layout, gamma) in [
        ("grayscale", SubpixelLayout::grayscale(), None),
        ("subpixel RGB", SubpixelLayout::horizontal(StripeOrder::Rgb), None),
    ] {
        stages(name, &f, gid, 64.0, &layout, gamma);
    }
}
