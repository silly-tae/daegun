// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Where a call takes a raw buffer, its length is the caller's promise
// from `daegun.h` and is not checkable here; the one argument that is not a pointer rule is
// written at its site.

use alloc::string::String;
use core::ffi::{CStr, c_char};

use alloc::sync::Arc;
use crate::{GlyphInstance, GpuBatch, ShaderLanguage, ShaderStage, SubpixelParams};
use crate::paint::daegpu::Mode;
use crate::paint::draw::{DeviceKind, DeviceProfile, Refusal, Rendered, Request};

use crate::ffi::draw::{Batch, PolicyC};
use crate::ffi::handle::{Status, borrow, deliver, release};
use crate::ffi::list::Text;
use crate::ffi::set_error;

pub const SHADER_GLSL: i32 = 0;
pub const SHADER_HLSL: i32 = 1;
pub const SHADER_MSL: i32 = 2;

pub const STAGE_VERTEX: i32 = 0;
pub const STAGE_FRAGMENT: i32 = 1;
pub const STAGE_SUBPIXEL_FRAGMENT: i32 = 2;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_shader_source(
    language: i32,
    stage: i32,
    out: *mut *mut Text,
) -> Status {
    let language = match language {
        SHADER_HLSL => ShaderLanguage::Hlsl,
        SHADER_MSL => ShaderLanguage::Metal,
        SHADER_GLSL => ShaderLanguage::Glsl,
        _ => return Status::Range,
    };
    let stage = match stage {
        STAGE_FRAGMENT => ShaderStage::Fragment,
        STAGE_SUBPIXEL_FRAGMENT => ShaderStage::SubpixelFragment,
        STAGE_VERTEX => ShaderStage::Vertex,
        _ => return Status::Range,
    };
    unsafe { deliver(out, Text::new(crate::shader(language, stage))) }
}

pub const DEVICE_UNKNOWN: i32 = 0;
pub const DEVICE_DISCRETE: i32 = 1;
pub const DEVICE_INTEGRATED: i32 = 2;
pub const DEVICE_VIRTUAL: i32 = 3;
pub const DEVICE_SOFTWARE: i32 = 4;

fn kind_code(kind: DeviceKind) -> i32 {
    match kind {
        DeviceKind::Discrete => DEVICE_DISCRETE,
        DeviceKind::Integrated => DEVICE_INTEGRATED,
        DeviceKind::Virtual => DEVICE_VIRTUAL,
        DeviceKind::Software => DEVICE_SOFTWARE,
        DeviceKind::Unknown => DEVICE_UNKNOWN,
    }
}

fn kind_of(code: i32) -> DeviceKind {
    match code {
        DEVICE_DISCRETE => DeviceKind::Discrete,
        DEVICE_INTEGRATED => DeviceKind::Integrated,
        DEVICE_VIRTUAL => DeviceKind::Virtual,
        DEVICE_SOFTWARE => DeviceKind::Software,
        _ => DeviceKind::Unknown,
    }
}

