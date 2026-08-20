// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Anything the caller must uphold beyond that is noted at its site.

use alloc::vec::Vec;

use crate::{DrawTarget, DrawnGlyph, Font, GpuBatch, Policy, Prefer};

use crate::ffi::handle::{Status, borrow, deliver, release};
use crate::ffi::list::{Axis, axes_of};
use crate::ffi::options::RasterOptionsC;
use crate::ffi::raster::Bitmap;

pub struct Batch(GpuBatch);

impl Batch {
    pub fn inner(&self) -> &GpuBatch {
        &self.0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_batch_new(out: *mut *mut Batch) -> Status {
    unsafe { deliver(out, Batch(GpuBatch::new())) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_batch_clear(batch: *mut Batch) -> Status {
    if batch.is_null() {
        return Status::Null;
    }
    unsafe { (*batch).0.clear() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_batch_append(
    batch: *mut Batch,
    quads: *const f32,
    count: usize,
    out: *mut crate::GlyphSlot,
) -> Status {
    if batch.is_null() || quads.is_null() || out.is_null() {
        return Status::Null;
    }
    let Some(floats) = count.checked_mul(6) else { return Status::Range };
    let src = unsafe { core::slice::from_raw_parts(quads, floats) };
    let mut curves: Vec<[[f32; 2]; 3]> = src
        .chunks_exact(6)
        .map(|q| [[q[0], q[1]], [q[2], q[3]], [q[4], q[5]]])
        .collect();
    match unsafe { (*batch).0.append(&mut curves) } {
        Some(slot) => {
            unsafe { *out = slot };
            Status::Ok
        }
        None => Status::Range,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_batch_revision(batch: *const Batch, out: *mut u64) -> Status {
    let Some(b) = (unsafe { borrow(batch) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = b.0.revision() };
    Status::Ok
}

macro_rules! batch_view {
    ($fn_name:ident, $method:ident, $elem:ty, $doc:literal) => {
        #[doc = $doc]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $fn_name(
            batch: *const Batch,
            out_count: *mut usize,
        ) -> *const $elem {
            let Some(b) = (unsafe { borrow(batch) }) else { return core::ptr::null() };
            if out_count.is_null() {
                return core::ptr::null();
            }
            let slice = b.0.$method();
            unsafe { *out_count = slice.len() };
            slice.as_ptr()
        }
    };
}

batch_view!(
    daegun_batch_curves,
    curves,
    crate::CurvePoint,
    "The curve points every glyph in the batch contributed."
);
batch_view!(
    daegun_batch_band_curves,
    band_curves,
    u32,
    "Which curves each band holds, as indices into the curve points."
);
batch_view!(daegun_batch_bands, bands, crate::Band, "The bands, horizontal then vertical.");
batch_view!(
    daegun_batch_hulls,
    hulls,
    crate::HullVertex,
    "The drawn polygon of every glyph, as vertices."
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_batch_free(batch: *mut Batch) {
    unsafe { release(batch) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_gpu_glyph(
    font: *const Font,
    batch: *mut Batch,
    gid: u16,
    axes: *const Axis,
    axes_len: usize,
    out: *mut crate::GlyphSlot,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if batch.is_null() || out.is_null() {
        return Status::Null;
    }
    let location = unsafe { axes_of(axes, axes_len) };
    match unsafe { font.gpu_glyph(&mut (*batch).0, gid, &location) } {
        Ok(slot) => {
            unsafe { *out = slot };
            Status::Ok
        }
        Err(e) => {
            crate::ffi::set_error(&alloc::format!("{e:?}"));
            Status::Absent
        }
    }
}

pub struct ColorSlots(Vec<crate::ColorSlot>);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_gpu_color_glyph(
    font: *const Font,
    batch: *mut Batch,
    gid: u16,
    axes: *const Axis,
    axes_len: usize,
    palette_index: u16,
    out: *mut *mut ColorSlots,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if batch.is_null() {
        return Status::Null;
    }
    let location = unsafe { axes_of(axes, axes_len) };
    match unsafe { font.gpu_color_glyph(&mut (*batch).0, gid, &location, palette_index) } {
        Ok(slots) => unsafe { deliver(out, ColorSlots(slots)) },
        Err(e) => {
            crate::ffi::set_error(&alloc::format!("{e:?}"));
            Status::Absent
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_color_slots_data(
    slots: *const ColorSlots,
    out_count: *mut usize,
) -> *const crate::ColorSlot {
    let Some(s) = (unsafe { borrow(slots) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = s.0.len() };
    s.0.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_color_slots_free(slots: *mut ColorSlots) {
    unsafe { release(slots) }
}

pub const PREFER_AUTO: i32 = 0;
pub const PREFER_CPU: i32 = 1;
pub const PREFER_GPU: i32 = 2;
pub const PREFER_REFERENCE: i32 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PolicyC {
    pub prefer: i32,
    pub strict: bool,
    pub has_cpu_below_ppem: bool,
    pub cpu_below_ppem: f32,
    pub avoid_software_gpu: bool,
}
const _: () = assert!(size_of::<PolicyC>() == 16);

impl PolicyC {
    pub fn to_rust(self) -> Policy {
        Policy {
            prefer: match self.prefer {
                PREFER_CPU => Prefer::Cpu,
                PREFER_GPU => Prefer::Gpu,
                PREFER_REFERENCE => Prefer::Reference,
                _ => Prefer::Auto,
            },
            strict: self.strict,
            cpu_below_ppem: self.has_cpu_below_ppem.then_some(self.cpu_below_ppem),
            avoid_software_gpu: self.avoid_software_gpu,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_policy_default(out: *mut PolicyC) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe {
        *out = PolicyC {
            prefer: PREFER_AUTO,
            strict: false,
            has_cpu_below_ppem: false,
            cpu_below_ppem: 0.0,
            avoid_software_gpu: false,
        }
    };
    Status::Ok
}

pub const DRAWN_NOTHING: i32 = 0;
pub const DRAWN_CPU: i32 = 1;
pub const DRAWN_GPU: i32 = 2;
pub const DRAWN_GPU_COLOR: i32 = 3;
pub const DRAWN_SCENE: i32 = 4;
pub const DRAWN_REFERENCE: i32 = 5;
pub const DRAWN_BATCH_FULL: i32 = 6;
pub const DRAWN_REFUSED: i32 = 7;

pub struct Drawn {
    kind: i32,
    bitmap: Option<Bitmap>,
    slot: Option<crate::GlyphSlot>,
    color_slots: Vec<crate::ColorSlot>,
    scene: Option<crate::ffi::color::Scene>,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_draw_glyph(
    font: *const Font,
    batch: *mut Batch,
    device: *const crate::DeviceProfile,
    policy: *const PolicyC,
    gid: u16,
    px: f32,
    axes: *const Axis,
    axes_len: usize,
    opts: *const RasterOptionsC,
    palette: i32,
    out: *mut *mut Drawn,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if batch.is_null() {
        return Status::Null;
    }
    let location = unsafe { axes_of(axes, axes_len) };
    let options = match unsafe { borrow(opts) } {
        Some(o) => o.to_rust(),
        None => RasterOptionsC::DEFAULT.to_rust(),
    };
    let pal = if palette < 0 { None } else { u16::try_from(palette).ok() };

    let built_policy =
        unsafe { borrow(policy) }.map_or_else(Policy::default, |p| p.to_rust());

    let drawn = unsafe {
        let batch = &mut (*batch).0;
        let mut target = match borrow(device) {
            Some(d) => DrawTarget::new(batch, d),
            None => DrawTarget::cpu_only(batch),
        }
        .with_policy(built_policy);
        font.draw_glyph(&mut target, gid, px, &location, &options, pal)
    };

    let built = match drawn {
        DrawnGlyph::Nothing => Drawn::empty(DRAWN_NOTHING),
        DrawnGlyph::Cpu(g) => Drawn { bitmap: Some(crate::ffi::raster::wrap(g)), ..Drawn::empty(DRAWN_CPU) },
        DrawnGlyph::Reference(g) => {
            Drawn { bitmap: Some(crate::ffi::raster::wrap(g)), ..Drawn::empty(DRAWN_REFERENCE) }
        }
        DrawnGlyph::Gpu(slot) => Drawn { slot: Some(slot), ..Drawn::empty(DRAWN_GPU) },
        DrawnGlyph::GpuColor(slots) => {
            Drawn { color_slots: slots, ..Drawn::empty(DRAWN_GPU_COLOR) }
        }
        DrawnGlyph::Scene(s) => {
            Drawn { scene: Some(crate::ffi::color::wrap_scene(s)), ..Drawn::empty(DRAWN_SCENE) }
        }
        DrawnGlyph::BatchFull => Drawn::empty(DRAWN_BATCH_FULL),
        DrawnGlyph::Refused(r) => {
            crate::ffi::set_error(&alloc::format!("{r:?}"));
            Drawn::empty(DRAWN_REFUSED)
        }
    };
    unsafe { deliver(out, built) }
}

impl Drawn {
    fn empty(kind: i32) -> Drawn {
        Drawn { kind, bitmap: None, slot: None, color_slots: Vec::new(), scene: None }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_drawn_kind(d: *const Drawn, out: *mut i32) -> Status {
    let Some(d) = (unsafe { borrow(d) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = d.kind };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_drawn_is_ok(d: *const Drawn, out: *mut bool) -> Status {
    let Some(d) = (unsafe { borrow(d) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = d.kind != DRAWN_REFUSED && d.kind != DRAWN_BATCH_FULL };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_drawn_bitmap(
    d: *const Drawn,
    out: *mut *const Bitmap,
) -> Status {
    let Some(d) = (unsafe { borrow(d) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(b) = &d.bitmap else { return Status::Absent };
    unsafe { *out = &raw const *b };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_drawn_slot(
    d: *const Drawn,
    out: *mut crate::GlyphSlot,
) -> Status {
    let Some(d) = (unsafe { borrow(d) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(s) = d.slot else { return Status::Absent };
    unsafe { *out = s };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_drawn_color_slots(
    d: *const Drawn,
    out_count: *mut usize,
) -> *const crate::ColorSlot {
    let Some(d) = (unsafe { borrow(d) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = d.color_slots.len() };
    d.color_slots.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_drawn_scene(
    d: *const Drawn,
    out: *mut *const crate::ffi::color::Scene,
) -> Status {
    let Some(d) = (unsafe { borrow(d) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(s) = &d.scene else { return Status::Absent };
    unsafe { *out = &raw const *s };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_drawn_free(d: *mut Drawn) {
    unsafe { release(d) }
}
