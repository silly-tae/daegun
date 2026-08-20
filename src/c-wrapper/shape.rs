// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Where a call takes a raw buffer, its length is the caller's promise
// from `daegun.h` and is not checkable here.

use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

use crate::{ClusterLevel, Font, Ignorables, ShapeOptions};

use crate::ffi::handle::{OwnedStr, Status, Str, borrow, deliver, release};
use crate::ffi::list::{Axis, axes_of};

pub(crate) unsafe fn str_of<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(s) }.to_str().ok()
}

pub struct Run {
    glyphs: Vec<u16>,
    advances: Vec<f64>,
    offsets: Vec<f64>,
    clusters: Vec<u32>,
    unsafe_to_break: Vec<u8>,
    unsafe_to_concat: Vec<u8>,
    safe_to_insert_tatweel: Vec<u8>,
    complete: bool,
    has_broken_syllable: bool,
    shaper: OwnedStr,
}

impl Run {
    pub(crate) fn of(r: &crate::ShapedRun) -> Run {
        Run {
            glyphs: r.glyphs.clone(),
            advances: r.advances.clone(),
            offsets: r.offsets.iter().flat_map(|(x, y)| [*x, *y]).collect(),
            clusters: r.clusters.clone(),
            unsafe_to_break: r.unsafe_to_break.iter().map(|b| u8::from(*b)).collect(),
            unsafe_to_concat: r.unsafe_to_concat.iter().map(|b| u8::from(*b)).collect(),
            safe_to_insert_tatweel: r
                .safe_to_insert_tatweel
                .iter()
                .map(|b| u8::from(*b))
                .collect(),
            complete: r.complete,
            has_broken_syllable: r.has_broken_syllable,
            shaper: OwnedStr::new(r.shaper),
        }
    }
}

macro_rules! run_view {
    ($fn_name:ident, $field:ident, $elem:ty, $doc:literal) => {
        #[doc = $doc]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $fn_name(run: *const Run, out_count: *mut usize) -> *const $elem {
            let Some(r) = (unsafe { borrow(run) }) else { return core::ptr::null() };
            if out_count.is_null() {
                return core::ptr::null();
            }
            unsafe { *out_count = r.$field.len() };
            r.$field.as_ptr()
        }
    };
}

