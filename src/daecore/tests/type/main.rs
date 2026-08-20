#[allow(dead_code, reason = "mounted test files use it; the target compiles empty until they exist")]
const FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test-fonts");

mod outline_latency;

mod autohint_points;

mod stroke;

mod hint_latency;

mod colr_latency;

mod colr_cache;

mod cffdecline;

mod fdef_scope;

#[cfg(feature = "threading")]
mod hint_threading;
