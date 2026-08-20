#![allow(non_camel_case_types)]

use alloc::vec::Vec;
use core::ops::{Add, Mul};
#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct f32x4 {
    x0: f32,
    x1: f32,
    x2: f32,
    x3: f32,
}

impl f32x4 {
    #[inline(always)]
    pub(crate) const fn new(x0: f32, x1: f32, x2: f32, x3: f32) -> Self {
        f32x4 { x0, x1, x2, x3 }
    }

    #[inline(always)]
    pub(crate) fn new_u32(x0: u32, x1: u32, x2: u32, x3: u32) -> Self {
        Self::new(
            f32::from_bits(x0),
            f32::from_bits(x1),
            f32::from_bits(x2),
            f32::from_bits(x3),
        )
    }

    #[inline(always)]
    // `saturating_sub` because `(+0.0).to_bits()` is 0 and the nudge is 1: release wraps that to
    // `0xFFFF_FFFF`, which is a NaN, and the raster silently follows it.
    pub(crate) fn sub_integer(&self, other: f32x4) -> f32x4 {
        Self::new(
            f32::from_bits(self.x0.to_bits().saturating_sub(other.x0.to_bits())),
            f32::from_bits(self.x1.to_bits().saturating_sub(other.x1.to_bits())),
            f32::from_bits(self.x2.to_bits().saturating_sub(other.x2.to_bits())),
            f32::from_bits(self.x3.to_bits().saturating_sub(other.x3.to_bits())),
        )
    }

    #[inline(always)]
    pub(crate) const fn copied(self) -> (f32, f32, f32, f32) {
        (self.x0, self.x1, self.x2, self.x3)
    }

    #[inline(always)]
    pub(crate) fn trunc(self) -> Self {
        Self::new(
            trunc(self.x0),
            trunc(self.x1),
            trunc(self.x2),
            trunc(self.x3),
        )
    }
}

impl Add for f32x4 {
    type Output = f32x4;
    #[inline(always)]
    fn add(self, other: f32x4) -> f32x4 {
        Self::new(
            self.x0 + other.x0,
            self.x1 + other.x1,
            self.x2 + other.x2,
            self.x3 + other.x3,
        )
    }
}

impl Mul for f32x4 {
    type Output = f32x4;
    #[inline(always)]
    fn mul(self, other: f32x4) -> f32x4 {
        Self::new(
            self.x0 * other.x0,
            self.x1 * other.x1,
            self.x2 * other.x2,
            self.x3 * other.x3,
        )
    }
}

#[inline(always)]
pub(crate) fn as_i32(value: f32) -> i32 {
    value as i32
}

#[inline(always)]
pub(crate) fn ceil(x: f32) -> f32 {
    x.ceil()
}

#[inline(always)]
pub(crate) fn floor(x: f32) -> f32 {
    x.floor()
}

#[inline(always)]
pub(crate) fn fract(value: f32) -> f32 {
    value - trunc(value)
}

pub(crate) fn get_bitmap(a: &[f32], length: usize) -> Vec<u8> {
    super::simd::get_bitmap(a, length)
}

pub(crate) fn coverage_in_place(a: &mut [f32], length: usize) {
    super::simd::coverage_in_place(a, length)
}

pub(crate) fn coverage_even_odd_in_place(a: &mut [f32], length: usize) {
    let mut height = 0.0;
    for slot in a.iter_mut().take(length) {
        height += *slot;
        let folded = abs(height) % 2.0;
        *slot = clamp(if folded > 1.0 { 2.0 - folded } else { folded }, 0.0, 1.0);
    }
}

pub fn gamma_lut(gamma: f32) -> [u8; 256] {
    let mut lut = [0u8; 256];
    let inv = 1.0 / gamma;
    for (level, slot) in lut.iter_mut().enumerate() {
        let linear = level as f32 / 255.0;
        // The clamp is load-bearing, not decorative: `powf_approx(1.0, 1.0)` is 1.000061, which
        // is what a fitted polynomial does at the end of its interval, and 256 does not fit a u8.
        *slot = clamp(powf(linear, inv) * 255.0 + 0.5, 0.0, 255.0) as u8;
    }
    lut
}

#[inline(always)]
pub(crate) fn powf(x: f32, y: f32) -> f32 {
    #[cfg(feature = "std")]
    { x.powf(y) }
    #[cfg(not(feature = "std"))]
    { powf_approx(x, y) }
}

#[cfg(any(not(feature = "std"), test))]
// Compiled under `test` as well so the std build can grade it against `f32::powf`. Without that
// a `no_std` caller's whole gamma curve is an unchecked polynomial pair.
pub(crate) fn powf_approx(x: f32, y: f32) -> f32 {
    if x <= 0.0 { return 0.0; }
    if y == 0.0 { return 1.0; }
    exp2(y * log2(x))
}

