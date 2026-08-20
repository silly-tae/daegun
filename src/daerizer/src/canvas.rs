use alloc::vec::Vec;
use crate::daecore::daemachine::daemath::blend::{composite, Rgb};
use crate::daecore::daetype::paint::{Blend, Rgba};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(crate) struct Pixel {
    pub(crate) rgb: Rgb,
    pub(crate) a: f32,
}

impl Pixel {
    pub(crate) fn straight(self) -> Rgb {
        if self.a <= 0.0 {
            return [0.0, 0.0, 0.0];
        }
        [self.rgb[0] / self.a, self.rgb[1] / self.a, self.rgb[2] / self.a]
    }
}

// Premultiplied and f32, neither by preference. `composite` returns premultiplied while taking
// its inputs straight, so storing straight would divide its result back out at every pixel; and
// quantising per step – coverage, then paint alpha – bands the dark end where small glyphs live.
pub struct Canvas {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) px: Vec<Pixel>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: alloc::vec![Pixel::default(); w * h] }
    }

    pub fn blend_at(&mut self, i: usize, src: Rgb, alpha: f32, mode: Blend) {
        // Out of range is skipped, not a panic: callers derive `i` from a rasterized box, and a
        // degenerate transform can put that box a pixel outside the canvas it was measured for.
        let Some(dst) = self.px.get_mut(i) else { return };
        if mode == Blend::SrcOver {
            let inv = 1.0 - alpha;
            for (d, s) in dst.rgb.iter_mut().zip(src) {
                *d = s * alpha + *d * inv;
            }
            dst.a = alpha + dst.a * inv;
            return;
        }
        let (rgb, a) = composite(mode, src, alpha, dst.straight(), dst.a);
        *dst = Pixel { rgb, a };
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = alloc::vec![0u8; self.w * self.h * 4];
        for (p, slot) in self.px.iter().zip(out.chunks_exact_mut(4)) {
            let s = p.straight();
            slot[0] = to_u8(s[0]);
            slot[1] = to_u8(s[1]);
            slot[2] = to_u8(s[2]);
            slot[3] = to_u8(p.a);
        }
        out
    }
}

// `+ 0.5` and a clamp rather than a bare cast: `as u8` truncates, so 254.9 would come back 254
// and a fully opaque fill would never quite reach 255.
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

pub(crate) fn to_linear_rgb(c: Rgba) -> (Rgb, f32) {
    (
        [f32::from(c.r) / 255.0, f32::from(c.g) / 255.0, f32::from(c.b) / 255.0],
        f32::from(c.a) / 255.0,
    )
}
