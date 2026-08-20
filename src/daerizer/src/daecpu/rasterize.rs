use alloc::vec::Vec;
use crate::daecore::daetype::outline::FillRule;
use super::math::{Glyph, Line, OutlineBounds};
use super::platform::{abs, as_i32, ceil, clamp, copysign, f32x4, floor, fract, is_negative};
use crate::daecore::daemachine::subpixel::SubpixelLayout;

#[derive(Clone, PartialEq, Debug)]
pub struct Metrics {
    pub xmin: i32,
    pub ymin: i32,
    pub width: usize,
    pub height: usize,
    pub advance_width: f32,
    pub advance_height: f32,
    pub bounds: OutlineBounds,
}

pub fn metrics_raw(
    scale: f32,
    bounds: OutlineBounds,
    advance_width: f32,
    advance_height: f32,
    offset: f32,
) -> (Metrics, f32, f32) {
    let bounds = bounds.scale(scale);
    let mut offset_x = fract(bounds.xmin + offset);
    let mut offset_y = fract(1.0 - fract(bounds.height) - fract(bounds.ymin));
    if is_negative(offset_x) {
        offset_x += 1.0;
    }
    if is_negative(offset_y) {
        offset_y += 1.0;
    }
    let metrics = Metrics {
        xmin: as_i32(floor(bounds.xmin)),
        ymin: as_i32(floor(bounds.ymin)),
        width: as_i32(ceil(bounds.width + offset_x)) as usize,
        height: as_i32(ceil(bounds.height + offset_y)) as usize,
        advance_width: scale * advance_width,
        advance_height: scale * advance_height,
        bounds,
    };
    (metrics, offset_x, offset_y)
}

pub struct Raster {
    w: usize,
    h: usize,
    a: Vec<f32>,
}

fn area(w: usize, h: usize) -> Option<usize> {
    w.checked_mul(h)?.checked_add(3)
}

impl Raster {
    pub fn new(w: usize, h: usize) -> Raster {
        let Some(need) = area(w, h) else {
            return Raster { w: 0, h: 0, a: alloc::vec![0.0; 3] };
        };
        Raster { w, h, a: alloc::vec![0.0; need] }
    }

    pub fn reset(&mut self, w: usize, h: usize) {
        // The direction is the opposite of the usual advice: `vec![0.0; n]` routes to
        // `alloc_zeroed` and gets pages the kernel already zeroed, so above 64 KB of `f32` a
        // fresh allocation beats writing the zeros in place – 344.8ns against 808.8ns at 137x171.
        const HAND_ZEROED_LIMIT: usize = 16_384;

        let Some(need) = area(w, h) else {
            self.w = 0;
            self.h = 0;
            self.a.clear();
            self.a.resize(3, 0.0);
            return;
        };
        self.w = w;
        self.h = h;
        if need >= HAND_ZEROED_LIMIT {
            self.a = alloc::vec![0.0; need];
        } else {
            self.a.clear();
            self.a.resize(need, 0.0);
        }
    }

    pub fn draw(
        &mut self,
        glyph: &Glyph,
        scale_x: f32,
        scale_y: f32,
        offset_x: f32,
        offset_y: f32,
    ) {
        if self.w == 0 || self.h == 0 {
            return;
        }
        let params = f32x4::new(1.0 / scale_x, 1.0 / scale_y, scale_x, scale_y);
        let scale = f32x4::new(scale_x, scale_y, scale_x, scale_y);
        let offset = f32x4::new(offset_x, offset_y, offset_x, offset_y);
        for seg in &glyph.v_segments {
            let line = Line::new(seg[0], seg[1]);
            self.v_line(&line, line.coords * scale + offset);
        }
        for seg in &glyph.m_segments {
            let line = Line::new(seg[0], seg[1]);
            self.m_line(&line, line.coords * scale + offset, line.params * params);
        }
    }

    #[inline(always)]
    fn add(&mut self, index: usize, height: f32, mid_x: f32) {
        let m = height * mid_x;
        let Some(pair) = index.checked_add(2).and_then(|end| self.a.get_mut(index..end)) else { return };
        pair[0] += height - m;
        pair[1] += m;
    }

    #[inline(always)]
    fn v_line(&mut self, line: &Line, coords: f32x4) {
        let (x0, y0, _, y1) = coords.copied();
        let temp = coords.sub_integer(line.nudge).trunc();
        let (start_x, start_y, end_x, end_y) = temp.copied();
        let (_, mut target_y, _, _) = (temp + line.adjustment).copied();
        let sy = copysign(1f32, y1 - y0);
        let mut y_prev = y0;
        let mut index = as_i32(start_x + start_y * self.w as f32);
        let index_y_inc = as_i32(copysign(self.w as f32, sy));
        let mut dist = as_i32(abs(start_y - end_y));
        let mid_x = fract(x0);
        while dist > 0 {
            dist -= 1;
            self.add(index as usize, y_prev - target_y, mid_x);
            index += index_y_inc;
            y_prev = target_y;
            target_y += sy;
        }
        self.add(
            as_i32(end_x + end_y * self.w as f32) as usize,
            y_prev - y1,
            mid_x,
        );
    }

