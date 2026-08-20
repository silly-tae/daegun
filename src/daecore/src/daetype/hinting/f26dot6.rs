pub(crate) const ONE: i32 = 64;
pub(crate) const HALF: i32 = 32;

pub(crate) const F2DOT14_ONE: i32 = 0x4000;

pub fn scale(value: i32, ppem: u16, upm: u16) -> i32 {
    if upm == 0 { return 0; }
    let num = value as i64 * ppem as i64 * ONE as i64;
    let den = upm as i64;
    let half = den / 2;
    let adjusted = if num >= 0 { num + half } else { num - half };
    clamp_i32(adjusted / den)
}

// Clamped rather than cast: `as i32` truncates the high bits, so a coordinate past the format
// re-enters it as a small in-range value rather than being pinned at the rail.
pub(crate) fn clamp_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

pub(crate) fn round_to_grid(v: i32) -> i32 {
    if v >= 0 {
        floor_pixel(v.saturating_add(HALF))
    } else {
        -floor_pixel(v.saturating_neg().saturating_add(HALF))
    }
}

pub(crate) fn round_to_half_grid(v: i32) -> i32 {
    if v >= 0 {
        floor_pixel(v.saturating_add(HALF)).saturating_add(HALF)
    } else {
        -(floor_pixel(v.saturating_neg().saturating_add(HALF)).saturating_add(HALF))
    }
}

pub(crate) fn floor_pixel(v: i32) -> i32 {
    v & !(ONE - 1)
}

pub(crate) fn ceil_pixel(v: i32) -> i32 {
    v.saturating_add(ONE - 1) & !(ONE - 1)
}

pub(crate) fn mul_f2dot14(a: i32, b: i32) -> i32 {
    let p = a as i64 * b as i64;
    let half = (F2DOT14_ONE / 2) as i64;
    let adjusted = if p >= 0 { p + half } else { p - half };
    clamp_i32(adjusted / F2DOT14_ONE as i64)
}

pub(crate) fn mul(a: i32, b: i32) -> i32 {
    let p = a as i64 * b as i64;
    let adjusted = if p >= 0 { p + HALF as i64 } else { p - HALF as i64 };
    clamp_i32(adjusted / ONE as i64)
}

pub(crate) fn div(a: i32, b: i32) -> i32 {
    if b == 0 { return 0; }
    let n = a as i64 * ONE as i64;
    let d = b as i64;
    let half = d.abs() / 2;
    let adjusted = if (n >= 0) == (d >= 0) { n + half } else { n - half };
    clamp_i32(adjusted / d)
}
