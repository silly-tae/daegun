use daegun::daerizer::daegpu::{GlyphSlot, GpuBatch};
use daegun::daecore::daetype::TableBytes;

pub type TableMap = std::collections::BTreeMap<String, TableBytes>;

pub struct Face {
    tables: TableMap,
    loca: Vec<usize>,
    upm: f32,
}

impl Face {
    pub fn load(rel: &str) -> Face {
        let bytes = std::fs::read(std::format!("{}/{rel}", super::fonts_dir())).expect("read font");
        let tables = daegun::daecore::daetype::decoder::extract_ttf_tables(&bytes).expect("tables");
        let head = tables.get("head").expect("head");
        let format = daegun::daecore::daetype::decoder::read_i16_be(head, 50).expect("loca format");
        let upm = f32::from(daegun::daecore::daetype::decoder::read_u16_be(head, 18).expect("upm"));
        let count = daegun::daecore::daetype::decoder::read_u16_be(tables.get("maxp").expect("maxp"), 4)
            .expect("num glyphs") as usize;
        let loca = daegun::daecore::daetype::instancer::parse_loca(&tables, format, count).expect("loca");
        Face { tables, loca, upm }
    }

    pub fn glyph(&self, batch: &mut GpuBatch, gid: u16) -> Option<GlyphSlot> {
        let mut pen = daegun::daerizer::daegpu::collector(self.upm);
        daegun::daecore::daetype::outline::outline_glyf_glyph_with_loca(
            &self.tables,
            &self.loca,
            gid,
            &mut pen,
        )
        .ok()?;
        let mut curves = pen.finish().ok()?;
        batch.append(&mut curves)
    }

    pub fn fill(&self, batch: &mut GpuBatch, range: core::ops::Range<u16>, step: usize) -> usize {
        range.step_by(step).filter(|&g| self.glyph(batch, g).is_some()).count()
    }
}
