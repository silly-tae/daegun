#[allow(dead_code, reason = "mounted test files use it; the target compiles empty until they exist")]
const FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test-fonts");

mod autohint;

mod stroke;

mod synthetic;

mod outline_sweep;

mod varhint;

#[cfg(feature = "threading")]
mod threading;

mod surface;

mod surface_glyphs;

mod prewarm;

mod access;

mod segmentation;

mod hybrid;

mod latency;

mod cached_facts;

mod scene_orientation;

mod rasterizer_agreement;
mod overlapping_contours;
mod linebreak_stretch;
