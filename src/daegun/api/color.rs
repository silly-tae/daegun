use super::*;

impl Font {
    pub fn glyph_bitmap(&self, gid: u16, target_ppem: u16) -> Option<GlyphBitmap> {
        crate::daecore::daetype::bitmap::glyph_bitmap(&self.cache.table_map, gid, target_ppem)
    }

    pub fn colr_layers(&self, gid: u16) -> Option<Vec<ColrLayer>> {
        crate::daecore::daetype::colr_v0::colr_layers(&self.cache.table_map, gid)
    }

    pub fn colr_layers_for_palette(&self, gid: u16, palette_index: u16) -> Option<Vec<ColrLayer>> {
        crate::daecore::daetype::colr_v0::colr_layers_for_palette(&self.cache.table_map, gid, palette_index)
    }

    pub fn palette_count(&self) -> u16 {
        crate::daecore::daetype::colr_v0::cpal_palette_count(&self.cache.table_map)
    }

    pub fn palette_info(&self) -> Vec<PaletteInfo> {
        crate::daecore::daetype::colr_v0::cpal_palette_info(&self.cache.table_map)
    }

    pub fn render_colr_glyph(
        &self,
        gid: u16,
        px: f32,
        axes: &[(&str, f64)],
        palette_index: u16,
    ) -> Option<crate::daerizer::RenderedScene> {
        let paint = self.colr_v1_paint(gid, axes, palette_index)?;
        let mut scene = crate::daerizer::DisplayList::default();
        let mut outline = |g: u16| {
            let mut p = crate::daecore::daetype::outline::Path::default();
            self.outline_glyph_instanced(g, axes, &mut p)?;
            (!p.is_empty()).then_some(p)
        };
        crate::daerizer::colr::lower(
            &paint,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            &mut outline,
            crate::daerizer::Rgba::default(),
            &mut scene,
        );
        crate::daerizer::render(&scene, px, f32::from(self.upm()))
    }

    pub fn colr_v1_paint(&self, gid: u16, axes: &[(&str, f64)], palette_index: u16) -> Option<Paint> {
        let location = self.cache.compute_location_rs(&owned_axes(axes));
        self.cache.colr_v1_paint(gid, &location, palette_index)
    }
}
