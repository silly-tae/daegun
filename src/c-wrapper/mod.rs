// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Where a call takes a raw buffer, its length is the caller's promise
// from `daegun.h` and is not checkable here.

#![allow(unsafe_code)]

mod handle;
mod atlas;
mod color;
mod draw;
mod glyphs;
mod gpu;
mod layout;
mod layout_check;
mod list;
mod metrics;
mod tables;
mod options;
mod outline;
mod pen;
mod raster;
mod raw;
mod shape;

use core::ffi::c_char;
use crate::Font;
use handle::{Status, Str, borrow, deliver, release};
use options::RasterOptionsC;

#[unsafe(no_mangle)]
pub extern "C" fn daegun_abi_version() -> u32 {
    // Read from the crate version rather than restated here, because a hand-kept copy drifts out
    // of step with the release it is meant to describe.
    const fn num(s: &str) -> u32 {
        let (b, mut v, mut i) = (s.as_bytes(), 0u32, 0usize);
        while i < b.len() {
            v = v * 10 + (b[i] - b'0') as u32;
            i += 1;
        }
        v
    }
    (num(env!("CARGO_PKG_VERSION_MAJOR")) << 16)
        | (num(env!("CARGO_PKG_VERSION_MINOR")) << 8)
        | num(env!("CARGO_PKG_VERSION_PATCH"))
}

std::thread_local! {
    static LAST_ERROR: core::cell::RefCell<Option<handle::OwnedStr>> =
        const { core::cell::RefCell::new(None) };
}

// A NULL foreground means the caller did not name one, matching how opts, policy and device are
// already allowed to be NULL here.
pub(crate) unsafe fn rgba_of(p: *const u8) -> Option<crate::daerizer::Rgba> {
    if p.is_null() {
        return None;
    }
    let c = unsafe { core::slice::from_raw_parts(p, 4) };
    Some(crate::daerizer::Rgba { r: c[0], g: c[1], b: c[2], a: c[3] })
}

pub(crate) fn set_error(message: &str) {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(handle::OwnedStr::new(message)));
}

