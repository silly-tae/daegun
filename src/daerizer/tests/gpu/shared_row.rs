use daegun::daerizer::daegpu::{eval, GlyphSlot, GpuBatch, SubpixelParams};
use daegun::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};
use super::fonts_dir;

fn glyphs(rel: &str, upto: u16) -> (GpuBatch, Vec<GlyphSlot>) {
    let bytes = std::fs::read(format!("{}/{rel}", fonts_dir())).expect("read font");
    let tables = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("tables");
    let head = tables.get("head").expect("head");
    let format = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("loca format");
    let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
    let count = daegun::daecore::daetype::decoder::read_u16_be(tables.get("maxp").expect("maxp"), 4)
        .expect("num glyphs") as usize;
    let loca = daegun::daecore::daetype::instancer::parse_loca(&tables, format, count).expect("loca");

    let mut batch = GpuBatch::new();
    let mut slots = Vec::new();
    for gid in 1..upto.min(count as u16) {
        let mut pen = daegun::daerizer::daegpu::collector(upm);
        if daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&tables, &loca, gid, &mut pen).is_err() {
            continue;
        }
        let Ok(mut curves) = pen.finish() else { continue };
        if let Some(slot) = batch.append(&mut curves) {
            slots.push(slot);
        }
    }
    (batch, slots)
}

fn oracle(
    batch: &GpuBatch,
    slot: &GlyphSlot,
    em: [f32; 2],
    em_pixels: [f32; 2],
    params: &SubpixelParams,
) -> [f32; 3] {
    let (ox, oy) = (params.oversample[0].max(1), params.oversample[1].max(1));
    let taps_x = params.taps[0].clamp(1, 8);
    let taps_y = params.taps[1].clamp(1, 8);
    let sample_m = [em_pixels[0] * ox as f32, em_pixels[1] * oy as f32];
    let ss = params.supersample.clamp(1, 4);
    let inv_ss = 1.0 / (ss * ss) as f32;

    let offset = |tap: u32, origin: i32, oversample: u32, ppem: f32| -> f32 {
        if oversample == 0 || ppem == 0.0 {
            return 0.0;
        }
        ((origin as f32 + tap as f32 + 0.5) / oversample as f32 - 0.5) / ppem
    };
    let jitter = |i: u32, m: f32| -> f32 {
        if ss == 1 || m == 0.0 || !m.is_finite() {
            return 0.0;
        }
        ((i as f32 + 0.5) / ss as f32 - 0.5) / m
    };

    let mut out = [0.0f32; 3];
    for ty in 0..taps_y {
        let dy = -offset(ty, params.origin[1], oy, em_pixels[1]);
        for tx in 0..taps_x {
            let dx = offset(tx, params.origin[0], ox, em_pixels[0]);
            let mut cov = 0.0;
            for sy in 0..ss {
                for sx in 0..ss {
                    let at = [em[0] + dx + jitter(sx, sample_m[0]), em[1] + dy + jitter(sy, sample_m[1])];
                    cov += eval::coverage(batch, slot, at, sample_m);
                }
            }
            cov *= inv_ss;
            let index = (ty * taps_x + tx) as usize;
            for (c, o) in out.iter_mut().enumerate() {
                if c as u32 >= params.channels {
                    break;
                }
                *o += params.weights.get(c * 64 + index).copied().unwrap_or(0.0) * cov;
            }
        }
    }
    if params.channels < 2 {
        out = [out[0]; 3];
    }
    for c in out.iter_mut() {
        *c = c.clamp(0.0, 1.0);
    }
    out
}

#[test]
fn shared_row_matches_a_sample_at_a_time() {
    let layouts: [(&str, SubpixelParams); 6] = [
        ("grayscale", SubpixelParams::default()),
        ("horizontal", SubpixelParams::from_layout(&SubpixelLayout::horizontal(StripeOrder::Rgb))),
        ("vertical", SubpixelParams::from_layout(&SubpixelLayout::vertical(StripeOrder::Rgb))),
        ("unfiltered", SubpixelParams::from_layout(&SubpixelLayout::unfiltered(StripeOrder::Rgb, false))),
        ("horizontal ss=2",
         SubpixelParams::from_layout(&SubpixelLayout::horizontal(StripeOrder::Rgb)).with_supersampling(2)),
        ("vertical ss=3",
         SubpixelParams::from_layout(&SubpixelLayout::vertical(StripeOrder::Rgb)).with_supersampling(3)),
    ];

    let (batch, slots) = glyphs("eb-garamond/EBGaramond.ttf", 260);
    assert!(slots.len() > 100, "only {} glyphs built", slots.len());

    let mut compared = 0usize;
    for (name, params) in &layouts {
        for slot in &slots {
            for px in [11.0f32, 41.0] {
                for iy in 0..7 {
                    for ix in 0..7 {
                        let at = [
                            slot.box_min[0] + (slot.box_max[0] - slot.box_min[0]) * (ix as f32 - 0.5) / 6.0,
                            slot.box_min[1] + (slot.box_max[1] - slot.box_min[1]) * (iy as f32 - 0.5) / 6.0,
                        ];
                        let got = eval::coverage_channels(&batch, slot, at, [px, px], params);
                        let want = oracle(&batch, slot, at, [px, px], params);
                        assert_eq!(
                            got.map(f32::to_bits),
                            want.map(f32::to_bits),
                            "{name} at {at:?}, {px}px: {got:?} against {want:?}",
                        );
                        compared += 1;
                    }
                }
            }
        }
    }
    assert!(compared > 100_000, "only {compared} samples compared");
}

#[test]
fn a_single_sample_row_agrees_with_the_shared_form() {
    let (batch, slots) = glyphs("inter/InterVariable.ttf", 200);
    let narrow = SubpixelParams::default();
    let column = SubpixelParams::from_layout(&SubpixelLayout::vertical(StripeOrder::Rgb));

    let mut checked = 0usize;
    for params in [&narrow, &column] {
        for slot in slots.iter().take(120) {
            for i in 0..25 {
                let at = [
                    slot.box_min[0] + (slot.box_max[0] - slot.box_min[0]) * (i % 5) as f32 / 4.0,
                    slot.box_min[1] + (slot.box_max[1] - slot.box_min[1]) * (i / 5) as f32 / 4.0,
                ];
                assert_eq!(
                    eval::coverage_channels(&batch, slot, at, [23.0, 23.0], params).map(f32::to_bits),
                    oracle(&batch, slot, at, [23.0, 23.0], params).map(f32::to_bits),
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 5_000, "only {checked} samples compared");
}
