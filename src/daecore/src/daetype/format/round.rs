#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;

// The spec's own rounding: half-integer ties toward +Infinity, for glyf/gvar points, cvar deltas
// and MVAR. `banker_round` below is half-to-even, for CFF2 `blend`. Rust's `f64::round` is neither,
// and the wrong one gives real off-by-one divergences at interior axis locations.
pub fn ot_round(v: f64) -> i32 {
    (v + 0.5).floor() as i32
}

pub(crate) fn banker_round(v: f64) -> i32 {
    v.round_ties_even() as i32
}

pub(crate) fn quantize_f2dot14(v: f64) -> f64 {
    ot_round(v * 16384.0) as f64 / 16384.0
}