run_view!(daegun_run_glyphs, glyphs, u16, "The glyph ids, in visual order.");
run_view!(daegun_run_advances, advances, f64, "How far each glyph advances the pen.");
run_view!(
    daegun_run_offsets,
    offsets,
    f64,
    "Two doubles per glyph — x then y — so glyph `i` is at `2 * i`. `out_count` is the number of \
     doubles, not of glyphs."
);
run_view!(
    daegun_run_clusters,
    clusters,
    u32,
    "Which byte of the input each glyph came from. Several glyphs may share a cluster and one glyph \
     may span several characters."
);
run_view!(
    daegun_run_unsafe_to_break,
    unsafe_to_break,
    u8,
    "Non-zero where breaking the run before this glyph would change the shaping."
);
run_view!(
    daegun_run_unsafe_to_concat,
    unsafe_to_concat,
    u8,
    "Non-zero where joining another run here would change the shaping."
);
run_view!(
    daegun_run_safe_to_insert_tatweel,
    safe_to_insert_tatweel,
    u8,
    "Non-zero where an Arabic tatweel may be inserted without disturbing the shaping."
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_run_complete(run: *const Run, out: *mut bool) -> Status {
    let Some(r) = (unsafe { borrow(run) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = r.complete };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_run_has_broken_syllable(
    run: *const Run,
    out: *mut bool,
) -> Status {
    let Some(r) = (unsafe { borrow(run) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = r.has_broken_syllable };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_run_shaper(run: *const Run, out: *mut Str) -> Status {
    let Some(r) = (unsafe { borrow(run) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = r.shaper.as_str() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_run_free(run: *mut Run) {
    unsafe { release(run) }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Feature {
    pub tag: *const c_char,
    pub value: u32,
}
const _: () = assert!(size_of::<Feature>() == 16);

unsafe fn features_of<'a>(features: *const Feature, len: usize) -> Vec<(&'a str, u32)> {
    if features.is_null() || len == 0 {
        return Vec::new();
    }
    let slice = unsafe { core::slice::from_raw_parts(features, len) };
    slice.iter().filter_map(|f| unsafe { str_of(f.tag) }.map(|t| (t, f.value))).collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    vertical: bool,
    out: *mut *mut Run,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let Some(r) = font.shape(text, &location, vertical) else { return Status::Absent };
    unsafe { deliver(out, Run::of(&r)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape_with_language(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    vertical: bool,
    language: *const c_char,
    out: *mut *mut Run,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let Some(language) = (unsafe { str_of(language) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let Some(r) = font.shape_with_language(text, &location, vertical, language) else {
        return Status::Absent;
    };
    unsafe { deliver(out, Run::of(&r)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape_with_features(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    vertical: bool,
    script: *const c_char,
    features: *const Feature,
    features_len: usize,
    out: *mut *mut Run,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let script = unsafe { str_of(script) };
    let feats = unsafe { features_of(features, features_len) };
    let Some(r) = font.shape_with_features(text, &location, vertical, script, &feats) else {
        return Status::Absent;
    };
    unsafe { deliver(out, Run::of(&r)) }
}

pub const CLUSTER_MONOTONE_GRAPHEMES: i32 = 0;
pub const CLUSTER_MONOTONE_CHARACTERS: i32 = 1;
pub const CLUSTER_CHARACTERS: i32 = 2;
pub const CLUSTER_GRAPHEMES: i32 = 3;

pub const IGNORABLES_HIDE: i32 = 0;
pub const IGNORABLES_REMOVE: i32 = 1;
pub const IGNORABLES_PRESERVE: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShapeOptionsC {
    pub cluster_level: i32,
    pub ignorables: i32,
    pub before: *const c_char,
    pub after: *const c_char,
    pub beginning_of_text: bool,
    pub has_point_size: bool,
    pub point_size: f64,
    pub features: *const Feature,
    pub features_len: usize,
    pub script: *const c_char,
    pub language: *const c_char,
    pub report_unsafe_to_concat: bool,
    pub report_tatweel_positions: bool,
    pub suppress_dotted_circle: bool,
    pub has_invisible_glyph: bool,
    pub invisible_glyph: u16,
}
const _: () = assert!(size_of::<ShapeOptionsC>() == 80);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_shape_options_default(out: *mut ShapeOptionsC) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe {
        *out = ShapeOptionsC {
            cluster_level: CLUSTER_MONOTONE_GRAPHEMES,
            ignorables: IGNORABLES_HIDE,
            before: core::ptr::null(),
            after: core::ptr::null(),
            beginning_of_text: false,
            has_point_size: false,
            point_size: 0.0,
            features: core::ptr::null(),
            features_len: 0,
            script: core::ptr::null(),
            language: core::ptr::null(),
            report_unsafe_to_concat: false,
            report_tatweel_positions: false,
            suppress_dotted_circle: false,
            has_invisible_glyph: false,
            invisible_glyph: 0,
        }
    };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape_with_options(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    vertical: bool,
    opts: *const ShapeOptionsC,
    out: *mut *mut Run,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };

    let mut built = ShapeOptions::default();
    let feats;
    if let Some(o) = unsafe { borrow(opts) } {
        feats = unsafe { apply_options(o, &mut built) };
        built.features = &feats;
    }

    let Some(r) = font.shape_with_options(text, &location, vertical, &built) else {
        return Status::Absent;
    };
    unsafe { deliver(out, Run::of(&r)) }
}

pub(crate) unsafe fn apply_options<'a>(
    o: &ShapeOptionsC,
    built: &mut ShapeOptions<'a>,
) -> Vec<(&'a str, u32)> {
    {
        built.cluster_level = match o.cluster_level {
            CLUSTER_MONOTONE_CHARACTERS => ClusterLevel::MonotoneCharacters,
            CLUSTER_CHARACTERS => ClusterLevel::Characters,
            CLUSTER_GRAPHEMES => ClusterLevel::Graphemes,
            _ => ClusterLevel::MonotoneGraphemes,
        };
        built.before = unsafe { str_of(o.before) }.unwrap_or("");
        built.after = unsafe { str_of(o.after) }.unwrap_or("");
        built.ignorables = match o.ignorables {
            IGNORABLES_REMOVE => Ignorables::Remove,
            IGNORABLES_PRESERVE => Ignorables::Preserve,
            _ => Ignorables::Hide,
        };
        built.beginning_of_text = o.beginning_of_text;
        built.point_size = o.has_point_size.then_some(o.point_size);
        built.script = unsafe { str_of(o.script) };
        built.language = unsafe { str_of(o.language) };
        built.report_unsafe_to_concat = o.report_unsafe_to_concat;
        built.report_tatweel_positions = o.report_tatweel_positions;
        built.suppress_dotted_circle = o.suppress_dotted_circle;
        built.invisible_glyph = o.has_invisible_glyph.then_some(o.invisible_glyph);
    }
    unsafe { features_of(o.features, o.features_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_measure_width(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    font_size: f64,
    out: *mut f64,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let location = unsafe { axes_of(axes, axes_len) };
    unsafe { *out = font.measure_width(text, &location, font_size) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_justification_extenders(
    font: *const Font,
    script_tag: *const c_char,
    out: *mut *mut crate::ffi::list::U16List,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(tag) = (unsafe { str_of(script_tag) }) else { return Status::Null };
    unsafe { deliver(out, crate::ffi::list::U16List(font.justification_extenders(tag))) }
}

fn cluster_of(code: i32) -> ClusterLevel {
    match code {
        CLUSTER_MONOTONE_CHARACTERS => ClusterLevel::MonotoneCharacters,
        CLUSTER_CHARACTERS => ClusterLevel::Characters,
        CLUSTER_GRAPHEMES => ClusterLevel::Graphemes,
        _ => ClusterLevel::MonotoneGraphemes,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_cluster_level_is_graphemes(level: i32, out: *mut i32) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = i32::from(cluster_of(level).is_graphemes()) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_cluster_level_is_monotone(level: i32, out: *mut i32) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = i32::from(cluster_of(level).is_monotone()) };
    Status::Ok
}
