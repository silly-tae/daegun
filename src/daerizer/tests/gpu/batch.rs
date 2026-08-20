use daegun::daerizer::daegpu::GpuBatch;
use super::fonts_dir;

fn curves_of(file: &str, gid: u16) -> Option<Vec<[[f32; 2]; 3]>> {
    let bytes = std::fs::read(format!("{}/{file}", fonts_dir())).expect("read font");
    let tables = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("tables");
    let head = tables.get("head")?;
    let format = daegun::daecore::daetype::decoder::read_i16_be(head, 50)?;
    let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18)?);
    let count =
        daegun::daecore::daetype::decoder::read_u16_be(tables.get("maxp")?, 4)? as usize;
    let loca = daegun::daecore::daetype::instancer::parse_loca(&tables, format, count).ok()?;

    let mut pen = daegun::daerizer::daegpu::collector(upm);
    daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(&tables, &loca, gid, &mut pen).ok()?;
    pen.finish().ok()
}

#[test]
fn appending_a_glyph_whole_matches_building_it_in_halves() {
    let mut compared = 0usize;
    for gid in 1..120u16 {
        let Some(base) = curves_of("eb-garamond/EBGaramond.ttf", gid) else { continue };

        let mut whole = base.clone();
        let mut batch_a = GpuBatch::new();
        let Some(slot_a) = batch_a.append(&mut whole) else { continue };

        let mut halves = base.clone();
        let banded = GpuBatch::build_glyph(&mut halves).expect("build_glyph refused what append took");
        let mut batch_b = GpuBatch::new();
        let slot_b =
            batch_b.append_prebuilt(&halves, &banded).expect("append_prebuilt refused it");

        assert_eq!(slot_a, slot_b, "gid {gid}: the two paths returned different slots");
        assert_eq!(batch_a.curves(), batch_b.curves(), "gid {gid}: different curve data");
        assert_eq!(batch_a.bands(), batch_b.bands(), "gid {gid}: different band table");
        assert_eq!(
            batch_a.band_curves(), batch_b.band_curves(),
            "gid {gid}: different band membership",
        );
        assert_eq!(batch_a.hulls(), batch_b.hulls(), "gid {gid}: different drawn polygon");
        assert_eq!(batch_a.revision(), batch_b.revision(), "gid {gid}: different revision count");

        assert_eq!(whole, halves, "gid {gid}: the two paths left the caller's curves different");
        compared += 1;
    }
    assert!(compared > 40, "only {compared} glyphs were compared, so this proves little");
}

#[test]
fn the_two_paths_agree_across_a_batch_of_many_glyphs() {
    let gids: Vec<u16> = (1..40u16).collect();
    let mut batch_a = GpuBatch::new();
    let mut batch_b = GpuBatch::new();
    let mut added = 0usize;

    for &gid in &gids {
        let Some(base) = curves_of("eb-garamond/EBGaramond.ttf", gid) else { continue };
        let mut whole = base.clone();
        if batch_a.append(&mut whole).is_none() {
            continue;
        }
        let mut halves = base.clone();
        let banded = GpuBatch::build_glyph(&mut halves).expect("build_glyph");
        batch_b.append_prebuilt(&halves, &banded).expect("append_prebuilt");
        added += 1;
    }

    assert!(added > 20, "only {added} glyphs entered the batch, so this proves little");
    assert_eq!(batch_a.curves(), batch_b.curves(), "curve data diverged across the batch");
    assert_eq!(batch_a.bands(), batch_b.bands(), "band table diverged across the batch");
    assert_eq!(batch_a.band_curves(), batch_b.band_curves(), "band membership diverged");
    assert_eq!(batch_a.hulls(), batch_b.hulls(), "drawn polygons diverged");
    assert_eq!(batch_a.revision(), batch_b.revision(), "revision counts diverged");
}
