use super::*;

impl Font {
    pub fn glyph_ids(&self, text: &str) -> Vec<Option<u16>> {
        text.chars().map(|c| self.cache.glyph_id(c as u32)).collect()
    }

    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        self.cache.glyph_id(codepoint)
    }

    pub fn has_glyph(&self, codepoint: u32) -> bool {
        self.cache.glyph_id(codepoint).is_some()
    }

    pub fn coverage(&self) -> Vec<(u32, u16)> {
        const MAX_ENTRIES: usize = 200_000;
        self.cache
            .table_map
            .get("cmap")
            .and_then(|cmap| crate::daecore::daetype::subsetter::cmap_entries(cmap, MAX_ENTRIES))
            .map(|mut e| {
                e.retain(|&(_, g)| self.cache.glyph_in_range(g).is_some());
                e
            })
            .unwrap_or_default()
    }

    pub fn codepoints(&self) -> Vec<u32> {
        self.coverage().into_iter().map(|(c, _)| c).collect()
    }

    pub fn glyph_bounds(&self, gid: u16, axes: &[(&str, f64)]) -> Option<(f64, f64, f64, f64)> {
        let mut path = crate::daecore::daetype::outline::Path::default();
        self.outline_glyph_instanced(gid, axes, &mut path)?;
        let (x0, y0, x1, y1) = path.bounds()?;
        let s = self.cache.scale_factor();
        Some((x0 * s, y0 * s, x1 * s, y1 * s))
    }

    pub fn variation_glyph_id(&self, base: u32, selector: u32) -> Option<u16> {
        self.cache.variation_glyph_id(base, selector)
    }

    pub fn advance_widths(&self, gids: &[u16], axes: &[(&str, f64)]) -> Vec<f64> {
        let axis_values = owned_axes(axes);
        gids.iter().map(|&gid| self.cache.advance_width_rs(&axis_values, gid) as f64).collect()
    }

    pub fn vertical_advance(&self, gid: u16, axes: &[(&str, f64)]) -> u32 {
        // Bounded by the glyph count, not just by table length: `VORG` and a `cmap` both answer for
        // ids past `maxp` – a confident wrong number where every other method here declines.
        if gid >= self.num_glyphs() {
            return 0;
        }
        let stated = self.cache.vertical_advance_rs(&owned_axes(axes), gid);
        if stated != 0 {
            return stated;
        }
        let height = self.ascender() - self.descender();
        if height > 0 { height as u32 } else { 1000 }
    }

    pub fn vertical_origin(&self, gid: u16, axes: &[(&str, f64)]) -> Option<i32> {
        if gid >= self.num_glyphs() {
            return None;
        }
        self.cache.font_vertical_origin_rs(&owned_axes(axes), gid)
    }

    pub fn default_vertical_origin(&self) -> i32 {
        if self.cache.table_map.contains_key("glyf") { return 0; }
        crate::daecore::daetype::vorg::vorg_default_origin_y(&self.cache.table_map)
            .map_or(0, |v| (v as f64 * self.cache.scale_factor()).round() as i32)
    }

    pub fn ligature_carets(&self, gid: u16, axes: &[(&str, f64)]) -> Vec<f64> {
        let Some(gdef) = self.cache.table_map.get("GDEF") else { return Vec::new() };
        let location = self.cache.compute_location_rs(&owned_axes(axes));
        let loca = self.cache.loca_offsets();
        let glyf = self.cache.table_map.get("glyf");
        let outline = match (glyf, &loca) {
            (Some(g), Some(l)) => Some((g.as_slice(), l.as_slice())),
            _ => None,
        };
        let scale = 1000.0 / self.cache.font_upm() as f64;
        crate::daecore::daetype::lig_caret::ligature_carets(gdef, gid, outline, &location)
            .into_iter()
            .map(|v| v * scale)
            .collect()
    }

    pub fn caret_positions(&self, text: &str, axes: &[(&str, f64)], vertical: bool) -> Option<Vec<f64>> {
        let run = self.shape(text, axes, vertical)?;
        let n_chars = text.chars().count();
        let rtl = crate::text::shape::run_is_rtl(text, vertical);

        let mut glyph_x = Vec::with_capacity(run.glyphs.len() + 1);
        let mut x = 0.0;
        for a in &run.advances {
            glyph_x.push(x);
            x += *a;
        }
        glyph_x.push(x);
        let total = x;

        let mut out = alloc::vec![0.0f64; n_chars + 1];
        out[n_chars] = if rtl { 0.0 } else { total };

        let mut counts: alloc::collections::BTreeMap<usize, usize> = alloc::collections::BTreeMap::new();
        for &c in &run.clusters {
            *counts.entry(c as usize).or_insert(0) += 1;
        }

        for (i, &cluster) in run.clusters.iter().enumerate() {
            let first = cluster as usize;
            if first > n_chars { continue; }
            let covered = counts.get(&first).copied().unwrap_or(1).max(1);
            let next = counts.range(first + 1..).next().map(|(&k, _)| k).unwrap_or(n_chars);
            let span = next.saturating_sub(first);
            let (left, right) = (glyph_x[i], glyph_x[i + 1]);

            if span <= 1 || covered > 1 {
                out[first] = if rtl { right } else { left };
                continue;
            }

            let carets = self.ligature_carets(run.glyphs[i], axes);
            for k in 0..span {
                let at = first + k;
                if at > n_chars { break; }
                let offset = if k == 0 {
                    0.0
                } else if let Some(c) = carets.get(k - 1) {
                    *c
                } else {
                    (right - left) * k as f64 / span as f64
                };
                out[at] = if rtl { right - offset } else { left + offset };
            }
        }

        Some(out)
    }

    pub fn glyph_class(&self, gid: u16) -> Option<GlyphClass> {
        self.cache.glyph_in_range(gid)?;
        let gdef = self.cache.table_map.get("GDEF")?;
        match crate::daecore::daetype::subsetter::otl::gdef::glyph_class(gdef, gid) {
            1 => Some(GlyphClass::Base),
            2 => Some(GlyphClass::Ligature),
            3 => Some(GlyphClass::Mark),
            4 => Some(GlyphClass::Component),
            _ => None,
        }
    }

    pub fn mark_attachment_class(&self, gid: u16) -> u16 {
        if self.cache.glyph_in_range(gid).is_none() {
            return 0;
        }
        self.cache
            .table_map
            .get("GDEF")
            .map_or(0, |gdef| crate::daecore::daetype::subsetter::otl::gdef::mark_attach_class(gdef, gid))
    }

    pub fn glyph_name(&self, gid: u16) -> Option<String> {
        crate::daecore::daetype::glyph_names::glyph_name(
            self.cache.table_map.get("post").map(|t| t.as_slice()),
            self.cache.cff().map(|t| t.as_slice()),
            gid,
        )
    }

    pub fn glyph_names(&self) -> Vec<Option<String>> {
        crate::daecore::daetype::glyph_names::glyph_names(
            self.cache.table_map.get("post").map(|t| t.as_slice()),
            self.cache.cff().map(|t| t.as_slice()),
            self.num_glyphs(),
        )
    }
}
