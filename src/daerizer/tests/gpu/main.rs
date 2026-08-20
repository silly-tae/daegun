#[allow(dead_code, reason = "mounted test files use it; the target compiles empty until they exist")]
fn fonts_dir() -> String {
    std::env::var("DAEGUN_FONTS").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test-fonts").to_string()
    })
}

mod face;
mod degenerate;
mod hull;
mod batch;
mod shared_row;
mod conformance;
mod latency;
mod winding;

mod vulkan;

#[cfg(target_vendor = "apple")]
mod render;

#[cfg(target_vendor = "apple")]
mod cross;

#[cfg(windows)]
mod d3d11;
#[cfg(windows)]
mod d3d12;

#[cfg(windows)]
mod cross_d3d;
