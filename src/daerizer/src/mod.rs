#![allow(unsafe_code)]

pub use crate::daecore::daemachine::daemath::{blend, gradient, matrix};
pub use crate::daecore::daetype::paint;
pub use crate::daecore::daetype::paint::colr;

pub use paint::{
    resolve_stops, Blend, ClipShape, DisplayList, Extend, Gradient, GradientKind, Op, Paint, PathId,
    Rgba, Stop, Stops,
};
pub use matrix::{concat, invert, Matrix, IDENTITY};

pub use blend::{blend, composite, Rgb};

pub use colr::lower;

use alloc::vec::Vec;
#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use crate::daecore::daetype::outline::{FillRule, Path};
use crate::daerizer::daecpu::math::{Geometry, Glyph};
use crate::daerizer::daecpu::rasterize::{metrics_raw, Raster};

pub mod daecpu;
pub mod daegpu;

pub mod draw;

pub mod canvas;
use crate::daerizer::canvas::{to_linear_rgb, Canvas};
use gradient::Ramp;

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedScene {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub left: i32,
    pub top: i32,
    pub skipped_ops: usize,
}

const MAX_SCENE_PIXELS: usize = 4096 * 4096;

const FLATTEN_RATIO: f32 = 128.0;

// MAX_NESTING bounds depth and MAX_SCENE_PIXELS bounds area, and neither bounds their product –
// 32 levels of a 4096x4096 scene is over ten gigabytes, every step a legal combination of the two.
// Charged on push and credited on pop, so repeated nesting costs what is held at once.
const MAX_SCENE_SCRATCH_BYTES: usize = 64 * 1024 * 1024;

const MAX_NESTING: usize = 32;

pub fn render(list: &DisplayList, px: f32, upem: f32) -> Option<RenderedScene> {
    render_in(list, px, upem, None)
}

pub(crate) fn render_in(
    list: &DisplayList,
    px: f32,
    upem: f32,
    viewport: Option<(usize, usize)>,
) -> Option<RenderedScene> {
    if !(px.is_finite() && px > 0.0 && upem.is_finite() && upem > 0.0) {
        return None;
    }
    let s = f64::from(px) / f64::from(upem);
    let to_device: [f64; 6] = [s, 0.0, 0.0, -s, 0.0, 0.0];

    let mut bounds: Option<Box2> = None;
    for op in list.ops() {
        if let Op::Fill { path, transform, .. } = op {
            let Some(p) = list.path(*path) else { continue };
            let Some(b) = p.bounds() else { continue };
            let t = concat(transform, &to_device);
            bounds = Some(match bounds {
                Some(acc) => acc.union(transform_box(&t, b)),
                None => transform_box(&t, b),
            });
        }
    }
    let (left, top, w, h) = match viewport {
        Some((vw, vh)) => (0i64, 0i64, vw, vh),
        None => {
            let b = bounds?;
            if !b.is_finite() {
                return None;
            }
            let left = (b.x0.floor() as i64).saturating_sub(1);
            let top = (b.y0.floor() as i64).saturating_sub(1);
            let right = (b.x1.ceil() as i64).saturating_add(1);
            let bottom = (b.y1.ceil() as i64).saturating_add(1);
            (
                left,
                top,
                usize::try_from(right.checked_sub(left)?).ok()?,
                usize::try_from(bottom.checked_sub(top)?).ok()?,
            )
        }
    };
    if w == 0 || h == 0 || w.checked_mul(h)? > MAX_SCENE_PIXELS {
        return None;
    }

    let mut stage = Stage {
        layers: alloc::vec![Canvas::new(w, h)],
        pending: Vec::new(),
        clips: Vec::new(),
        w,
        h,
        left,
        top,
        skipped: 0,
        scratch: 0,
        clip_admitted: Vec::new(),
        layer_admitted: Vec::new(),
    };
    stage.run(list, &to_device);

    while stage.layers.len() > 1 {
        stage.pop_layer();
    }

    Some(RenderedScene {
        width: w,
        height: h,
        rgba: stage.layers[0].to_rgba8(),
        left: i32::try_from(left).unwrap_or(i32::MIN),
        top: i32::try_from(top).unwrap_or(i32::MIN),
        skipped_ops: stage.skipped,
    })
}

struct Stage {
    layers: Vec<Canvas>,
    pending: Vec<(f32, Blend)>,
    clips: Vec<Vec<f32>>,
    w: usize,
    h: usize,
    left: i64,
    top: i64,
    skipped: usize,
    scratch: usize,
    // A refused push must still consume its matching pop. Without this a pop belonging to a refused
    // push removes the *enclosing* clip, and every fill after draws outside a clip that still
    // applies. A counter cannot do it – the budget interleaves refusals with admissions.
    clip_admitted: Vec<bool>,
    layer_admitted: Vec<bool>,
}

