use alloc::string::String;

use super::{GlyphInstance, GpuBatch, Mode, SubpixelParams};
use crate::daerizer::draw::DeviceProfile;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    NoDevice,
    BadTarget,
    Unsupported,
    Failed,
}

pub trait Surface {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn pixels(&self) -> &[u8];
    fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]>;
}

pub trait Uploaded {
    fn revision(&self) -> u64;
}

pub trait Backend: Sized {
    type Error: core::fmt::Debug + core::fmt::Display;

    // Generic over a lifetime because one of the four needs it: `vk::Target` borrows its renderer,
    // since `VkDevice` is not refcounted and `drop(renderer); drop(target)` segfaulted until the
    // borrow made it a compile error. The other three hold a reference and ignore the parameter.
    type Target<'r>: Surface
    where
        Self: 'r;

    type Geometry<'r>: Uploaded
    where
        Self: 'r;

    const NAME: &'static str;

    fn new() -> Result<Self, Self::Error>;

    fn refusal(e: &Self::Error) -> Refusal;

    fn target(&self, width: u32, height: u32) -> Result<Self::Target<'_>, Self::Error>;

    fn geometry(&self, batch: &GpuBatch) -> Result<Self::Geometry<'_>, Self::Error>;

    fn draw(
        &self,
        target: &mut Self::Target<'_>,
        geometry: &Self::Geometry<'_>,
        instances: &[GlyphInstance],
        subpixel: &SubpixelParams,
        mode: Mode,
    ) -> Result<(), Self::Error>;

    fn draw_with(
        &self,
        target: &mut Self::Target<'_>,
        geometry: &Self::Geometry<'_>,
        instances: &[GlyphInstance],
        subpixel: &SubpixelParams,
        mode: Mode,
        projection: &[f32; 16],
    ) -> Result<(), Self::Error>;

    // Waits on what drew the target, not on this renderer's own latest work. Two renderers on one
    // device hold two queues on Metal and D3D12, and the wrong one returns while the draw runs.
    fn wait(&self, target: &mut Self::Target<'_>) -> Result<(), Self::Error>;

    fn read_pixels<'t>(&self, target: &'t mut Self::Target<'_>) -> Result<&'t [u8], Self::Error>;

    fn profile(&self) -> DeviceProfile;

    fn device_name(&self) -> String;

    fn supports_subpixel(&self) -> bool;

    // Not the same matrix for all four: Vulkan's clip space runs y down where Metal's and D3D's
    // run y up, so Vulkan negates the y scale. The input is pixels, origin bottom left, either way.
    fn ortho(width: u32, height: u32) -> [f32; 16];
}
