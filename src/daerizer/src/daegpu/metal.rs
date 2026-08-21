use alloc::string::String;
use core::ffi::c_void;

use super::objc::{Bool, Id, NO, Owned, Sel, class, sel, send0, send1, send2, send3, send4, send9};

#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {
    fn MTLCreateSystemDefaultDevice() -> Id;
}

pub const PIXEL_FORMAT_RGBA8_UNORM: u64 = 70;
pub const PIXEL_FORMAT_BGRA8_UNORM: u64 = 80;
pub const TEXTURE_USAGE_RENDER_TARGET: u64 = 4;
pub const STORAGE_MODE_SHARED: u64 = 0;
pub const LOAD_ACTION_LOAD: u64 = 1;
pub const LOAD_ACTION_CLEAR: u64 = 2;
pub const STORE_ACTION_STORE: u64 = 1;
pub const PRIMITIVE_TRIANGLE_STRIP: u64 = 4;
pub const RESOURCE_STORAGE_MODE_SHARED: u64 = 0;
pub const LANGUAGE_VERSION_2_2: u64 = (2 << 16) | 2;
pub const BLEND_FACTOR_SOURCE1_COLOR: u64 = 15;
pub const BLEND_FACTOR_ONE_MINUS_SOURCE1_COLOR: u64 = 16;

#[repr(C)]
pub struct Origin {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[repr(C)]
pub struct Size {
    pub width: u64,
    pub height: u64,
    pub depth: u64,
}

#[repr(C)]
// Four doubles make a homogeneous float aggregate, which AAPCS64 passes in v0-v3 – the exception
// to composites over 16 bytes going indirect, and why `send1::<ClearColor, ()>` is right rather
// than merely the right size. Origin and Size are over 16 and not HFAs, so they do go indirect.
pub struct ClearColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

// The other two FFI layers were guarded and this one was not, on the same class of thing: a struct
// transcribed from a header and passed by value. vulkan.rs was 8 bytes short and the driver wrote
// past a stack variable; direct3d.rs was 4 short and CreateShaderResourceView read past one.
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(size_of::<Origin>() == 24 && align_of::<Origin>() == 8);
    assert!(offset_of!(Origin, x) == 0 && offset_of!(Origin, y) == 8 && offset_of!(Origin, z) == 16);
    assert!(size_of::<Size>() == 24 && align_of::<Size>() == 8);
    assert!(
        offset_of!(Size, width) == 0
            && offset_of!(Size, height) == 8
            && offset_of!(Size, depth) == 16
    );
    assert!(size_of::<ClearColor>() == 32 && align_of::<ClearColor>() == 8);
    assert!(
        offset_of!(ClearColor, red) == 0
            && offset_of!(ClearColor, green) == 8
            && offset_of!(ClearColor, blue) == 16
            && offset_of!(ClearColor, alpha) == 24
    );
    assert!(size_of::<u64>() == 8 && size_of::<Bool>() == 1);
};

const _: (Origin, Size, ClearColor) = (
    Origin { x: 0u64, y: 0u64, z: 0u64 },
    Size { width: 0u64, height: 0u64, depth: 0u64 },
    ClearColor { red: 0.0f64, green: 0.0f64, blue: 0.0f64, alpha: 0.0f64 },
);

pub fn system_default_device() -> Option<Owned> {
    unsafe { Owned::new(MTLCreateSystemDefaultDevice()) }
}

pub unsafe fn device_name(device: Id) -> String {
    unsafe { super::objc::to_string(send0(device, sel(c"name"))) }
}

pub unsafe fn new_command_queue(device: Id) -> Option<Owned> {
    unsafe { Owned::new(send0(device, sel(c"newCommandQueue"))) }
}

pub unsafe fn registry_id(device: Id) -> u64 {
    unsafe { send0(device, sel(c"registryID")) }
}

pub unsafe fn has_unified_memory(device: Id) -> Option<bool> {
    let known: Bool =
        unsafe { send1(device, sel(c"respondsToSelector:"), sel(c"hasUnifiedMemory")) };
    if known == NO {
        return None;
    }
    let uma: Bool = unsafe { send0(device, sel(c"hasUnifiedMemory")) };
    Some(uma != NO)
}

pub unsafe fn new_library(device: Id, source: &str) -> Result<Owned, String> {
    let Some(src) = super::objc::nsstring(source) else {
        return Err("could not allocate an NSString for the shader source".into());
    };

    let options: Id = unsafe { send0(class(c"MTLCompileOptions"), sel(c"new")) };
    let Some(options) = (unsafe { Owned::new(options) }) else {
        return Err("could not allocate MTLCompileOptions".into());
    };
    unsafe {
        send1::<u64, ()>(options.id(), sel(c"setLanguageVersion:"), LANGUAGE_VERSION_2_2);
    }

    let mut error: Id = core::ptr::null_mut();
    let lib: Id = unsafe {
        send3(
            device,
            sel(c"newLibraryWithSource:options:error:"),
            src.id(),
            options.id(),
            &raw mut error,
        )
    };
    match unsafe { Owned::new(lib) } {
        Some(lib) => Ok(lib),
        None => Err(unsafe { super::objc::error_message(error) }),
    }
}