impl Stage {
    fn run(&mut self, list: &DisplayList, to_device: &[f64; 6]) {
        for op in list.ops() {
            match op {
                Op::Fill { path, paint, rule, transform } => {
                    let Some(p) = list.path(*path) else { continue };
                    let source = match paint {
                        Paint::Solid(c) => Source::Solid(*c),
                        Paint::Gradient(g) => {
                            Source::Ramp(Ramp::new(g, &concat(&g.transform, to_device)))
                        }
                    };
                    let t = concat(transform, to_device);
                    self.fill(p, &t, *rule, &source);
                }
                Op::PushClip { shapes } => self.push_clip(list, shapes, to_device),
                Op::PopClip => {
                    match self.clip_admitted.pop() {
                        Some(true) => {
                            self.clips.pop();
                            self.credit(self.clip_bytes());
                        }
                        _ => self.skipped += 1,
                    }
                }
                Op::PushLayer { opacity, blend } => self.push_layer(*opacity, *blend),
                Op::PopLayer => {
                    match self.layer_admitted.pop() {
                        Some(true) if self.layers.len() > 1 => self.pop_layer(),
                        _ => self.skipped += 1,
                    }
                }
            }
        }
    }

    fn push_clip(&mut self, list: &DisplayList, shapes: &[ClipShape], to_device: &[f64; 6]) {
        if self.clips.len() >= MAX_NESTING || !self.charge(self.clip_bytes()) {
            self.clip_admitted.push(false);
            self.skipped += 1;
            return;
        }
        self.clip_admitted.push(true);
        let mut union = alloc::vec![0.0f32; self.w * self.h];
        for shape in shapes {
            let Some(p) = list.path(shape.path) else { continue };
            let t = concat(&shape.transform, to_device);
            let Some(placed) = self.rasterize(p, &t, shape.rule) else { continue };
            placed.for_each(self.w, self.h, self.left, self.top, |i, c| {
                if c > union[i] {
                    union[i] = c;
                }
            });
        }
        if let Some(outer) = self.clips.last() {
            for (u, o) in union.iter_mut().zip(outer) {
                *u *= o;
            }
        }
        self.clips.push(union);
    }

    fn clip_bytes(&self) -> usize {
        self.w.saturating_mul(self.h).saturating_mul(core::mem::size_of::<f32>())
    }

    fn layer_bytes(&self) -> usize {
        self.w.saturating_mul(self.h).saturating_mul(core::mem::size_of::<crate::daerizer::canvas::Pixel>())
    }

    fn charge(&mut self, bytes: usize) -> bool {
        let after = self.scratch.saturating_add(bytes);
        if after > MAX_SCENE_SCRATCH_BYTES {
            return false;
        }
        self.scratch = after;
        true
    }

    fn credit(&mut self, bytes: usize) {
        self.scratch = self.scratch.saturating_sub(bytes);
    }

    fn push_layer(&mut self, opacity: f32, blend: Blend) {
        if self.layers.len() > MAX_NESTING || !self.charge(self.layer_bytes()) {
            self.layer_admitted.push(false);
            self.skipped += 1;
            return;
        }
        self.layer_admitted.push(true);
        self.layers.push(Canvas::new(self.w, self.h));
        self.pending.push((opacity.clamp(0.0, 1.0), blend));
    }

    fn pop_layer(&mut self) {
        let Some(layer) = self.layers.pop() else { return };
        self.credit(self.layer_bytes());
        let (opacity, blend) = self.pending.pop().unwrap_or((1.0, Blend::SrcOver));
        let Some(parent) = self.layers.last_mut() else { return };
        for (i, p) in layer.px.iter().enumerate() {
            let alpha = p.a * opacity;
            if alpha <= 0.0 && keeps_backdrop(blend) {
                continue;
            }
            parent.blend_at(i, p.straight(), alpha, blend);
        }
    }

    fn fill(&mut self, path: &Path, transform: &[f64; 6], rule: FillRule, source: &Source) {
        let Some(placed) = self.rasterize(path, transform, rule) else { return };
        let flat = match source {
            Source::Solid(c) => {
                let (rgb, alpha) = to_linear_rgb(*c);
                if alpha <= 0.0 {
                    return;
                }
                Some((rgb, alpha))
            }
            Source::Ramp(_) => None,
        };
        let (w, left, top) = (self.w, self.left, self.top);
        let clip = self.clips.last();
        let Some(canvas) = self.layers.last_mut() else { return };
        placed.for_each(w, self.h, left, top, |i, c| {
            let c = match clip {
                Some(mask) => c * mask[i],
                None => c,
            };
            if c <= 0.0 {
                return;
            }
            let (cx, cy) = ((i % w) as i64, (i / w) as i64);
            let (rgb, alpha) = match (flat, source) {
                (Some(pair), _) => pair,
                (None, Source::Ramp(ramp)) => {
                    let Some(color) = ramp.at((left + cx) as f64, (top + cy) as f64) else {
                        return;
                    };
                    let (rgb, alpha) = to_linear_rgb(color);
                    if alpha <= 0.0 {
                        return;
                    }
                    (rgb, alpha)
                }
                (None, Source::Solid(_)) => return,
            };
            canvas.blend_at(i, rgb, alpha * c, Blend::SrcOver);
        });
    }

