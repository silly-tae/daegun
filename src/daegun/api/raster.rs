use super::*;

impl Font {
    pub fn outline_glyph(&self, gid: u16, pen: &mut dyn crate::daecore::daetype::outline::OutlinePen) -> Option<()> {
        draw_glyph_outline(&self.cache, gid, pen)
    }

    pub fn outline_glyph_instanced(&self, gid: u16, axes: &[(&str, f64)], pen: &mut dyn crate::daecore::daetype::outline::OutlinePen) -> Option<()> {
        self.outline_glyph_keyed(gid, &crate::daecore::cache::canonical_axes(axes), pen)
    }

    pub(crate) fn prewarmed_outline(
        &self,
        gid: u16,
        axes: &crate::sync::Shared<crate::daecore::cache::AxisKey>,
    ) -> Option<crate::sync::Shared<crate::daecore::daetype::outline::Path>> {
        let mut cache = crate::sync::write(&self.outlines);
        if cache.len() == 0 { return None; }
        cache
            .get(&(gid, crate::sync::Shared::clone(axes)))
            .map(crate::sync::Shared::clone)
    }

    pub fn prewarm(&self, gids: impl IntoIterator<Item = u16>, axes: &[(&str, f64)]) -> usize {
        let axes_shared = self.cache.intern_axes(&crate::daecore::cache::canonical_axes(axes));
        let mut added = 0;
        for gid in gids {
            let key = (gid, crate::sync::Shared::clone(&axes_shared));
            if crate::sync::write(&self.outlines).get(&key).is_some() { continue; }
            let mut path = crate::daecore::daetype::outline::Path::default();
            if self.outline_glyph_keyed(gid, &axes_shared, &mut path).is_none() { continue; }
            if path.is_empty() { continue; }
            crate::sync::write(&self.outlines).insert(key, crate::sync::Shared::new(path));
            added += 1;
        }
        added
    }

    pub fn clear_prewarm(&self) {
        crate::sync::write(&self.outlines).clear();
    }

    pub fn hinted_glyph(
        &self,
        gid: u16,
        px: f32,
        axes: &[(&str, f64)],
        mode: HintMode,
    ) -> Option<crate::daecore::daetype::hinting::HintedOutline> {
        if !(px > 0.0 && px.is_finite()) {
            return None;
        }
        let opts = RasterOptions::default().with_hinting(mode);
        self.hinted_outline(gid, px, &crate::daecore::cache::canonical_axes(axes), &opts)
    }

    pub fn cff_hints(&self, gid: u16) -> Option<crate::daecore::daetype::outline::CffHints> {
        let cff = self.cache.cff()?;
        let outlines = self.cache.cff_outlines()?;
        let mut pen = crate::daecore::daetype::hinting::auto::CollectPen::new();
        crate::daecore::daetype::outline::outline_cff_glyph_hinted(&outlines, cff, gid, &mut pen).ok()
    }

    pub(crate) fn outline_glyph_keyed(
        &self,
        gid: u16,
        axes: &crate::daecore::cache::AxisKey,
        pen: &mut dyn crate::daecore::daetype::outline::OutlinePen,
    ) -> Option<()> {
        let location = self.cache.compute_location_keyed(axes);
        if !self.is_cff2 && location.iter().all(|&v| v == 0.0) {
            return draw_glyph_outline(&self.cache, gid, pen);
        }
        let instanced = self.cache.instanced_font_cache_keyed(axes);
        draw_glyph_outline(&instanced, gid, pen)
    }

    pub fn rasterize_glyph(&self, gid: u16, px: f32, axes: &[(&str, f64)]) -> Option<RasterizedGlyph> {
        self.rasterize_glyph_with(gid, px, axes, &RasterOptions::default())
    }