pub unsafe fn new_function(library: Id, name: &str) -> Option<Owned> {
    let name = super::objc::nsstring(name)?;
    unsafe { Owned::new(send1(library, sel(c"newFunctionWithName:"), name.id())) }
}

pub unsafe fn new_buffer<T>(device: Id, data: &[T]) -> Option<Owned> {
    let filler = [0u8; 4];
    let (ptr, len) = if data.is_empty() {
        (filler.as_ptr().cast::<c_void>(), filler.len() as u64)
    } else {
        (data.as_ptr().cast::<c_void>(), core::mem::size_of_val(data) as u64)
    };
    unsafe {
        Owned::new(send3(
            device,
            sel(c"newBufferWithBytes:length:options:"),
            ptr,
            len,
            RESOURCE_STORAGE_MODE_SHARED,
        ))
    }
}

pub unsafe fn new_render_target(
    device: Id,
    width: u32,
    height: u32,
    pixel_format: u64,
) -> Option<Owned> {
    let desc: Id = unsafe {
        send4(
            class(c"MTLTextureDescriptor"),
            sel(c"texture2DDescriptorWithPixelFormat:width:height:mipmapped:"),
            pixel_format,
            u64::from(width),
            u64::from(height),
            NO,
        )
    };
    if desc.is_null() {
        return None;
    }
    unsafe {
        send1::<u64, ()>(desc, sel(c"setUsage:"), TEXTURE_USAGE_RENDER_TARGET);
        send1::<u64, ()>(desc, sel(c"setStorageMode:"), STORAGE_MODE_SHARED);
        Owned::new(send1(device, sel(c"newTextureWithDescriptor:"), desc))
    }
}

pub unsafe fn new_pipeline(
    device: Id,
    vertex_fn: Id,
    fragment_fn: Id,
    dual_source: bool,
    pixel_format: u64,
) -> Result<Owned, String> {
    let desc: Id = unsafe { send0(class(c"MTLRenderPipelineDescriptor"), sel(c"new")) };
    let Some(desc) = (unsafe { Owned::new(desc) }) else {
        return Err("could not allocate MTLRenderPipelineDescriptor".into());
    };
    unsafe {
        send1::<Id, ()>(desc.id(), sel(c"setVertexFunction:"), vertex_fn);
        send1::<Id, ()>(desc.id(), sel(c"setFragmentFunction:"), fragment_fn);
    }

    let attachments: Id = unsafe { send0(desc.id(), sel(c"colorAttachments")) };
    let attachment: Id = unsafe { send1(attachments, sel(c"objectAtIndexedSubscript:"), 0u64) };
    unsafe {
        send1::<u64, ()>(attachment, sel(c"setPixelFormat:"), pixel_format);
        if dual_source {
            send1::<Bool, ()>(attachment, sel(c"setBlendingEnabled:"), 1);
            for (setter, factor) in [
                (sel(c"setSourceRGBBlendFactor:"), BLEND_FACTOR_SOURCE1_COLOR),
                (sel(c"setSourceAlphaBlendFactor:"), BLEND_FACTOR_SOURCE1_COLOR),
                (sel(c"setDestinationRGBBlendFactor:"), BLEND_FACTOR_ONE_MINUS_SOURCE1_COLOR),
                (sel(c"setDestinationAlphaBlendFactor:"), BLEND_FACTOR_ONE_MINUS_SOURCE1_COLOR),
            ] {
                send1::<u64, ()>(attachment, setter, factor);
            }
        }
    }

    let mut error: Id = core::ptr::null_mut();
    let pipeline: Id = unsafe {
        send2(
            device,
            sel(c"newRenderPipelineStateWithDescriptor:error:"),
            desc.id(),
            &raw mut error,
        )
    };
    match unsafe { Owned::new(pipeline) } {
        Some(pipeline) => Ok(pipeline),
        None => Err(unsafe { super::objc::error_message(error) }),
    }
}

