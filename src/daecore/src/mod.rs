// `forbid` at module scope loses nothing to crate scope: lint levels nest, so an
// `#[allow(unsafe_code)]` anywhere below this line is `E0453`, a hard error rather than an
// override. The engine cannot contain unsafe by construction, and this is where a hostile font's
// bytes are parsed.
#![forbid(unsafe_code)]
#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod sync;

pub mod daemachine;
pub mod daetype;
pub mod daeshaper;

pub mod cache;

pub mod text;