    pub fn rasterize_glyph_with(
        &self,
        gid: u16,
        px: f32,
        axes: &[(&str, f64)],
        opts: &RasterOptions,
    ) -> Option<RasterizedGlyph> {
        // Bounds the *requested* size. The size that actually results is bounded further down, and
        // they are not the same number: a font may declare unitsPerEm = 16, which turns px = 4096
        // into a scale of 256 and a box 16.7 million pixels a side, times 4 under a subpixel layout.
        const MAX_PX: f32 = 4096.0;
        if !(px > 0.0 && px <= MAX_PX) { return None; }

        if let Some(t) = opts.transform
        // A non-finite transform is refused rather than carried: NaN never updates the bounds
        // accumulator, so `finalize` computes a width of -inf and `ceil(-inf) as i32 as usize` is
        // 18_446_744_071_562_067_968 – which `Raster::new` then tries to allocate.
            && !t.iter().all(|v| v.is_finite()) {
                return None;
            }
        if let Some(g) = opts.gamma
            && !(g.is_finite() && g > 0.0) {
                return None;
            }

        let axes_key = crate::daecore::cache::canonical_axes(axes);
        let axes_shared = self.cache.intern_axes(&axes_key);
        let key = self.glyph_key(gid, px, &axes_shared, opts);
        let hit = crate::sync::write(&self.glyphs)
            .get(&key)
            .map(|c| (c.xmin, c.ymin, c.width, c.height, c.bounds, c.bitmap.clone()));
        if let Some((xmin, ymin, width, height, bounds, bitmap)) = hit {
            return Some(RasterizedGlyph {
                metrics: Metrics {
                    xmin,
                    ymin,
                    width,
                    height,
                    advance_width: self.cache.advance_keyed(&axes_key, gid, false) as f32 * px / self.upm() as f32,
                    advance_height: self.cache.advance_keyed(&axes_key, gid, true) as f32 * px / self.upm() as f32,
                    bounds,
                },
                bitmap,
            });
        }

        let upm = self.upm() as f32;
        let scale = px / upm;
        let layout = &opts.layout;

        let opts = &match opts.oblique {
            None => *opts,
            Some(t) if t.is_finite() => {
                let shear = [1.0f32, 0.0, t, 1.0, 0.0, 0.0];
                let m = match opts.transform {
                    None => shear,
                    Some(o) => [
                        shear[0] * o[0] + shear[1] * o[2],
                        shear[0] * o[1] + shear[1] * o[3],
                        shear[2] * o[0] + shear[3] * o[2],
                        shear[2] * o[1] + shear[3] * o[3],
                        shear[4] * o[0] + shear[5] * o[2] + o[4],
                        shear[4] * o[1] + shear[5] * o[3] + o[5],
                    ],
                };
                RasterOptions { transform: Some(m), oblique: None, ..*opts }
            }
            Some(_) => return None,
        };

        let tolerance_px = match opts.transform {
            Some(t) => px * transform_max_scale(&t),
            None => px,
        };
        let hinted = self.hinted_outline(gid, px, &axes_key, opts);
        let draw_scale = if hinted.is_some() { 1.0 } else { scale };

        let (mut canvas, mut glyph) = crate::sync::write(&self.raster_scratch)
            .take()
            .unwrap_or_else(|| {
                (
                    crate::daerizer::daecpu::rasterize::Raster::new(0, 0),
                    crate::daerizer::daecpu::math::Glyph::default(),
                )
            });
        let mut geometry = crate::daerizer::daecpu::math::Geometry::reusing(
            tolerance_px,
            if hinted.is_some() { px } else { upm },
            &mut glyph,
        );
        if let Some(style) = opts.stroke {
            let mut path = crate::daecore::daetype::outline::Path::default();
            match (&hinted, opts.transform) {
                (Some(out), _) => crate::daecore::daetype::hinting::draw_hinted(out, &mut path),
                (None, None) => self.outline_glyph_keyed(gid, &axes_key, &mut path)?,
                (None, Some(t)) => {
                    let mut pen = crate::daecore::daetype::outline::TransformPen::new(&mut path, t.map(f64::from));
                    self.outline_glyph_keyed(gid, &axes_key, &mut pen)?;
                }
            }
            let per_px = if hinted.is_some() { 1.0 } else { upm / px.max(1e-6) };
            crate::daecore::daetype::outline::stroke(&path, &style, 0.25 * per_px, &mut geometry);
        } else if let Some(units) = opts.embolden.filter(|u| u.is_finite() && *u > 0.0) {
            let mut path = crate::daecore::daetype::outline::Path::default();
            match (&hinted, opts.transform) {
                (Some(out), _) => crate::daecore::daetype::hinting::draw_hinted(out, &mut path),
                (None, None) => self.outline_glyph_keyed(gid, &axes_key, &mut path)?,
                (None, Some(t)) => {
                    let mut pen = crate::daecore::daetype::outline::TransformPen::new(&mut path, t.map(f64::from));
                    self.outline_glyph_keyed(gid, &axes_key, &mut pen)?;
                }
            }
            let per_px = if hinted.is_some() { 1.0 } else { upm / px.max(1e-6) };
            let width = if hinted.is_some() { units * px / upm } else { units };
            path.replay(None, &mut geometry);
            let style = crate::daecore::daetype::outline::StrokeStyle {
                width,
                join: crate::daecore::daetype::outline::Join::Round,
                cap: crate::daecore::daetype::outline::Cap::Round,
            };
            crate::daecore::daetype::outline::stroke(&path, &style, 0.25 * per_px, &mut geometry);
        } else {
            match (&hinted, opts.transform) {
                (Some(out), _) => crate::daecore::daetype::hinting::draw_hinted(out, &mut geometry),
                (None, t) => match self.prewarmed_outline(gid, &axes_shared) {
                    Some(outline) => outline.replay(t.map(|m| m.map(f64::from)).as_ref(), &mut geometry),
                    None => match t {
                        None => self.outline_glyph_keyed(gid, &axes_key, &mut geometry)?,
                        Some(t) => {
                            let mut pen = crate::daecore::daetype::outline::TransformPen::new(&mut geometry, t.map(f64::from));
                            self.outline_glyph_keyed(gid, &axes_key, &mut pen)?;
                        }
                    },
                },
            }
        }
        geometry.finalize(&mut glyph);

        let b = glyph.bounds;
        if !(b.xmin.is_finite() && b.ymin.is_finite() && b.width.is_finite() && b.height.is_finite())
            || b.width < 0.0
            || b.height < 0.0
        {
            return None;
        }

        let bold = opts.embolden.filter(|u| u.is_finite() && *u > 0.0).unwrap_or(0.0);
        let advance_width = self.cache.advance_keyed(&axes_key, gid, false) as f32 + bold;
        let advance_height = self.cache.advance_keyed(&axes_key, gid, true) as f32;
        let (mut metrics, offset_x, offset_y) =
            crate::daerizer::daecpu::rasterize::metrics_raw(draw_scale, glyph.bounds, advance_width, advance_height, 0.0);

        // Saturating, because the size refusal below only runs if the padding does not panic first.
        // `metrics_raw` saturates its casts, so enormous-but-finite bounds arrive at the rail and a
        // pad of one underflows: release wraps i32::MIN to a large *positive* xmin, and the glyph
        // then passes for reasonable and rasterizes somewhere it was never asked to.
        let (pad_x, pad_y) = layout.pad();
        metrics.xmin = metrics.xmin.saturating_sub(pad_x as i32);
        metrics.ymin = metrics.ymin.saturating_sub(pad_y as i32);
        metrics.width = metrics.width.saturating_add(pad_x.saturating_mul(2));
        metrics.height = metrics.height.saturating_add(pad_y.saturating_mul(2));

        const MAX_SAMPLES: usize = 33_554_432;
        const MAX_RESOLVE_OPS: usize = 134_217_728;

        let (sx, sy) = layout.oversample();
        let (taps_x, taps_y) = layout.taps();
        let samples = metrics
            .width
            .checked_mul(sx as usize)
            .and_then(|w| metrics.height.checked_mul(sy as usize).and_then(|h| w.checked_mul(h)))?;
        if samples > MAX_SAMPLES {
            return None;
        }

        let resolve_ops = metrics
            .width
            .checked_mul(metrics.height)
            .and_then(|px| px.checked_mul(layout.channels() as usize))
            .and_then(|v| v.checked_mul(taps_x as usize))
            .and_then(|v| v.checked_mul(taps_y as usize))?;
        if resolve_ops > MAX_RESOLVE_OPS {
            return None;
        }

        let (ox, oy) = (f32::from(sx), f32::from(sy));
        canvas.reset(metrics.width * sx as usize, metrics.height * sy as usize);
        canvas.draw(
            &glyph,
            draw_scale * ox,
            draw_scale * oy,
            (offset_x + pad_x as f32) * ox,
            (offset_y + pad_y as f32) * oy,
        );

        let gamma = opts.gamma.map(|g| self.gamma_table(g));
        let bitmap = canvas.resolve(metrics.width, metrics.height, layout, gamma.as_ref());

        crate::sync::write(&self.glyphs).insert(key, glyphcache::cache::CachedGlyph {
            bitmap: bitmap.clone(),
            width: metrics.width,
            height: metrics.height,
            xmin: metrics.xmin,
            ymin: metrics.ymin,
            bounds: metrics.bounds,
        });
        *crate::sync::write(&self.raster_scratch) = Some((canvas, glyph));
        Some(RasterizedGlyph { metrics, bitmap })
    }

