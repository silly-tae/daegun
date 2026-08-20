use super::*;
use crate::daerizer::daegpu::{GpuBatch, GpuGlyphError};
use crate::daerizer::draw::{route, DeviceProfile, Policy, Refusal, Rendered, Request};

pub struct DrawTarget<'a> {
    pub batch: &'a mut GpuBatch,
    pub device: Option<&'a DeviceProfile>,
    pub policy: Policy,
}

impl<'a> DrawTarget<'a> {
    pub fn cpu_only(batch: &'a mut GpuBatch) -> DrawTarget<'a> {
        DrawTarget { batch, device: None, policy: Policy::default() }
    }

    pub fn new(batch: &'a mut GpuBatch, device: &'a DeviceProfile) -> DrawTarget<'a> {
        DrawTarget { batch, device: Some(device), policy: Policy::default() }
    }

    pub fn with_policy(mut self, policy: Policy) -> DrawTarget<'a> {
        self.policy = policy;
        self
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum DrawnGlyph {
    Nothing,
    Cpu(RasterizedGlyph),
    Gpu(crate::daerizer::daegpu::GlyphSlot),
    GpuColor(Vec<ColorSlot>),
    Scene(crate::daerizer::RenderedScene),
    Reference(RasterizedGlyph),
    BatchFull,
    Refused(Refusal),
}

impl DrawnGlyph {
    pub fn bitmap(&self) -> Option<&RasterizedGlyph> {
        match self {
            DrawnGlyph::Cpu(g) | DrawnGlyph::Reference(g) => Some(g),
            _ => None,
        }
    }

    pub fn is_ok(&self) -> bool {
        !matches!(self, DrawnGlyph::Refused(_) | DrawnGlyph::BatchFull)
    }
}

const MAX_REFERENCE_PIXELS: usize = 4096 * 4096;

fn request_for(px: f32, opts: &RasterOptions) -> Request {
    Request {
        ppem: px,
        hinted: opts.hinting != HintMode::None,
        stroked: opts.stroke.is_some(),
        gamma: opts.gamma.is_some(),
        emboldened: opts.embolden.is_some(),
        obliqued: opts.oblique.is_some(),
    }
}

impl Font {
    pub fn draw_glyph(
        &self,
        target: &mut DrawTarget<'_>,
        gid: u16,
        px: f32,
        axes: &[(&str, f64)],
        opts: &RasterOptions,
        palette: Option<u16>,
    ) -> DrawnGlyph {
        let request = request_for(px, opts);

        if let Some(palette_index) = palette {
            let attempt = self.gpu_color_glyph(target.batch, gid, axes, palette_index);
            // A glyph with no colour description answers `NoOutline` here, meaning "not a colour
            // glyph" rather than "not a glyph" – so it falls through instead of returning, which is
            // what lets a caller ask for colour on every glyph in a run.
            if !matches!(attempt, Err(GpuGlyphError::NoOutline)) {
                let decision =
                    route(attempt.as_ref().map(|_| ()).map_err(|e| *e), &request, target.device, &target.policy);
                return match decision {
                    Rendered::Gpu => match attempt {
                        Ok(slots) => DrawnGlyph::GpuColor(slots),
                        Err(_) => DrawnGlyph::Refused(Refusal::NonFinite),
                    },
                    Rendered::Scene | Rendered::Cpu | Rendered::Reference => {
                        match self.render_colr_glyph(gid, px, axes, palette_index) {
                            Some(scene) => DrawnGlyph::Scene(scene),
                            None => DrawnGlyph::Nothing,
                        }
                    }
                    Rendered::Nothing => DrawnGlyph::Nothing,
                    Rendered::FlushAndRetry => DrawnGlyph::BatchFull,
                    Rendered::Refused(why) => DrawnGlyph::Refused(why),
                };
            }
        }

        let attempt = self.gpu_glyph(target.batch, gid, axes);
        let decision =
            route(attempt.as_ref().map(|_| ()).map_err(|e| *e), &request, target.device, &target.policy);
        match decision {
            Rendered::Gpu => match attempt {
                Ok(slot) => DrawnGlyph::Gpu(slot),
                Err(_) => DrawnGlyph::Refused(Refusal::NonFinite),
            },
            Rendered::Cpu => match self.rasterize_glyph_with(gid, px, axes, opts) {
                Some(g) => DrawnGlyph::Cpu(g),
                None => DrawnGlyph::Nothing,
            },
            Rendered::Reference => match attempt {
                Ok(slot) => match self.reference_glyph(target.batch, &slot, px, opts) {
                    Some(g) => DrawnGlyph::Reference(g),
                    None => DrawnGlyph::Nothing,
                },
                Err(_) => DrawnGlyph::Refused(Refusal::NonFinite),
            },
            Rendered::Scene => match self.render_colr_glyph(gid, px, axes, palette.unwrap_or(0)) {
                Some(scene) => DrawnGlyph::Scene(scene),
                None => DrawnGlyph::Nothing,
            },
            Rendered::Nothing => DrawnGlyph::Nothing,
            Rendered::FlushAndRetry => DrawnGlyph::BatchFull,
            Rendered::Refused(why) => DrawnGlyph::Refused(why),
        }
    }

    fn reference_glyph(
        &self,
        batch: &GpuBatch,
        slot: &crate::daerizer::daegpu::GlyphSlot,
        px: f32,
        opts: &RasterOptions,
    ) -> Option<RasterizedGlyph> {
        use crate::daerizer::daegpu::{eval, SubpixelParams};
        if !(px.is_finite() && px > 0.0) {
            return None;
        }
        let params = SubpixelParams::from_layout(&opts.layout);
        let channels = opts.layout.channels() as usize;

        let x0 = (slot.box_min[0] * px).floor() as i32 - 1;
        let x1 = (slot.box_max[0] * px).ceil() as i32 + 1;
        let y0 = (-slot.box_max[1] * px).floor() as i32 - 1;
        let y1 = (-slot.box_min[1] * px).ceil() as i32 + 1;
        let w = usize::try_from(x1.checked_sub(x0)?).ok()?;
        let h = usize::try_from(y1.checked_sub(y0)?).ok()?;
        if w == 0 || h == 0 || w.checked_mul(h)? > MAX_REFERENCE_PIXELS {
            return None;
        }

        let mut bitmap = alloc::vec![0u8; w.checked_mul(h)?.checked_mul(channels)?];
        for row in 0..h {
            for col in 0..w {
                let em = [
                    (x0 as f32 + col as f32 + 0.5) / px,
                    -(y0 as f32 + row as f32 + 0.5) / px,
                ];
                let cov = eval::coverage_channels(batch, slot, em, [px, px], &params);
                let at = (row * w + col) * channels;
                let Some(slot_px) = bitmap.get_mut(at..at + channels) else { continue };
                for (byte, c) in slot_px.iter_mut().zip(cov) {
                    *byte = (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }

        Some(RasterizedGlyph {
            metrics: crate::daerizer::daecpu::rasterize::Metrics {
                xmin: x0,
                ymin: -y1,
                width: w,
                height: h,
                advance_width: 0.0,
                advance_height: 0.0,
                bounds: Default::default(),
            },
            bitmap,
        })
    }
}
