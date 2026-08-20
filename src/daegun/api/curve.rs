use super::*;

impl Font {
    pub fn gpu_glyph(&self, batch: &mut GpuBatch, gid: u16, axes: &[(&str, f64)])
        -> Result<GlyphSlot, GpuGlyphError>
    {
        let key = crate::daerizer::daegpu::GpuGlyphKey {
            gid,
            axes: self.cache.intern_axes(&crate::cache::canonical_axes(axes)),
            shape: 0,
        };

        if let Some(slot) = batch.slot_for(&key) {
            return Ok(slot);
        }

        // The lookup takes the guard, copies out, and drops it before anything else runs. Held
        // across the miss path it is a borrow across a call that borrows again – an already-borrowed
        // panic by default, a deadlock under `threading`. A guard in a `match` scrutinee lives for
        // the whole body, which is what makes this easy to write by accident.
        let hit = crate::sync::write(&self.gpu_curves)
            .get(&key)
            .map(crate::sync::Shared::clone);

        let built = match hit {
            Some(built) => built,
            None => {
                let mut pen = crate::daerizer::daegpu::collector(self.upm() as f32);
                match self.prewarmed_outline(gid, &key.axes) {
                    Some(path) => path.replay(None, &mut pen),
                    None => {
                        if self.outline_glyph_keyed(gid, &key.axes, &mut pen).is_none() {
                            return Err(GpuGlyphError::NoOutline);
                        }
                    }
                }
                let mut curves = match pen.finish() {
                    Ok(c) => c,
                    Err(Some(crate::daerizer::daegpu::Reject::TooComplex)) => {
                        return Err(GpuGlyphError::TooComplex)
                    }
                    Err(Some(crate::daerizer::daegpu::Reject::NonFinite)) => {
                        return Err(GpuGlyphError::NonFinite)
                    }
                    Err(None) => return Err(GpuGlyphError::NoOutline),
                };
                let banded = crate::daerizer::daegpu::GpuBatch::build_glyph(&mut curves)
                    .ok_or(GpuGlyphError::TooComplex)?;
                let built = crate::sync::Shared::new(crate::daerizer::daegpu::BuiltGlyph { curves, banded });
                crate::sync::write(&self.gpu_curves)
                    .insert(key.clone(), crate::sync::Shared::clone(&built));
                built
            }
        };

        let slot = batch
            .append_prebuilt(&built.curves, &built.banded)
            .ok_or(GpuGlyphError::BatchFull)?;
        batch.remember(key, slot);
        Ok(slot)
    }

    pub fn gpu_color_glyph(
        &self,
        batch: &mut crate::daerizer::daegpu::GpuBatch,
        gid: u16,
        axes: &[(&str, f64)],
        palette_index: u16,
    ) -> Result<alloc::vec::Vec<ColorSlot>, GpuGlyphError> {
        let paint = self.colr_v1_paint(gid, axes, palette_index).ok_or(GpuGlyphError::NoOutline)?;
        let mut scene = crate::daerizer::DisplayList::default();
        let axes_shared = self.cache.intern_axes(&crate::cache::canonical_axes(axes));
        let mut outline = |g: u16| {
            let mut p = crate::daecore::daetype::outline::Path::default();
            self.outline_glyph_keyed(g, &axes_shared, &mut p)?;
            (!p.is_empty()).then_some(p)
        };
        crate::daerizer::lower(
            &paint,
            crate::daerizer::IDENTITY,
            &mut outline,
            crate::daerizer::Rgba::default(),
            &mut scene,
        );

        let mut out = alloc::vec::Vec::new();
        for (i, op) in scene.ops().iter().enumerate() {
            let crate::daerizer::Op::Fill { path, paint, transform, .. } = op else {
                return Err(GpuGlyphError::NotFlatColor);
            };
            let crate::daerizer::Paint::Solid(color) = paint else {
                return Err(GpuGlyphError::NotFlatColor);
            };
            let p = scene.path(*path).ok_or(GpuGlyphError::NoOutline)?;

            let key = crate::daerizer::daegpu::GpuGlyphKey {
                gid,
                axes: crate::sync::Shared::clone(&axes_shared),
                shape: u32::try_from(i).map_err(|_| GpuGlyphError::BatchFull)?,
            };
            let slot = self.gpu_upload(batch, key, |pen| p.replay(Some(transform), pen))?;
            out.push(ColorSlot {
                slot,
                tint: [
                    f32::from(color.r) / 255.0,
                    f32::from(color.g) / 255.0,
                    f32::from(color.b) / 255.0,
                    f32::from(color.a) / 255.0,
                ],
            });
        }
        if out.is_empty() {
            return Err(GpuGlyphError::NoOutline);
        }
        Ok(out)
    }

    fn gpu_upload(
        &self,
        batch: &mut crate::daerizer::daegpu::GpuBatch,
        key: crate::daerizer::daegpu::GpuGlyphKey,
        draw: impl FnOnce(&mut dyn crate::daecore::daetype::outline::OutlinePen),
    ) -> Result<GlyphSlot, GpuGlyphError> {
        if let Some(slot) = batch.slot_for(&key) {
            return Ok(slot);
        }
        let hit = crate::sync::write(&self.gpu_curves)
            .get(&key)
            .map(crate::sync::Shared::clone);

        let built = match hit {
            Some(built) => built,
            None => {
                let mut pen = crate::daerizer::daegpu::collector(f32::from(self.upm()));
                draw(&mut pen);
                let mut curves = match pen.finish() {
                    Ok(c) => c,
                    Err(Some(crate::daerizer::daegpu::Reject::TooComplex)) => {
                        return Err(GpuGlyphError::TooComplex)
                    }
                    Err(Some(crate::daerizer::daegpu::Reject::NonFinite)) => {
                        return Err(GpuGlyphError::NonFinite)
                    }
                    Err(None) => return Err(GpuGlyphError::NoOutline),
                };
                let banded = crate::daerizer::daegpu::GpuBatch::build_glyph(&mut curves)
                    .ok_or(GpuGlyphError::TooComplex)?;
                let built = crate::sync::Shared::new(crate::daerizer::daegpu::BuiltGlyph { curves, banded });
                crate::sync::write(&self.gpu_curves)
                    .insert(key.clone(), crate::sync::Shared::clone(&built));
                built
            }
        };
        let slot = batch
            .append_prebuilt(&built.curves, &built.banded)
            .ok_or(GpuGlyphError::BatchFull)?;
        batch.remember(key, slot);
        Ok(slot)
    }
}