#[unsafe(no_mangle)]
pub extern "C" fn daegun_last_error() -> Str {
    LAST_ERROR.with(|slot| slot.borrow().as_ref().map_or(Str::EMPTY, handle::OwnedStr::as_str))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_open(
    data: *const u8,
    len: usize,
    out: *mut *mut Font,
) -> Status {
    if data.is_null() || out.is_null() {
        return Status::Null;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    match Font::from_bytes(bytes) {
        Ok(font) => unsafe { deliver(out, font) },
        Err(e) => {
            set_error(&alloc::format!("{e}"));
            Status::Parse
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn daegun_font_buffer_new(len: usize) -> *mut u8 {
    if len == 0 {
        return core::ptr::null_mut();
    }
    let mut v = alloc::vec![0u8; len];
    debug_assert_eq!(v.capacity(), len, "the buffer must be exactly its length to be reclaimed");
    let ptr = v.as_mut_ptr();
    core::mem::forget(v);
    ptr
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_buffer_free(data: *mut u8, len: usize) {
    if data.is_null() || len == 0 {
        return;
    }
    // `daegun_font_buffer_new` is the only source and allocates capacity exactly `len`, which is
    // what makes reconstructing the Vec sound. A pointer from anywhere else is undefined behavior.
    drop(unsafe { alloc::vec::Vec::from_raw_parts(data, len, len) });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_open_owned(
    data: *mut u8,
    len: usize,
    out: *mut *mut Font,
) -> Status {
    if data.is_null() || len == 0 || out.is_null() {
        return Status::Null;
    }
    let bytes = unsafe { alloc::vec::Vec::from_raw_parts(data, len, len) };
    match Font::from_vec(bytes) {
        Ok(font) => unsafe { deliver(out, font) },
        Err(e) => {
            set_error(&alloc::format!("{e}"));
            Status::Parse
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_open_collection(
    data: *const u8,
    len: usize,
    index: usize,
    out: *mut *mut Font,
) -> Status {
    if data.is_null() || out.is_null() {
        return Status::Null;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    match Font::from_ttc(bytes, index) {
        Ok(font) => unsafe { deliver(out, font) },
        Err(e) => {
            set_error(&alloc::format!("{e}"));
            Status::Parse
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_free(font: *mut Font) {
    unsafe { release(font) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_glyph_id(
    font: *const Font,
    codepoint: u32,
    out: *mut u16,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    match font.glyph_id(codepoint) {
        Some(gid) => {
            unsafe { *out = gid };
            Status::Ok
        }
        None => Status::Absent,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_num_glyphs(font: *const Font, out: *mut u16) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = font.num_glyphs() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_upm(font: *const Font, out: *mut u16) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = font.upm() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_ttc_font_count(
    data: *const u8,
    len: usize,
    out: *mut usize,
) -> Status {
    if data.is_null() || out.is_null() {
        return Status::Null;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    unsafe { *out = Font::ttc_font_count(bytes) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_set_glyph_cache_bytes(
    font: *const Font,
    bytes: usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.set_glyph_cache_bytes(bytes);
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_clear_glyph_cache(font: *const Font) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.clear_glyph_cache();
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_glyph_cache_stats(
    font: *const Font,
    out_count: *mut usize,
    out_bytes: *mut usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let (count, bytes) = font.glyph_cache_stats();
    if !out_count.is_null() {
        unsafe { *out_count = count };
    }
    if !out_bytes.is_null() {
        unsafe { *out_bytes = bytes };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_set_curve_cache_bytes(font: *const Font, bytes: usize) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.set_curve_cache_bytes(bytes);
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_clear_curve_cache(font: *const Font) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.clear_curve_cache();
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_curve_cache_stats(
    font: *const Font,
    out_count: *mut usize,
    out_bytes: *mut usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let (count, bytes) = font.curve_cache_stats();
    if !out_count.is_null() {
        unsafe { *out_count = count };
    }
    if !out_bytes.is_null() {
        unsafe { *out_bytes = bytes };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_set_outline_cache_bytes(font: *const Font, bytes: usize) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.set_outline_cache_bytes(bytes);
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_outline_cache_stats(
    font: *const Font,
    out_count: *mut usize,
    out_bytes: *mut usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let (count, bytes) = font.outline_cache_stats();
    if !out_count.is_null() {
        unsafe { *out_count = count };
    }
    if !out_bytes.is_null() {
        unsafe { *out_bytes = bytes };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_set_shape_cache_bytes(font: *const Font, bytes: usize) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.set_shape_cache_bytes(bytes);
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_clear_shape_cache(font: *const Font) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.clear_shape_cache();
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape_cache_stats(
    font: *const Font,
    out_count: *mut usize,
    out_bytes: *mut usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let (count, bytes) = font.shape_cache_stats();
    if !out_count.is_null() {
        unsafe { *out_count = count };
    }
    if !out_bytes.is_null() {
        unsafe { *out_bytes = bytes };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_set_instance_cache_bytes(font: *const Font, bytes: usize) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.set_instance_cache_bytes(bytes);
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_instance_cache_stats(
    font: *const Font,
    out_locations: *mut usize,
    out_tables: *mut usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let (locations, tables) = font.instance_cache_stats();
    if !out_locations.is_null() {
        unsafe { *out_locations = locations };
    }
    if !out_tables.is_null() {
        unsafe { *out_tables = tables };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_set_cmap_index_allowance(font: *const Font, bytes: usize) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    font.set_cmap_index_allowance(bytes);
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_cmap_index_allowance(
    font: *const Font,
    out_bytes: *mut usize,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if !out_bytes.is_null() {
        unsafe { *out_bytes = font.cmap_index_allowance() };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_raster_options_default(out: *mut RasterOptionsC) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = RasterOptionsC::DEFAULT };
    Status::Ok
}

const _: () = {
    assert!(size_of::<Status>() == 4);
    assert!(align_of::<Status>() == 4);
    assert!(size_of::<*mut Font>() == size_of::<usize>());
    assert!(size_of::<RasterOptionsC>() == 80);
};

const _: Option<*const c_char> = None;