#[cfg(any(not(feature = "std"), test))]
#[inline(always)]
fn log2(x: f32) -> f32 {
    use core::f32::consts::LOG2_E;
    let bits = x.to_bits();
    let exponent = ((bits >> 23) & 0xFF) as i32 - 127;
    let m = f32::from_bits((bits & 0x007F_FFFF) | 0x3F80_0000);
    let ln_m = -1.7417939 + (2.8212026 + (-1.4699568 + (0.44717955 - 0.05657085 * m) * m) * m) * m;
    ln_m * LOG2_E + exponent as f32
}

#[cfg(any(not(feature = "std"), test))]
#[inline(always)]
fn exp2(x: f32) -> f32 {
    if x < -126.0 { return 0.0; }
    if x > 127.0 { return f32::INFINITY; }
    let whole = floor(x) as i32;
    let frac = x - whole as f32;
    use core::f32::consts::LN_2;
    let p = 1.0 + (LN_2 + (0.2402265 + (0.0555041 + (0.0096181 + 0.0013333 * frac) * frac) * frac) * frac) * frac;
    let scale = f32::from_bits((((whole + 127) as u32) & 0xFF) << 23);
    p * scale
}

#[inline(always)]
pub(crate) fn trunc(x: f32) -> f32 {
    x.trunc()
}

#[inline(always)]
pub(crate) fn abs(value: f32) -> f32 {
    value.abs()
}

#[inline(always)]
pub(crate) fn is_negative(value: f32) -> bool {
    value.to_bits() >= 0x80000000
}

#[inline(always)]
pub(crate) fn copysign(value: f32, sign: f32) -> f32 {
    f32::from_bits((value.to_bits() & 0x7fffffff) | (sign.to_bits() & 0x80000000))
}

#[inline(always)]
pub(crate) fn clamp(value: f32, min: f32, max: f32) -> f32 {
    let mut x = value;
    if x < min {
        x = min;
    }
    if x > max {
        x = max;
    }
    x
}

#[cfg(test)]
mod approximation {
    use super::{gamma_lut, powf_approx};

    #[test]
    fn the_gamma_table_is_within_one_level_of_std() {
        for gamma in [1.0f32, 1.2, 1.4, 1.8, 2.2, 2.4, 3.0] {
            let inv = 1.0 / gamma;
            let mut worst = 0i32;
            for level in 0..=255u32 {
                let linear = level as f32 / 255.0;
                let approx =
                    super::clamp(powf_approx(linear, inv) * 255.0 + 0.5, 0.0, 255.0) as u8;
                let exact =
                    super::clamp(linear.powf(inv) * 255.0 + 0.5, 0.0, 255.0) as u8;
                worst = worst.max(i32::from(approx).abs_diff(i32::from(exact)) as i32);
            }
            assert!(worst <= 1, "gamma {gamma}: the table is {worst} levels from std's");
        }
        let table = gamma_lut(2.2);
        assert_eq!(table[0], 0, "black did not stay black");
        assert_eq!(table[255], 255, "white did not stay white");
    }

    #[test]
    fn the_relative_error_stays_under_four_parts_in_ten_thousand() {
        let mut worst = 0.0f32;
        let mut at = (0.0f32, 0.0f32);
        for i in 1..=4000u32 {
            let x = i as f32 / 4000.0;
            for j in 0..=60u32 {
                let y = 0.1 + j as f32 * 0.05;
                let exact = x.powf(y);
                if exact > 0.0 && exact.is_finite() {
                    let r = ((powf_approx(x, y) - exact) / exact).abs();
                    if r > worst {
                        worst = r;
                        at = (x, y);
                    }
                }
            }
        }
        assert!(
            worst < 4.0e-4,
            "relative error is {worst:e} at x={}, y={} — measured at 2.73e-4",
            at.0, at.1,
        );
    }

    #[test]
    fn the_edges_are_handled_rather_than_merely_survived() {
        assert_eq!(powf_approx(0.0, 0.4545), 0.0, "zero to a positive power is zero");
        assert_eq!(powf_approx(-1.0, 2.0), 0.0, "a negative base is refused rather than guessed");

        assert_eq!(powf_approx(0.0, 0.0), 0.0, "the divergence from std at 0^0 has moved");
        assert_eq!(0.0f32.powf(0.0), 1.0, "std no longer answers 1 for 0^0, so the note above is stale");
        assert!(powf_approx(1.0, 1.0) > 1.0, "the overshoot this fit has is gone, so the clamp is now untested");
        assert!(powf_approx(1.0, 1.0) < 1.001, "the overshoot grew past what the clamp absorbs");
        assert_eq!(gamma_lut(1.0)[255], 255, "the overshoot reached the table");
    }
}