    fn rasterize(&self, path: &Path, transform: &[f64; 6], rule: FillRule) -> Option<Placed> {
        // The path arrives already in device pixels, so the flattening tolerance has to be stated
        // in pixels too. `Geometry::new(px, upem)` yields `6 * upem / px` as its area bound, so a
        // ratio of 1/128 asks for about 0.05 square pixels — which is what the CPU rasterizer
        // effectively uses on text (its `6 * upm/px` in font units is 6 * px/upm once converted).
        // `Geometry::new(self.px, self.px)` asked for 6 instead, a hundred times looser, and curves
        // came out visibly faceted.
        let mut geometry = Geometry::new(FLATTEN_RATIO, 1.0);
        path.replay(Some(transform), &mut geometry);
        let mut glyph = Glyph::default();
        geometry.finalize(&mut glyph);

        let gb = glyph.bounds;
        if !(gb.xmin.is_finite()
            && gb.ymin.is_finite()
            && gb.width.is_finite()
            && gb.height.is_finite())
            || gb.width < 0.0
            || gb.height < 0.0
        {
            return None;
        }
        let (metrics, offset_x, offset_y) = metrics_raw(1.0, gb, 0.0, 0.0, 0.0);
        if metrics.width == 0 || metrics.height == 0 {
            return None;
        }
        if metrics.width.checked_mul(metrics.height)? > MAX_SCENE_PIXELS {
            return None;
        }
        // Its own raster, sized to its own box, rather than one accumulator for the scene: the
        // accumulator holds signed winding deltas, so two overlapping shapes sharing one would sum
        // their coverage. Right for the two contours of a single outline, wrong for two shapes.
        let mut raster = Raster::new(metrics.width, metrics.height);
        raster.draw(&glyph, 1.0, 1.0, offset_x, offset_y);

        // `Geometry::finalize` puts the path's *ymax* at row 0, which is the flip into raster order
        // the CPU rasterizer needs because it is handed y-up font outlines. Here the path has
        // already been flipped by `to_device`, so that second flip mirrors it: `Placed::for_each`
        // treats `y` as the top edge and walks down, while row 0 held the bottom. The rows are put
        // back the right way up rather than the transform being un-flipped, because `to_device` also
        // orients gradients and clips.
        let mut cov = raster.into_coverage(rule);
        let (w, h) = (metrics.width, metrics.height);
        for row in 0..h / 2 {
            let (a, b) = cov.split_at_mut((h - 1 - row) * w);
            a[row * w..row * w + w].swap_with_slice(&mut b[..w]);
        }

        Some(Placed {
            cov,
            w: metrics.width,
            h: metrics.height,
            x: i64::from(metrics.xmin),
            y: i64::from(metrics.ymin),
        })
    }
}

fn keeps_backdrop(mode: Blend) -> bool {
    !matches!(
        mode,
        Blend::Clear
            | Blend::Src
            | Blend::SrcIn
            | Blend::DestIn
            | Blend::SrcOut
            | Blend::DestOut
            | Blend::DestAtop
            | Blend::Xor
    )
}

struct Placed {
    cov: Vec<f32>,
    w: usize,
    h: usize,
    x: i64,
    y: i64,
}

impl Placed {
    fn for_each(&self, cw: usize, ch: usize, left: i64, top: i64, mut f: impl FnMut(usize, f32)) {
        let (dx, dy) = (self.x - left, self.y - top);
        for row in 0..self.h {
            let cy = dy + row as i64;
            if cy < 0 || cy as usize >= ch {
                continue;
            }
            for col in 0..self.w {
                let cx = dx + col as i64;
                if cx < 0 || cx as usize >= cw {
                    continue;
                }
                let c = self.cov[row * self.w + col];
                if c > 0.0 {
                    f(cy as usize * cw + cx as usize, c);
                }
            }
        }
    }
}

enum Source {
    Solid(crate::daerizer::paint::Rgba),
    Ramp(Ramp),
}

#[derive(Clone, Copy, Debug)]
struct Box2 {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Box2 {
    fn union(self, o: Box2) -> Box2 {
        Box2 {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    fn is_finite(&self) -> bool {
        self.x0.is_finite()
            && self.y0.is_finite()
            && self.x1.is_finite()
            && self.y1.is_finite()
            && self.x1 >= self.x0
            && self.y1 >= self.y0
    }
}

fn transform_box(t: &[f64; 6], (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Box2 {
    let pts = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    let mut out = Box2 { x0: f64::MAX, y0: f64::MAX, x1: f64::MIN, y1: f64::MIN };
    for (x, y) in pts {
        let tx = t[0] * x + t[2] * y + t[4];
        let ty = t[1] * x + t[3] * y + t[5];
        out.x0 = out.x0.min(tx);
        out.y0 = out.y0.min(ty);
        out.x1 = out.x1.max(tx);
        out.y1 = out.y1.max(ty);
    }
    out
}
