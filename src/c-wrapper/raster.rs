// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Where a call takes a raw buffer, its length is the caller's promise
// from `daegun.h` and is not checkable here.

use alloc::vec::Vec;

use crate::{Font, HintMode};

use crate::ffi::handle::{Status, borrow, deliver, release};
use crate::ffi::list::{Axis, axes_of};
use crate::ffi::options::RasterOptionsC;
use crate::ffi::pen::{Pen, PenBridge};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_outline_glyph(
    font: *const Font,
    gid: u16,
    pen: *const Pen,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(pen) = (unsafe { borrow(pen) }) else { return Status::Null };
    let mut bridge = PenBridge(*pen);
    match font.outline_glyph(gid, &mut bridge) {
        Some(()) => Status::Ok,
        None => Status::Absent,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_outline_glyph_instanced(
    font: *const Font,
    gid: u16,
    axes: *const Axis,
    axes_len: usize,
    pen: *const Pen,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(pen) = (unsafe { borrow(pen) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let mut bridge = PenBridge(*pen);
    match font.outline_glyph_instanced(gid, &location, &mut bridge) {
        Some(()) => Status::Ok,
        None => Status::Absent,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_prewarm(
    font: *const Font,
    gids: *const u16,
    gids_len: usize,
    axes: *const Axis,
    axes_len: usize,
    out_added: *mut usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if gids.is_null() && gids_len != 0 {
        return Status::Null;
    }
    let ids =
        if gids_len == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(gids, gids_len) } };
    let location = unsafe { axes_of(axes, axes_len) };
    let added = font.prewarm(ids.iter().copied(), &location);
    if !out_added.is_null() {
        unsafe { *out_added = added };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_clear_prewarm(font: *const Font) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.clear_prewarm();
    Status::Ok
}

pub struct Bitmap {
    metrics: MetricsC,
    pixels: Vec<u8>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MetricsC {
    pub xmin: i32,
    pub ymin: i32,
    pub width: usize,
    pub height: usize,
    pub advance_width: f32,
    pub advance_height: f32,
    pub bounds_xmin: f32,
    pub bounds_ymin: f32,
    pub bounds_width: f32,
    pub bounds_height: f32,
}
const _: () = assert!(size_of::<MetricsC>() == 48);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bitmap_metrics(
    bitmap: *const Bitmap,
    out: *mut MetricsC,
) -> Status {
    let Some(b) = (unsafe { borrow(bitmap) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = b.metrics };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bitmap_pixels(
    bitmap: *const Bitmap,
    out_len: *mut usize,
) -> *const u8 {
    let Some(b) = (unsafe { borrow(bitmap) }) else { return core::ptr::null() };
    if out_len.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_len = b.pixels.len() };
    b.pixels.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bitmap_free(bitmap: *mut Bitmap) {
    unsafe { release(bitmap) }
}

pub(crate) fn wrap(g: crate::RasterizedGlyph) -> Bitmap {
    let m = g.metrics;
    Bitmap {
        metrics: MetricsC {
            xmin: m.xmin,
            ymin: m.ymin,
            width: m.width,
            height: m.height,
            advance_width: m.advance_width,
            advance_height: m.advance_height,
            bounds_xmin: m.bounds.xmin,
            bounds_ymin: m.bounds.ymin,
            bounds_width: m.bounds.width,
            bounds_height: m.bounds.height,
        },
        pixels: g.bitmap,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_rasterize_glyph(
    font: *const Font,
    gid: u16,
    px: f32,
    axes: *const Axis,
    axes_len: usize,
    out: *mut *mut Bitmap,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let Some(g) = font.rasterize_glyph(gid, px, &location) else { return Status::Absent };
    unsafe { deliver(out, wrap(g)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_rasterize_glyph_with(
    font: *const Font,
    gid: u16,
    px: f32,
    axes: *const Axis,
    axes_len: usize,
    opts: *const RasterOptionsC,
    out: *mut *mut Bitmap,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let options = match unsafe { borrow(opts) } {
        Some(o) => o.to_rust(),
        None => RasterOptionsC::DEFAULT.to_rust(),
    };
    let Some(g) = font.rasterize_glyph_with(gid, px, &location, &options) else {
        return Status::Absent;
    };
    unsafe { deliver(out, wrap(g)) }
}

pub struct HintedOutline(crate::HintedOutline);

impl HintedOutline {
    pub fn inner(&self) -> &crate::HintedOutline {
        &self.0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_hinted_glyph(
    font: *const Font,
    gid: u16,
    px: f32,
    axes: *const Axis,
    axes_len: usize,
    hint_mode: i32,
    out: *mut *mut HintedOutline,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let mode = match hint_mode {
        crate::ffi::options::HINT_SUBPIXEL => HintMode::Subpixel,
        crate::ffi::options::HINT_CLASSIC => HintMode::Classic,
        crate::ffi::options::HINT_AUTO => HintMode::Auto,
        crate::ffi::options::HINT_AUTO_FORCE => HintMode::AutoForce,
        _ => HintMode::None,
    };
    let Some(h) = font.hinted_glyph(gid, px, &location, mode) else { return Status::Absent };
    unsafe {
        deliver(
            out,
            HintedOutline(h),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_hinted_outline_points(
    outline: *const HintedOutline,
    out_count: *mut usize,
    out_x: *mut *const i32,
    out_y: *mut *const i32,
    out_flags: *mut *const u8,
) -> Status {
    let Some(o) = (unsafe { borrow(outline) }) else { return Status::Null };
    if out_count.is_null() {
        return Status::Null;
    }
    unsafe {
        *out_count = o.0.x.len();
        if !out_x.is_null() {
            *out_x = o.0.x.as_ptr();
        }
        if !out_y.is_null() {
            *out_y = o.0.y.as_ptr();
        }
        if !out_flags.is_null() {
            *out_flags = o.0.flags.as_ptr();
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_hinted_outline_contours(
    outline: *const HintedOutline,
    out_count: *mut usize,
) -> *const usize {
    let Some(o) = (unsafe { borrow(outline) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = o.0.contour_ends.len() };
    o.0.contour_ends.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_hinted_outline_free(outline: *mut HintedOutline) {
    unsafe { release(outline) }
}

pub struct CffHints {
    stems: Vec<f64>,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_cff_hints(
    font: *const Font,
    gid: u16,
    out: *mut *mut CffHints,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(h) = font.cff_hints(gid) else { return Status::Absent };
    let stems: Vec<f64> = h
        .stems
        .iter()
        .flat_map(|s| [f64::from(u8::from(s.vertical)), f64::from(s.min), f64::from(s.max)])
        .collect();
    unsafe { deliver(out, CffHints { stems }) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_cff_hints_stems(
    hints: *const CffHints,
    out_count: *mut usize,
) -> *const f64 {
    let Some(h) = (unsafe { borrow(hints) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = h.stems.len() / 3 };
    h.stems.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_cff_hints_free(hints: *mut CffHints) {
    unsafe { release(hints) }
}
