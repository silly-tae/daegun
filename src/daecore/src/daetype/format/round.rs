#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;

// The spec's own rounding: half-integer ties toward +Infinity, for glyf/gvar points, cvar deltas
// and MVAR. `banker_round_i64` below is half-to-even, for CFF2 `blend`. Rust's `f64::round` is
// neither, and the wrong one gives real off-by-one divergences at interior axis locations.
pub fn ot_round(v: f64) -> i32 {
    (v + 0.5).floor() as i32
}

// CFF2 charstring coordinates are relative, so rounding a blended delta to a whole unit
// accumulates error along the outline. The caller scales to 16.16 before rounding.
pub(crate) fn banker_round_i64(v: f64) -> i64 {
    v.round_ties_even() as i64
}

pub(crate) fn quantize_f2dot14(v: f64) -> f64 {
    ot_round(v * 16384.0) as f64 / 16384.0
}