    #[inline(always)]
    fn m_line(&mut self, line: &Line, coords: f32x4, params: f32x4) {
        let (x0, y0, x1, y1) = coords.copied();
        let temp = coords.sub_integer(line.nudge).trunc();
        let (start_x, start_y, end_x, end_y) = temp.copied();
        let (tdx, tdy, dx, dy) = params.copied();
        let (mut target_x, mut target_y, _, _) = (temp + line.adjustment).copied();
        let sx = copysign(1f32, tdx);
        let sy = copysign(1f32, tdy);
        let mut tmx = tdx * (target_x - x0);
        let mut tmy = tdy * (target_y - y0);
        let tdx = abs(tdx);
        let tdy = abs(tdy);
        let mut x_prev = x0;
        let mut y_prev = y0;
        let mut index = as_i32(start_x + start_y * self.w as f32);
        let index_x_inc = as_i32(sx);
        let index_y_inc = as_i32(copysign(self.w as f32, sy));
        let mut dist = as_i32(abs(start_x - end_x) + abs(start_y - end_y));
        while dist > 0 {
            dist -= 1;
            let prev_index = index;
            let y_next: f32;
            let x_next: f32;
            if tmx < tmy {
                y_next = tmx * dy + y0;
                x_next = target_x;
                tmx += tdx;
                target_x += sx;
                index += index_x_inc;
            } else {
                y_next = target_y;
                x_next = tmy * dx + x0;
                tmy += tdy;
                target_y += sy;
                index += index_y_inc;
            }
            self.add(
                prev_index as usize,
                y_prev - y_next,
                fract((x_prev + x_next) / 2.0),
            );
            x_prev = x_next;
            y_prev = y_next;
        }
        self.add(
            as_i32(end_x + end_y * self.w as f32) as usize,
            y_prev - y1,
            fract((x_prev + x1) / 2.0),
        );
    }

    #[inline(always)]
    pub fn get_bitmap(&self) -> Vec<u8> {
        super::platform::get_bitmap(&self.a, self.w * self.h)
    }

    pub fn into_coverage(mut self, rule: FillRule) -> Vec<f32> {
        let n = self.w * self.h;
        match rule {
            FillRule::NonZero => super::platform::coverage_in_place(&mut self.a, n),
            FillRule::EvenOdd => super::platform::coverage_even_odd_in_place(&mut self.a, n),
        }
        self.a.truncate(n);
        self.a
    }

    pub fn resolve(
        &mut self,
        out_w: usize,
        out_h: usize,
        layout: &SubpixelLayout,
        gamma: Option<&[u8; 256]>,
    ) -> Vec<u8> {
        if layout.is_grayscale() {
            let mut out = self.get_bitmap();
            if let Some(lut) = gamma {
                for level in &mut out { *level = lut[*level as usize]; }
            }
            return out;
        }
        super::platform::coverage_in_place(&mut self.a, self.w * self.h);
        let coverage = &self.a;
        let channels = layout.channels() as usize;
        let mut out = vec![0u8; out_w * out_h * channels];
        let (tx, ty) = layout.taps();
        let (taps_x, taps_y) = (tx as usize, ty as usize);
        let (sx, sy) = layout.oversample();
        let (ox, oy) = (sx as usize, sy as usize);
        let (orx, ory) = layout.origin();
        let (origin_x, origin_y) = (orx as isize, ory as isize);
        let w = layout.weight_rows();

        for py in 0..out_h {
            let base_y = (py * oy) as isize + origin_y;
            let ty0 = (-base_y).max(0) as usize;
            let ty1 = ((self.h as isize - base_y).max(0) as usize).min(taps_y);
            for px in 0..out_w {
                let base_x = (px * ox) as isize + origin_x;
                let tx0 = (-base_x).max(0) as usize;
                let tx1 = ((self.w as isize - base_x).max(0) as usize).min(taps_x);
                let mut sum = [0.0f32; 3];
                if tx1 > tx0 {
                    // Off-canvas taps are skipped, never zero-filled: a weight of infinity times
                    // a stored `0.0` is NaN, which `as u8` turns into a blank glyph cached by key.
                    let lo = (base_x + tx0 as isize) as usize;
                    let span = tx1 - tx0;
                    for ty in ty0..ty1 {
                        let row = (base_y + ty as isize) as usize * self.w;
                        let Some(cov) = coverage.get(row + lo..row + lo + span) else { continue };
                        let wbase = ty * taps_x + tx0;
                        if channels == 3
                            && let Some(w0) = w[0].get(wbase..wbase + span)
                            && let Some(w1) = w[1].get(wbase..wbase + span)
                            && let Some(w2) = w[2].get(wbase..wbase + span)
                        {
                            let (d0, d1, d2) = super::simd::tap_run3(cov, w0, w1, w2);
                            sum[0] += d0;
                            sum[1] += d1;
                            sum[2] += d2;
                        } else {
                            for (s, wc) in sum.iter_mut().zip(w).take(channels) {
                                let Some(row_w) = wc.get(wbase..wbase + span) else { continue };
                                for (cv, wv) in cov.iter().zip(row_w) {
                                    *s += wv * cv;
                                }
                            }
                        }
                    }
                }
                let slot = (py * out_w + px) * channels;
                for (c, s) in sum.iter().take(channels).enumerate() {
                    let level = clamp(*s * 255.9, 0.0, 255.0) as u8;
                    if let Some(byte) = out.get_mut(slot + c) {
                        *byte = match gamma {
                            Some(lut) => lut[level as usize],
                            None => level,
                        };
                    }
                }
            }
        }
        out
    }
}