    fn gamma_table(&self, g: f32) -> [u8; 256] {
        let key = g.to_bits();
        let cached = *crate::sync::read(&self.gamma);
        if let Some((k, lut)) = cached
            && k == key {
                return lut;
            }
        let lut = crate::daerizer::daecpu::platform::gamma_lut(g);
        *crate::sync::write(&self.gamma) = Some((key, lut));
        lut
    }

    fn glyph_key(&self, gid: u16, px: f32, axes: &crate::sync::Shared<crate::daecore::cache::AxisKey>, opts: &RasterOptions)
        -> glyphcache::cache::GlyphKey
    {
        glyphcache::cache::GlyphKey {
            gid,
            px_bits: px.to_bits(),
            layout: opts.layout.key(),
            gamma_bits: opts.gamma.map(f32::to_bits),
            transform_bits: opts.transform.map(|t| t.map(f32::to_bits)),
            hinting: opts.hinting as u8,
            embolden_bits: opts.embolden.map(f32::to_bits),
            oblique_bits: opts.oblique.map(f32::to_bits),
            stroke: opts.stroke.map(|s| {
                let (join, limit) = match s.join {
                    Join::Miter { limit } => (0u8, limit.to_bits()),
                    Join::Round => (1, 0),
                    Join::Bevel => (2, 0),
                };
                let cap = match s.cap {
                    Cap::Butt => 0u8,
                    Cap::Round => 1,
                    Cap::Square => 2,
                };
                (s.width.to_bits(), join, limit, cap)
            }),
            axes: crate::sync::Shared::clone(axes),
        }
    }

