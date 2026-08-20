#[allow(dead_code, reason = "mounted test files use it; the target compiles empty until they exist")]
const FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test-fonts");

mod latency;

mod simd_diff;

mod simd_agreement;
mod zero_delta;
mod degenerate;
mod overlapping_contours;
