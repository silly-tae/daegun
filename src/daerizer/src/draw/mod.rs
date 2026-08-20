// The three engines are not interchangeable: only the CPU rasterizer hints, strokes and
// gamma-corrects, so every glyph the GPU can draw the CPU can too and not the reverse. That
// asymmetry is the whole reason a router exists.
pub mod device;
pub mod policy;
pub mod route;

pub use device::{DeviceKind, DeviceProfile};
pub use policy::{Policy, Prefer};
pub use route::{route, Refusal, Rendered, Request};