    fn hinted_outline(&self, gid: u16, px: f32, axes: &crate::daecore::cache::AxisKey, opts: &RasterOptions)
        -> Option<crate::daecore::daetype::hinting::HintedOutline>
    {
        if opts.hinting == HintMode::None { return None; }
        let upm = self.upm();
        let ppem = px.round() as u16;
        if ppem == 0 { return None; }

        let location = self.cache.compute_location_keyed(axes);
        let instanced;
        let cache: &FontCache = if !self.is_cff2 && location.iter().all(|&v| v == 0.0) {
            &self.cache
        } else {
            instanced = self.cache.instanced_font_cache_keyed(axes);
            &instanced
        };

        if opts.hinting != HintMode::AutoForce
            && let Some(glyf) = cache.table_map.get("glyf")
            && let Some(loca) = cache.loca_offsets()
            && let Some(out) = cache.hint_glyph_cached(glyf, &loca, gid, ppem, upm, opts.hinting)
        {
            return Some(out);
        }

        if !opts.hinting.may_autohint() { return None; }

        if opts.hinting != HintMode::AutoForce
            && let Some(out) = Self::cff_hinted_outline(cache, gid, ppem, upm)
        {
            return Some(out);
        }

        let mut pen = crate::daecore::daetype::hinting::auto::CollectPen::new();
        self.outline_glyph_keyed(gid, axes, &mut pen)?;
        let pts = pen.finish();
        if pts.is_empty() { return None; }
        if let Some(out) = cache.try_autohint(&pts, ppem) { return out; }
        self.ensure_autohinter(cache, axes);
        cache.try_autohint(&pts, ppem)?
    }

    fn cff_hinted_outline(cache: &FontCache, gid: u16, ppem: u16, upm: u16)
        -> Option<crate::daecore::daetype::hinting::HintedOutline>
    {
        use crate::daecore::daetype::hinting::{auto::CollectPen, HintedOutline, FLAG_ON_CURVE};

        let cff = cache.cff()?;
        let outlines = cache.cff_outlines()?;
        let mut pen = CollectPen::new();
        let hints =
            crate::daecore::daetype::outline::outline_cff_glyph_hinted(&outlines, cff, gid, &mut pen).ok()?;
        let pts = pen.finish();
        if pts.is_empty() { return None; }

        let y = crate::daecore::daetype::hinting::cff::apply(&pts, &hints, ppem, upm)?;
        let x = pts.x.iter()
            .map(|&v| crate::daecore::daetype::hinting::f26dot6::scale(v as i32, ppem, upm))
            .collect();
        let flags = pts.flags.iter()
            .map(|&f| if f & crate::daecore::daetype::hinting::auto::ON_CURVE != 0 { FLAG_ON_CURVE } else { 0 })
            .collect();
        Some(HintedOutline { x, y, flags, contour_ends: pts.contour_ends })
    }

    fn ensure_autohinter(&self, cache: &FontCache, axes: &crate::daecore::cache::AxisKey) {
        use crate::daecore::daetype::hinting::auto::AutoHinter;
        let upm = self.upm();
        if let Some(zones) = cache.autohint_blues() {
            cache.set_autohinter(AutoHinter::from_zones(zones, upm));
            return;
        }
        let mut resolve = |c: char| cache.glyph_id(c as u32);
        let mut outline_of = |gid: u16| {
            let mut pen = crate::daecore::daetype::hinting::auto::CollectPen::new();
            self.outline_glyph_keyed(gid, axes, &mut pen)?;
            let pts = pen.finish();
            (!pts.is_empty()).then_some(pts)
        };
        let zones = AutoHinter::compute_zones(upm, &mut resolve, &mut outline_of);
        cache.set_autohint_blues(zones.clone());
        cache.set_autohinter(AutoHinter::from_zones(zones, upm));
    }
}