unsafe fn text_of<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(s) }.to_str().ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_new(
    kind: i32,
    name: *const c_char,
    out: *mut *mut DeviceProfile,
) -> Status {
    let Some(name) = (unsafe { text_of(name) }) else { return Status::Null };
    unsafe { deliver(out, DeviceProfile::new(kind_of(kind), name)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_from_vulkan(
    device_type: i32,
    name: *const c_char,
    out: *mut *mut DeviceProfile,
) -> Status {
    let Some(name) = (unsafe { text_of(name) }) else { return Status::Null };
    unsafe { deliver(out, DeviceProfile::from_vulkan(device_type, name)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_kind(
    profile: *const DeviceProfile,
    out: *mut i32,
) -> Status {
    let Some(profile) = (unsafe { borrow(profile) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = kind_code(profile.kind) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_name(
    profile: *const DeviceProfile,
    out: *mut *mut Text,
) -> Status {
    let Some(profile) = (unsafe { borrow(profile) }) else { return Status::Null };
    unsafe { deliver(out, Text::new(&profile.name)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_free(profile: *mut DeviceProfile) {
    unsafe { release(profile) }
}

pub const GPU_OK: i32 = 0;
pub const GPU_NO_OUTLINE: i32 = 1;
pub const GPU_TOO_COMPLEX: i32 = 2;
pub const GPU_NON_FINITE: i32 = 3;
pub const GPU_BATCH_FULL: i32 = 4;
pub const GPU_NOT_FLAT_COLOR: i32 = 5;

pub const ROUTED_NOTHING: i32 = 0;
pub const ROUTED_CPU: i32 = 1;
pub const ROUTED_GPU: i32 = 2;
pub const ROUTED_REFERENCE: i32 = 3;
pub const ROUTED_SCENE: i32 = 4;
pub const ROUTED_FLUSH_AND_RETRY: i32 = 5;
pub const ROUTED_REFUSED_NON_FINITE: i32 = 6;
pub const ROUTED_REFUSED_PREFERENCE_UNMET: i32 = 7;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RequestC {
    pub ppem: f32,
    pub hinted: i32,
    pub stroked: i32,
    pub gamma: i32,
    pub emboldened: i32,
    pub obliqued: i32,
}

const _: () = assert!(size_of::<RequestC>() == 24);
const _: () = assert!(align_of::<RequestC>() == 4);

impl RequestC {
    fn to_rust(self) -> Request {
        Request {
            ppem: self.ppem,
            hinted: self.hinted != 0,
            stroked: self.stroked != 0,
            gamma: self.gamma != 0,
            emboldened: self.emboldened != 0,
            obliqued: self.obliqued != 0,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_route(
    attempt: i32,
    request: *const RequestC,
    device: *const DeviceProfile,
    policy: *const PolicyC,
    out: *mut i32,
) -> Status {
    let Some(request) = (unsafe { borrow(request) }) else { return Status::Null };
    let Some(policy) = (unsafe { borrow(policy) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let attempt = match attempt {
        GPU_OK => Ok(()),
        GPU_NO_OUTLINE => Err(crate::GpuGlyphError::NoOutline),
        GPU_TOO_COMPLEX => Err(crate::GpuGlyphError::TooComplex),
        GPU_NON_FINITE => Err(crate::GpuGlyphError::NonFinite),
        GPU_BATCH_FULL => Err(crate::GpuGlyphError::BatchFull),
        GPU_NOT_FLAT_COLOR => Err(crate::GpuGlyphError::NotFlatColor),
        _ => return Status::Range,
    };
    let device = unsafe { borrow(device) };
    let routed = crate::route(attempt, &request.to_rust(), device, &policy.to_rust());
    unsafe {
        *out = match routed {
            Rendered::Nothing => ROUTED_NOTHING,
            Rendered::Cpu => ROUTED_CPU,
            Rendered::Gpu => ROUTED_GPU,
            Rendered::Reference => ROUTED_REFERENCE,
            Rendered::Scene => ROUTED_SCENE,
            Rendered::FlushAndRetry => ROUTED_FLUSH_AND_RETRY,
            Rendered::Refused(Refusal::NonFinite) => ROUTED_REFUSED_NON_FINITE,
            Rendered::Refused(Refusal::PreferenceUnmet) => ROUTED_REFUSED_PREFERENCE_UNMET,
        }
    };
    Status::Ok
}

pub const MODE_GRAYSCALE: i32 = 0;
pub const MODE_SUBPIXEL: i32 = 1;

pub const REFUSAL_NO_DEVICE: i32 = 0;
pub const REFUSAL_BAD_TARGET: i32 = 1;
pub const REFUSAL_UNSUPPORTED: i32 = 2;
pub const REFUSAL_FAILED: i32 = 3;

fn mode_of(code: i32) -> Mode {
    match code {
        MODE_SUBPIXEL => Mode::Subpixel,
        MODE_GRAYSCALE => Mode::Grayscale,
        _ => Mode::Grayscale,
    }
}

fn backend_refusal(r: crate::paint::daegpu::backend::Refusal) -> i32 {
    use crate::paint::daegpu::backend::Refusal as R;
    match r {
        R::NoDevice => REFUSAL_NO_DEVICE,
        R::BadTarget => REFUSAL_BAD_TARGET,
        R::Unsupported => REFUSAL_UNSUPPORTED,
        R::Failed => REFUSAL_FAILED,
    }
}

unsafe fn instances_of<'a>(instances: *const GlyphInstance, count: usize) -> &'a [GlyphInstance] {
    if instances.is_null() || count == 0 {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(instances, count) }
}

unsafe fn projection_of(m: *const f32) -> Option<[f32; 16]> {
    if m.is_null() {
        return None;
    }
    let mut out = [0.0f32; 16];
    out.copy_from_slice(unsafe { core::slice::from_raw_parts(m, 16) });
    Some(out)
}

unsafe fn batch_of<'a>(batch: *const Batch) -> Option<&'a GpuBatch> {
    unsafe { borrow(batch) }.map(Batch::inner)
}

fn fail(message: String, refusal: i32) -> Status {
    set_error(&message);
    match refusal {
        REFUSAL_NO_DEVICE => Status::Unsupported,
        REFUSAL_UNSUPPORTED => Status::Unsupported,
        REFUSAL_BAD_TARGET => Status::Range,
        _ => Status::Parse,
    }
}

#[inline]
unsafe fn gpu_ref<'a, T>(handle: *const T) -> Option<&'a T> {
    if handle.is_null() {
        None
    } else {
        Some(unsafe { &*handle })
    }
}

#[inline]
unsafe fn gpu_mut<'a, T>(handle: *mut T) -> Option<&'a mut T> {
    if handle.is_null() {
        None
    } else {
        Some(unsafe { &mut *handle })
    }
}

macro_rules! backend {
    (
        $backend:path, $modname:ident,
        $renderer:ident, $target:ident, $geometry:ident,
        $target_ty:ty, $geom_ty:ty,
        $new:ident, $free:ident, $name:ident, $profile:ident, $subpixel:ident, $ortho:ident,
        $mk_target:ident, $t_with_format:ident, $t_set_clear:ident,
        $t_width:ident, $t_height:ident, $t_pixels:ident, $t_pixel:ident,
        $t_free:ident,
        $mk_geom:ident, $g_revision:ident, $g_free:ident,
        $draw:ident, $draw_with:ident, $wait:ident, $read:ident,
        $detach_t:expr, $detach_g:expr
    ) => {
        pub mod $modname {
        use super::*;
        use $backend as backend_mod;

        pub(crate) fn err_status(e: &backend_mod::Error) -> Status {
            use crate::paint::daegpu::backend::Backend;
            let refusal = <backend_mod::Renderer as Backend>::refusal(e);
            fail(alloc::format!("{e}"), backend_refusal(refusal))
        }

        pub struct $renderer(pub(crate) Arc<backend_mod::Renderer>);

        pub struct $target {
            pub(crate) inner: $target_ty,
            pub(crate) _renderer: Arc<backend_mod::Renderer>,
        }

        pub struct $geometry {
            pub(crate) inner: $geom_ty,
            _renderer: Arc<backend_mod::Renderer>,
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $new(out: *mut *mut $renderer) -> Status {
            match backend_mod::Renderer::new() {
                Ok(r) => unsafe { deliver(out, $renderer(Arc::new(r))) },
                Err(e) => err_status(&e),
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $free(renderer: *mut $renderer) {
            unsafe { release(renderer) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            renderer: *const $renderer,
            out: *mut *mut Text,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            unsafe { deliver(out, Text::new(&r.0.device_name())) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $profile(
            renderer: *const $renderer,
            out: *mut *mut DeviceProfile,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            unsafe { deliver(out, r.0.profile()) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $subpixel(
            renderer: *const $renderer,
            out: *mut i32,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            if out.is_null() {
                return Status::Null;
            }
            let supported = <backend_mod::Renderer as
                crate::paint::daegpu::backend::Backend>::supports_subpixel(&r.0);
            unsafe { *out = i32::from(supported) };
            Status::Ok
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $ortho(width: u32, height: u32, out: *mut f32) -> Status {
            if out.is_null() {
                return Status::Null;
            }
            let m = backend_mod::ortho(width, height);
            unsafe { core::slice::from_raw_parts_mut(out, 16) }.copy_from_slice(&m);
            Status::Ok
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $mk_target(
            renderer: *const $renderer,
            width: u32,
            height: u32,
            out: *mut *mut $target,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            match r.0.target(width, height) {
                Ok(t) => {
                    let inner = $detach_t(t);
                    unsafe { deliver(out, $target { inner, _renderer: Arc::clone(&r.0) }) }
                }
                Err(e) => err_status(&e),
            }
        }

        // An offscreen target in the caller's byte order. `format` is a `DAEGUN_SURFACE_*` value.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $t_with_format(
            renderer: *const $renderer,
            width: u32,
            height: u32,
            format: i32,
            out: *mut *mut $target,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            let format = match format {
                0 => crate::paint::daegpu::backend::SurfaceFormat::Rgba8Unorm,
                1 => crate::paint::daegpu::backend::SurfaceFormat::Bgra8Unorm,
                _ => return Status::Range,
            };
            match r.0.target_with_format(width, height, format) {
                Ok(t) => {
                    let inner = $detach_t(t);
                    unsafe { deliver(out, $target { inner, _renderer: Arc::clone(&r.0) }) }
                }
                Err(e) => err_status(&e),
            }
        }

        // What the target clears to before each draw, four bytes RGBA. NULL keeps what it already
        // holds, which is how a second geometry draws over the first rather than erasing it.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $t_set_clear(target: *mut $target, clear: *const u8) -> Status {
            let Some(t) = (unsafe { gpu_mut(target) }) else { return Status::Null };
            t.inner.set_clear(unsafe { crate::ffi::rgba_of(clear) });
            Status::Ok
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $t_width(target: *const $target, out: *mut u32) -> Status {
            let Some(t) = (unsafe { gpu_ref(target) }) else { return Status::Null };
            if out.is_null() {
                return Status::Null;
            }
            unsafe { *out = t.inner.width() };
            Status::Ok
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $t_height(target: *const $target, out: *mut u32) -> Status {
            let Some(t) = (unsafe { gpu_ref(target) }) else { return Status::Null };
            if out.is_null() {
                return Status::Null;
            }
            unsafe { *out = t.inner.height() };
            Status::Ok
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $t_pixels(
            target: *const $target,
            out_count: *mut usize,
        ) -> *const u8 {
            let Some(t) = (unsafe { gpu_ref(target) }) else { return core::ptr::null() };
            if out_count.is_null() {
                return core::ptr::null();
            }
            let px = t.inner.pixels();
            unsafe { *out_count = px.len() };
            px.as_ptr()
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $t_pixel(
            target: *const $target,
            x: u32,
            y: u32,
            out: *mut u8,
        ) -> Status {
            let Some(t) = (unsafe { gpu_ref(target) }) else { return Status::Null };
            if out.is_null() {
                return Status::Null;
            }
            let Some(px) = t.inner.pixel(x, y) else { return Status::Range };
            unsafe { core::slice::from_raw_parts_mut(out, 4) }.copy_from_slice(&px);
            Status::Ok
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $t_free(target: *mut $target) {
            unsafe { release(target) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $mk_geom(
            renderer: *const $renderer,
            batch: *const Batch,
            out: *mut *mut $geometry,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            let Some(b) = (unsafe { batch_of(batch) }) else { return Status::Null };
            match r.0.geometry(b) {
                Ok(g) => {
                    let inner = $detach_g(g);
                    unsafe { deliver(out, $geometry { inner, _renderer: Arc::clone(&r.0) }) }
                }
                Err(e) => err_status(&e),
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $g_revision(
            geometry: *const $geometry,
            out: *mut u64,
        ) -> Status {
            let Some(g) = (unsafe { gpu_ref(geometry) }) else { return Status::Null };
            if out.is_null() {
                return Status::Null;
            }
            unsafe { *out = g.inner.revision() };
            Status::Ok
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $g_free(geometry: *mut $geometry) {
            unsafe { release(geometry) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $draw(
            renderer: *const $renderer,
            target: *mut $target,
            geometry: *const $geometry,
            instances: *const GlyphInstance,
            instance_count: usize,
            subpixel: *const SubpixelParams,
            mode: i32,
        ) -> Status {
            unsafe {
                $draw_with(
                    renderer,
                    target,
                    geometry,
                    instances,
                    instance_count,
                    subpixel,
                    mode,
                    core::ptr::null(),
                )
            }
        }

        #[unsafe(no_mangle)]
        #[allow(clippy::too_many_arguments, reason = "the Rust call takes six; C adds a count for the slice and a null for the default projection, and splitting it would make a caller pass the same three handles twice")]
        pub unsafe extern "C" fn $draw_with(
            renderer: *const $renderer,
            target: *mut $target,
            geometry: *const $geometry,
            instances: *const GlyphInstance,
            instance_count: usize,
            subpixel: *const SubpixelParams,
            mode: i32,
            projection: *const f32,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            let Some(t) = (unsafe { gpu_mut(target) }) else { return Status::Null };
            let Some(g) = (unsafe { gpu_ref(geometry) }) else { return Status::Null };
            let Some(sp) = (unsafe { gpu_ref(subpixel) }) else { return Status::Null };
            let instances = unsafe { instances_of(instances, instance_count) };
            let mode = mode_of(mode);
            let result = match unsafe { projection_of(projection) } {
                Some(m) => r.0.draw_with(&mut t.inner, &g.inner, instances, sp, mode, &m),
                None => r.0.draw(&mut t.inner, &g.inner, instances, sp, mode),
            };
            match result {
                Ok(()) => Status::Ok,
                Err(e) => err_status(&e),
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $wait(
            renderer: *const $renderer,
            target: *mut $target,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            let Some(t) = (unsafe { gpu_mut(target) }) else { return Status::Null };
            match r.0.wait(&mut t.inner) {
                Ok(()) => Status::Ok,
                Err(e) => err_status(&e),
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $read(
            renderer: *const $renderer,
            target: *mut $target,
            out_count: *mut usize,
        ) -> *const u8 {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return core::ptr::null() };
            let Some(t) = (unsafe { gpu_mut(target) }) else { return core::ptr::null() };
            if out_count.is_null() {
                return core::ptr::null();
            }
            match r.0.read_pixels(&mut t.inner) {
                Ok(px) => {
                    unsafe { *out_count = px.len() };
                    px.as_ptr()
                }
                Err(e) => {
                    let _ = err_status(&e);
                    core::ptr::null()
                }
            }
        }
        }
    };
}

#[cfg(target_vendor = "apple")]
backend!(
    crate::paint::daegpu::ffi, metal,
    MetalRenderer, MetalTarget, MetalGeometry,
    crate::paint::daegpu::ffi::Target, crate::paint::daegpu::ffi::Geometry,
    daegun_metal_renderer_new, daegun_metal_renderer_free, daegun_metal_renderer_device_name,
    daegun_metal_renderer_profile, daegun_metal_renderer_supports_subpixel, daegun_metal_ortho,
    daegun_metal_target_new, daegun_metal_target_with_format, daegun_metal_target_set_clear,
    daegun_metal_target_width, daegun_metal_target_height,
    daegun_metal_target_pixels, daegun_metal_target_pixel, daegun_metal_target_free,
    daegun_metal_geometry_new, daegun_metal_geometry_revision, daegun_metal_geometry_free,
    daegun_metal_draw, daegun_metal_draw_with, daegun_metal_wait, daegun_metal_read_pixels,
    core::convert::identity, core::convert::identity
);

// Metal's half of the borrowed-surface work. It stays hand-written rather than joining
// `d3d_surface!`: adoption takes one handle where D3D takes two, and a drawable is a second kind of
// borrow that no other backend has.

#[cfg(target_vendor = "apple")]
#[allow(clippy::arc_with_non_send_sync, reason = "one thread per renderer, by the header's rule 5")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_metal_renderer_from_device(
    device: *mut core::ffi::c_void,
    out: *mut *mut metal::MetalRenderer,
) -> Status {
    match unsafe { crate::paint::daegpu::ffi::Renderer::from_device(device) } {
        Ok(r) => unsafe { deliver(out, metal::MetalRenderer(Arc::new(r))) },
        Err(e) => metal::err_status(&e),
    }
}

// No format argument, unlike the other three: an MTLTexture carries its own pixel format, so daegun
// reads it rather than letting the caller name one the texture disagrees with.
#[cfg(target_vendor = "apple")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_metal_target_from_texture(
    renderer: *const metal::MetalRenderer,
    texture: *mut core::ffi::c_void,
    width: u32,
    height: u32,
    out: *mut *mut metal::MetalTarget,
) -> Status {
    let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
    match unsafe { r.0.target_from_texture(texture, width, height) } {
        Ok(t) => unsafe {
            deliver(out, metal::MetalTarget { inner: t, _renderer: Arc::clone(&r.0) })
        },
        Err(e) => metal::err_status(&e),
    }
}

// daegun presents the drawable on the command buffer carrying the draw, so the caller must not
// present it as well.
#[cfg(target_vendor = "apple")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_metal_target_from_drawable(
    renderer: *const metal::MetalRenderer,
    drawable: *mut core::ffi::c_void,
    width: u32,
    height: u32,
    out: *mut *mut metal::MetalTarget,
) -> Status {
    let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
    match unsafe { r.0.target_from_drawable(drawable, width, height) } {
        Ok(t) => unsafe {
            deliver(out, metal::MetalTarget { inner: t, _renderer: Arc::clone(&r.0) })
        },
        Err(e) => metal::err_status(&e),
    }
}

// Vulkan's half of the borrowed-surface work. It stays out of `backend!` because the image comes
// in as a bare uint64 handle rather than a pointer, which no other backend does.

fn vk_format_of(format: i32) -> Option<crate::paint::daegpu::vk::Format> {
    match format {
        0 => Some(crate::paint::daegpu::vk::Format::Rgba8Unorm),
        1 => Some(crate::paint::daegpu::vk::Format::Bgra8Unorm),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_vulkan_target_from_image(
    renderer: *const vulkan::VulkanRenderer,
    image: u64,
    width: u32,
    height: u32,
    format: i32,
    out: *mut *mut vulkan::VulkanTarget,
) -> Status {
    let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
    let Some(format) = vk_format_of(format) else { return Status::Range };
    match unsafe { r.0.target_from_image(image, width, height, format) } {
        Ok(t) => {
            let inner = unsafe {
                core::mem::transmute::<
                    crate::paint::daegpu::vk::Target<'_>,
                    crate::paint::daegpu::vk::Target<'static>,
                >(t)
            };
            unsafe { deliver(out, vulkan::VulkanTarget { inner, _renderer: Arc::clone(&r.0) }) }
        }
        Err(e) => vulkan::err_status(&e),
    }
}

#[allow(clippy::arc_with_non_send_sync, reason = "one thread per renderer, by the header's rule 5")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_vulkan_renderer_from_device(
    instance: *mut core::ffi::c_void,
    physical: *mut core::ffi::c_void,
    device: *mut core::ffi::c_void,
    queue_family: u32,
    dual_src_blend: i32,
    out: *mut *mut vulkan::VulkanRenderer,
) -> Status {
    let built = unsafe {
        crate::paint::daegpu::vk::Renderer::from_device(
            instance.cast(),
            physical.cast(),
            device.cast(),
            queue_family,
            dual_src_blend != 0,
        )
    };
    match built {
        Ok(r) => unsafe { deliver(out, vulkan::VulkanRenderer(Arc::new(r))) },
        Err(e) => vulkan::err_status(&e),
    }
}

backend!(
    crate::paint::daegpu::vk, vulkan,
    VulkanRenderer, VulkanTarget, VulkanGeometry,
    crate::paint::daegpu::vk::Target<'static>, crate::paint::daegpu::vk::Geometry<'static>,
    daegun_vulkan_renderer_new, daegun_vulkan_renderer_free, daegun_vulkan_renderer_device_name,
    daegun_vulkan_renderer_profile, daegun_vulkan_renderer_supports_subpixel, daegun_vulkan_ortho,
    daegun_vulkan_target_new, daegun_vulkan_target_with_format, daegun_vulkan_target_set_clear,
    daegun_vulkan_target_width, daegun_vulkan_target_height,
    daegun_vulkan_target_pixels, daegun_vulkan_target_pixel, daegun_vulkan_target_free,
    daegun_vulkan_geometry_new, daegun_vulkan_geometry_revision, daegun_vulkan_geometry_free,
    daegun_vulkan_draw, daegun_vulkan_draw_with, daegun_vulkan_wait, daegun_vulkan_read_pixels,
    // Erasing `'r` is sound because `vk::Target<'r>` and `vk::Geometry<'r>` hold no reference into
    // the renderer – every field is a raw handle or a copied function table, and `'r` is a
    // `PhantomData` whose only job is to make `drop(renderer); drop(target)` a compile error. The
    // handle stores an `Arc<Renderer>` declared second, so field drop order frees the target first.
    |t| unsafe { core::mem::transmute::<crate::paint::daegpu::vk::Target<'_>, crate::paint::daegpu::vk::Target<'static>>(t) },
    |g| unsafe { core::mem::transmute::<crate::paint::daegpu::vk::Geometry<'_>, crate::paint::daegpu::vk::Geometry<'static>>(g) }
);

#[cfg(windows)]
backend!(
    crate::paint::daegpu::d3d11, d3d11,
    D3d11Renderer, D3d11Target, D3d11Geometry,
    crate::paint::daegpu::d3d11::Target, crate::paint::daegpu::d3d11::Geometry,
    daegun_d3d11_renderer_new, daegun_d3d11_renderer_free, daegun_d3d11_renderer_device_name,
    daegun_d3d11_renderer_profile, daegun_d3d11_renderer_supports_subpixel, daegun_d3d11_ortho,
    daegun_d3d11_target_new, daegun_d3d11_target_with_format, daegun_d3d11_target_set_clear,
    daegun_d3d11_target_width, daegun_d3d11_target_height,
    daegun_d3d11_target_pixels, daegun_d3d11_target_pixel, daegun_d3d11_target_free,
    daegun_d3d11_geometry_new, daegun_d3d11_geometry_revision, daegun_d3d11_geometry_free,
    daegun_d3d11_draw, daegun_d3d11_draw_with, daegun_d3d11_wait, daegun_d3d11_read_pixels,
    core::convert::identity, core::convert::identity
);

#[cfg(windows)]
backend!(
    crate::paint::daegpu::d3d12, d3d12,
    D3d12Renderer, D3d12Target, D3d12Geometry,
    crate::paint::daegpu::d3d12::Target, crate::paint::daegpu::d3d12::Geometry,
    daegun_d3d12_renderer_new, daegun_d3d12_renderer_free, daegun_d3d12_renderer_device_name,
    daegun_d3d12_renderer_profile, daegun_d3d12_renderer_supports_subpixel, daegun_d3d12_ortho,
    daegun_d3d12_target_new, daegun_d3d12_target_with_format, daegun_d3d12_target_set_clear,
    daegun_d3d12_target_width, daegun_d3d12_target_height,
    daegun_d3d12_target_pixels, daegun_d3d12_target_pixel, daegun_d3d12_target_free,
    daegun_d3d12_geometry_new, daegun_d3d12_geometry_revision, daegun_d3d12_geometry_free,
    daegun_d3d12_draw, daegun_d3d12_draw_with, daegun_d3d12_wait, daegun_d3d12_read_pixels,
    core::convert::identity, core::convert::identity
);

#[cfg(target_vendor = "apple")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_metal_geometry_sync(
    geometry: *mut metal::MetalGeometry,
    renderer: *const metal::MetalRenderer,
    batch: *const Batch,
) -> Status {
    let Some(g) = (unsafe { gpu_mut(geometry) }) else { return Status::Null };
    let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
    let Some(b) = (unsafe { batch_of(batch) }) else { return Status::Null };
    match g.inner.sync(&r.0, b) {
        Ok(()) => Status::Ok,
        Err(e) => {
            use crate::paint::daegpu::backend::Backend;
            let refusal = <crate::paint::daegpu::ffi::Renderer as Backend>::refusal(&e);
            fail(alloc::format!("{e}"), backend_refusal(refusal))
        }
    }
}

#[cfg(windows)]
macro_rules! d3d_extras {
    ($mod:ident, $renderer:ident, $level:ident, $software:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $level(
            renderer: *const $mod::$renderer,
            out: *mut *mut Text,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            unsafe { deliver(out, Text::new(r.0.feature_level())) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $software(
            renderer: *const $mod::$renderer,
            out: *mut i32,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            if out.is_null() {
                return Status::Null;
            }
            unsafe { *out = i32::from(r.0.is_software()) };
            Status::Ok
        }
    };
}

#[cfg(windows)]
macro_rules! d3d_surface {
    ($modname:ident, $renderer:ident, $target:ident, $from_device:ident, $from_texture:ident,
     $second:ident) => {
        // Adopts a device the caller already made, which is what lets daegun draw into that
        // device's swapchain: a backbuffer belongs to the device its swapchain was created on.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $from_device(
            device: *mut core::ffi::c_void,
            second: *mut core::ffi::c_void,
            out: *mut *mut $modname::$renderer,
        ) -> Status {
            let built = unsafe {
                crate::paint::daegpu::$modname::Renderer::from_device(device, second)
            };
            match built {
                Ok(r) => unsafe { deliver(out, $modname::$renderer(Arc::new(r))) },
                Err(e) => $modname::err_status(&e),
            }
        }

        // A target over a texture daegun did not create, such as a swapchain backbuffer.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $from_texture(
            renderer: *const $modname::$renderer,
            texture: *mut core::ffi::c_void,
            width: u32,
            height: u32,
            format: i32,
            out: *mut *mut $modname::$target,
        ) -> Status {
            let Some(r) = (unsafe { gpu_ref(renderer) }) else { return Status::Null };
            let format = match format {
                0 => crate::paint::daegpu::backend::SurfaceFormat::Rgba8Unorm,
                1 => crate::paint::daegpu::backend::SurfaceFormat::Bgra8Unorm,
                _ => return Status::Range,
            };
            match unsafe { r.0.target_from_texture(texture, width, height, format) } {
                Ok(t) => unsafe {
                    deliver(out, $modname::$target { inner: t, _renderer: Arc::clone(&r.0) })
                },
                Err(e) => $modname::err_status(&e),
            }
        }

        // Named so the macro's second handle reads as what it is at each call site.
        #[allow(dead_code)]
        const $second: () = ();
    };
}

#[cfg(windows)]
d3d_surface!(
    d3d11, D3d11Renderer, D3d11Target,
    daegun_d3d11_renderer_from_device, daegun_d3d11_target_from_texture, D3D11_SECOND_IS_CONTEXT
);

#[cfg(windows)]
d3d_surface!(
    d3d12, D3d12Renderer, D3d12Target,
    daegun_d3d12_renderer_from_device, daegun_d3d12_target_from_texture, D3D12_SECOND_IS_QUEUE
);

#[cfg(windows)]
d3d_extras!(d3d11, D3d11Renderer, daegun_d3d11_feature_level, daegun_d3d11_is_software);
#[cfg(windows)]
d3d_extras!(d3d12, D3d12Renderer, daegun_d3d12_feature_level, daegun_d3d12_is_software);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subpixel_params_from_layout(
    layout: i32,
    out: *mut SubpixelParams,
) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = SubpixelParams::from_layout(&crate::ffi::options::layout_of(layout)) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_glyph_slot_instance(
    slot: *const crate::GlyphSlot,
    offset: *const f32,
    scale: f32,
    em_pixels: *const f32,
    tint: *const f32,
    out: *mut GlyphInstance,
) -> Status {
    let Some(slot) = (unsafe { borrow(slot) }) else { return Status::Null };
    if offset.is_null() || em_pixels.is_null() || tint.is_null() || out.is_null() {
        return Status::Null;
    }
    unsafe {
        let o = core::slice::from_raw_parts(offset, 2);
        let e = core::slice::from_raw_parts(em_pixels, 2);
        let t = core::slice::from_raw_parts(tint, 4);
        *out = slot.instance(
            [o[0], o[1]],
            scale,
            [e[0], e[1]],
            [t[0], t[1], t[2], t[3]],
        );
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_from_d3d(
    software: i32,
    uma: i32,
    name: *const c_char,
    out: *mut *mut DeviceProfile,
) -> Status {
    let Some(name) = (unsafe { text_of(name) }) else { return Status::Null };
    unsafe { deliver(out, DeviceProfile::from_d3d(software != 0, tri(uma), name)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_from_metal(
    uma: i32,
    name: *const c_char,
    out: *mut *mut DeviceProfile,
) -> Status {
    let Some(name) = (unsafe { text_of(name) }) else { return Status::Null };
    unsafe { deliver(out, DeviceProfile::from_metal(tri(uma), name)) }
}

fn tri(v: i32) -> Option<bool> {
    match v {
        0 => Some(false),
        n if n > 0 => Some(true),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_device_profile_is_software(
    profile: *const DeviceProfile,
    out: *mut i32,
) -> Status {
    let Some(profile) = (unsafe { borrow(profile) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = i32::from(profile.kind.is_software()) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_glyph_slot_instance_affine(
    slot: *const crate::GlyphSlot,
    offset: *const f32,
    scale: f32,
    transform: *const f32,
    tint: *const f32,
    out: *mut GlyphInstance,
) -> Status {
    let Some(slot) = (unsafe { borrow(slot) }) else { return Status::Null };
    if offset.is_null() || transform.is_null() || tint.is_null() || out.is_null() {
        return Status::Null;
    }
    unsafe {
        let o = core::slice::from_raw_parts(offset, 2);
        let m = core::slice::from_raw_parts(transform, 4);
        let t = core::slice::from_raw_parts(tint, 4);
        *out = slot.instance_affine(
            [o[0], o[1]],
            scale,
            [m[0], m[1], m[2], m[3]],
            [t[0], t[1], t[2], t[3]],
        );
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subpixel_params_dilation(
    params: *const SubpixelParams,
    out: *mut f32,
) -> Status {
    let Some(params) = (unsafe { borrow(params) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let d = params.dilation();
    unsafe { core::slice::from_raw_parts_mut(out, 2) }.copy_from_slice(&d);
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subpixel_params_with_supersampling(
    params: *const SubpixelParams,
    n: u32,
    out: *mut SubpixelParams,
) -> Status {
    let Some(params) = (unsafe { borrow(params) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = params.with_supersampling(n) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subpixel_params_from_weights(
    oversample_x: u8,
    oversample_y: u8,
    taps_x: u8,
    taps_y: u8,
    origin_x: i8,
    origin_y: i8,
    weights: *const f32,
    out: *mut SubpixelParams,
) -> Status {
    if weights.is_null() || out.is_null() {
        return Status::Null;
    }
    let per = usize::from(taps_x) * usize::from(taps_y);
    if per == 0 {
        return Status::Range;
    }
    let all = unsafe { core::slice::from_raw_parts(weights, per * 3) };
    let Some(layout) = crate::SubpixelLayout::from_weights(
        (oversample_x, oversample_y),
        (taps_x, taps_y),
        (origin_x, origin_y),
        [&all[..per], &all[per..per * 2], &all[per * 2..]],
    ) else {
        return Status::Range;
    };
    unsafe { *out = SubpixelParams::from_layout(&layout) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subpixel_layout_key(layout: i32, out: *mut u64) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = crate::ffi::options::layout_of(layout).key() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subpixel_params_pad(
    params: *const SubpixelParams,
    out: *mut usize,
) -> Status {
    let Some(p) = (unsafe { borrow(params) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let per = (p.taps[0] as usize) * (p.taps[1] as usize);
    let rows = p.weights.chunks(crate::MAX_SUBPIXEL_WEIGHTS);
    let w: alloc::vec::Vec<&[f32]> = rows.map(|r| &r[..per.min(r.len())]).collect();
    if w.len() < 3 {
        return Status::Range;
    }
    let Ok(oversample) = u8::try_from(p.oversample[0]).and_then(|x| {
        u8::try_from(p.oversample[1]).map(|y| (x, y))
    }) else {
        return Status::Range;
    };
    let Ok(taps) = u8::try_from(p.taps[0]).and_then(|x| u8::try_from(p.taps[1]).map(|y| (x, y)))
    else {
        return Status::Range;
    };
    let Ok(origin) = i8::try_from(p.origin[0]).and_then(|x| i8::try_from(p.origin[1]).map(|y| (x, y)))
    else {
        return Status::Range;
    };
    let Some(layout) =
        crate::SubpixelLayout::from_weights(oversample, taps, origin, [w[0], w[1], w[2]])
    else {
        return Status::Range;
    };
    let (px, py) = layout.pad();
    unsafe { core::slice::from_raw_parts_mut(out, 2) }.copy_from_slice(&[px, py]);
    Status::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_vendor = "apple")]
    use metal::{
        daegun_metal_renderer_free, daegun_metal_renderer_new, daegun_metal_target_free,
        daegun_metal_target_new, daegun_metal_target_pixels, daegun_metal_target_width,
    };
    use vulkan::{
        daegun_vulkan_renderer_free, daegun_vulkan_renderer_new, daegun_vulkan_target_free,
        daegun_vulkan_target_new, daegun_vulkan_target_pixels, daegun_vulkan_target_width,
    };

    #[test]
    fn a_target_outlives_the_renderer_it_came_from() {
        let mut ran = 0;

        #[cfg(target_vendor = "apple")]
        {
            let mut r: *mut metal::MetalRenderer = core::ptr::null_mut();
            if unsafe { daegun_metal_renderer_new(&mut r) } == Status::Ok {
                let mut t: *mut metal::MetalTarget = core::ptr::null_mut();
                assert_eq!(unsafe { daegun_metal_target_new(r, 32, 32, &mut t) }, Status::Ok);
                unsafe { daegun_metal_renderer_free(r) };
                let mut w = 0u32;
                assert_eq!(unsafe { daegun_metal_target_width(t, &mut w) }, Status::Ok);
                assert_eq!(w, 32, "the target forgot its width when the renderer went");
                let mut n = 0usize;
                let px = unsafe { daegun_metal_target_pixels(t, &mut n) };
                assert!(!px.is_null() && n > 0, "the pixels went with the renderer");
                let _sum: u64 = unsafe { core::slice::from_raw_parts(px, n) }
                    .iter()
                    .map(|b| u64::from(*b))
                    .sum();
                unsafe { daegun_metal_target_free(t) };
                ran += 1;
            }
        }

        let mut r: *mut vulkan::VulkanRenderer = core::ptr::null_mut();
        if unsafe { daegun_vulkan_renderer_new(&mut r) } == Status::Ok {
            let mut t: *mut vulkan::VulkanTarget = core::ptr::null_mut();
            assert_eq!(unsafe { daegun_vulkan_target_new(r, 32, 32, &mut t) }, Status::Ok);
            unsafe { daegun_vulkan_renderer_free(r) };
            let mut w = 0u32;
            assert_eq!(unsafe { daegun_vulkan_target_width(t, &mut w) }, Status::Ok);
            assert_eq!(w, 32, "the target forgot its width when the renderer went");
            let mut n = 0usize;
            let px = unsafe { daegun_vulkan_target_pixels(t, &mut n) };
            assert!(!px.is_null() && n > 0, "the pixels went with the renderer");
            let _sum: u64 = unsafe { core::slice::from_raw_parts(px, n) }
                .iter()
                .map(|b| u64::from(*b))
                .sum();
            unsafe { daegun_vulkan_target_free(t) };
            ran += 1;
        }

        assert!(ran > 0, "no GPU backend opened, so the ordering this exists to check went untested");
    }
}