// `clear` of `None` loads what the target already holds, which is what lets a second geometry draw
// over the first instead of erasing it.
pub unsafe fn render_pass(texture: Id, clear: Option<ClearColor>) -> Id {
    let pass: Id =
        unsafe { send0(class(c"MTLRenderPassDescriptor"), sel(c"renderPassDescriptor")) };
    let attachments: Id = unsafe { send0(pass, sel(c"colorAttachments")) };
    let attachment: Id = unsafe { send1(attachments, sel(c"objectAtIndexedSubscript:"), 0u64) };
    unsafe {
        send1::<Id, ()>(attachment, sel(c"setTexture:"), texture);
        send1::<u64, ()>(attachment, sel(c"setStoreAction:"), STORE_ACTION_STORE);
        match clear {
            Some(color) => {
                send1::<u64, ()>(attachment, sel(c"setLoadAction:"), LOAD_ACTION_CLEAR);
                send1::<ClearColor, ()>(attachment, sel(c"setClearColor:"), color);
            }
            None => send1::<u64, ()>(attachment, sel(c"setLoadAction:"), LOAD_ACTION_LOAD),
        }
    }
    pass
}

pub unsafe fn set_vertex_buffer(encoder: Id, buffer: Id, index: u32) {
    unsafe {
        send3::<Id, u64, u64, ()>(
            encoder,
            sel(c"setVertexBuffer:offset:atIndex:"),
            buffer,
            0,
            u64::from(index),
        );
    }
}

pub unsafe fn set_fragment_buffer(encoder: Id, buffer: Id, index: u32) {
    unsafe {
        send3::<Id, u64, u64, ()>(
            encoder,
            sel(c"setFragmentBuffer:offset:atIndex:"),
            buffer,
            0,
            u64::from(index),
        );
    }
}

pub unsafe fn set_vertex_bytes<T>(encoder: Id, data: &T, index: u32) {
    unsafe { set_bytes(encoder, sel(c"setVertexBytes:length:atIndex:"), data, index) }
}

pub unsafe fn set_fragment_bytes<T>(encoder: Id, data: &T, index: u32) {
    unsafe { set_bytes(encoder, sel(c"setFragmentBytes:length:atIndex:"), data, index) }
}

unsafe fn set_bytes<T>(encoder: Id, selector: Sel, data: &T, index: u32) {
    unsafe {
        send3::<*const c_void, u64, u64, ()>(
            encoder,
            selector,
            (data as *const T).cast(),
            core::mem::size_of::<T>() as u64,
            u64::from(index),
        );
    }
}

pub unsafe fn new_buffer_uninit(device: Id, len: usize) -> Option<Owned> {
    unsafe {
        Owned::new(send2(
            device,
            sel(c"newBufferWithLength:options:"),
            len.max(4) as u64,
            RESOURCE_STORAGE_MODE_SHARED,
        ))
    }
}

pub unsafe fn buffer_contents(buffer: Id) -> *mut c_void {
    unsafe { send0(buffer, sel(c"contents")) }
}

pub unsafe fn write_buffer<T>(buffer: Id, data: &[T]) {
    if data.is_empty() {
        return;
    }
    unsafe {
        let dst = buffer_contents(buffer);
        if dst.is_null() {
            return;
        }
        core::ptr::copy_nonoverlapping(
            data.as_ptr().cast::<u8>(),
            dst.cast::<u8>(),
            core::mem::size_of_val(data),
        );
    }
}

pub unsafe fn blit_encoder(commands: Id) -> Id {
    unsafe { send0(commands, sel(c"blitCommandEncoder")) }
}

pub unsafe fn blit_texture_to_buffer(encoder: Id, texture: Id, buffer: Id, width: u32, height: u32) {
    let origin = Origin { x: 0, y: 0, z: 0 };
    let size = Size { width: u64::from(width), height: u64::from(height), depth: 1 };
    let bytes_per_row = u64::from(width) * 4;
    unsafe {
        send9::<Id, u64, u64, Origin, Size, Id, u64, u64, u64, ()>(
            encoder,
            sel(c"copyFromTexture:sourceSlice:sourceLevel:sourceOrigin:sourceSize:toBuffer:destinationOffset:destinationBytesPerRow:destinationBytesPerImage:"),
            texture,
            0,
            0,
            origin,
            size,
            buffer,
            0,
            bytes_per_row,
            bytes_per_row * u64::from(height),
        );
    }
}

pub unsafe fn send_void(obj: Id, selector: Sel) {
    unsafe { send0::<()>(obj, selector) }
}

pub unsafe fn drawable_texture(drawable: Id) -> Id {
    unsafe { send0(drawable, sel(c"texture")) }
}

// Presentation rides the command buffer that already carries the draw, so the two are ordered by
// the queue rather than by the caller waiting on anything.
pub unsafe fn present_drawable(commands: Id, drawable: Id) {
    unsafe { send1::<Id, ()>(commands, sel(c"presentDrawable:"), drawable) }
}

pub unsafe fn texture_pixel_format(texture: Id) -> u64 {
    unsafe { send0(texture, sel(c"pixelFormat")) }
}
