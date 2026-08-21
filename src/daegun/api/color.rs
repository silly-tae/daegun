use super::*;

// What a COLR layer resolves to when it defers to the caller's text color and the caller did not
// name one. Opaque, because transparent would drop the layer silently.
pub(crate) const FOREGROUND: crate::daerizer::Rgba =
    crate::daerizer::Rgba { r: 0, g: 0, b: 0, a: 255 };

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
        self.render_colr_glyph_with(gid, px, axes, palette_index, FOREGROUND)
    }

    pub fn render_colr_glyph_with(
        &self,
        gid: u16,
        px: f32,
        axes: &[(&str, f64)],
        palette_index: u16,
        foreground: crate::daerizer::Rgba,
    ) -> Option<crate::daerizer::RenderedScene> {
        let mut scene = crate::daerizer::DisplayList::default();
        let drew = {
            let mut outline = |g: u16| {
                let mut p = crate::daecore::daetype::outline::Path::default();
                self.outline_glyph_instanced(g, axes, &mut p)?;
                (!p.is_empty()).then_some(p)
            };
            self.colr_display_list(gid, axes, palette_index, foreground, &mut outline, &mut scene)
        };
        if !drew {
            return None;
        }
        crate::daerizer::render(&scene, px, f32::from(self.upm()))
    }

    // v1 is a paint graph, v0 a flat stack of layers; both lower to the same Fill ops, so the CPU
    // and GPU paths stay version-blind.
    pub(crate) fn colr_display_list(
        &self,
        gid: u16,
        axes: &[(&str, f64)],
        palette_index: u16,
        foreground: crate::daerizer::Rgba,
        outline: &mut dyn FnMut(u16) -> Option<crate::daecore::daetype::outline::Path>,
        out: &mut crate::daerizer::DisplayList,
    ) -> bool {
        if let Some(paint) = self.colr_v1_paint(gid, axes, palette_index) {
            crate::daerizer::colr::lower(&paint, crate::daerizer::IDENTITY, outline, foreground, out);
            return !out.is_empty();
        }

        let Some(layers) = self.colr_layers_for_palette(gid, palette_index) else { return false };
        // Back to front, which is the order COLR v0 records them in.
        for (layer_gid, r, g, b, a, is_foreground) in layers {
            let Some(path) = outline(layer_gid) else { continue };
            let path = out.push_path(path);
            let color =
                if is_foreground { foreground } else { crate::daerizer::Rgba { r, g, b, a } };
            out.push(crate::daerizer::Op::Fill {
                path,
                paint: crate::daerizer::Paint::Solid(color),
                rule: crate::daecore::daetype::outline::FillRule::NonZero,
                transform: crate::daerizer::IDENTITY,
            });
        }
        !out.is_empty()
    }

    pub fn colr_v1_paint(&self, gid: u16, axes: &[(&str, f64)], palette_index: u16) -> Option<Paint> {
        let location = self.cache.compute_location_rs(&owned_axes(axes));
        self.cache.colr_v1_paint(gid, &location, palette_index)
    }
}
