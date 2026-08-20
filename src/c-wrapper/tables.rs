// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Where a call takes a raw buffer, its length is the caller's promise
// from `daegun.h` and is not checkable here.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

use crate::{Font, MathKernCorner};

use crate::ffi::handle::{Status, borrow, deliver, release, OwnedStr, Str};
use crate::ffi::list::{Axis, Blob, F64List, StrList, Text, U16List, axes_of};

unsafe fn tag_of<'a>(tag: *const c_char) -> Option<&'a str> {
    if tag.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(tag) }.to_str().ok()
}

pub struct SubsetHandle(crate::SubsetResult);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subset_ttf(
    subset: *const SubsetHandle,
    out_len: *mut usize,
) -> *const u8 {
    let Some(s) = (unsafe { borrow(subset) }) else { return core::ptr::null() };
    if out_len.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_len = s.0.ttf.len() };
    s.0.ttf.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subset_gid_map(
    subset: *const SubsetHandle,
    out_len: *mut usize,
) -> *const u16 {
    let Some(s) = (unsafe { borrow(subset) }) else { return core::ptr::null() };
    if out_len.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_len = s.0.gid_map.len() };
    s.0.gid_map.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subset_free(subset: *mut SubsetHandle) {
    unsafe { release(subset) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_subset(
    font: *const Font,
    gids: *const u16,
    gids_len: usize,
    axes: *const Axis,
    axes_len: usize,
    out: *mut *mut SubsetHandle,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if gids.is_null() && gids_len != 0 {
        return Status::Null;
    }
    let ids = if gids_len == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(gids, gids_len) } };
    let location = unsafe { axes_of(axes, axes_len) };
    match font.subset(ids, &location) {
        Ok(r) => unsafe { deliver(out, SubsetHandle(r)) },
        Err(e) => {
            crate::ffi::set_error(&alloc::format!("{e}"));
            Status::Parse
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_subset_text(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    out: *mut *mut SubsetHandle,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { tag_of(text) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    match font.subset_text(text, &location) {
        Ok(r) => unsafe { deliver(out, SubsetHandle(r)) },
        Err(e) => {
            crate::ffi::set_error(&alloc::format!("{e}"));
            Status::Parse
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_glyph_closure(
    font: *const Font,
    gids: *const u16,
    gids_len: usize,
    axes: *const Axis,
    axes_len: usize,
    out: *mut *mut U16List,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if gids.is_null() && gids_len != 0 {
        return Status::Null;
    }
    let ids = if gids_len == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(gids, gids_len) } };
    let location = unsafe { axes_of(axes, axes_len) };
    match font.glyph_closure(ids, &location) {
        Ok(v) => unsafe { deliver(out, U16List(v)) },
        Err(e) => {
            crate::ffi::set_error(&alloc::format!("{e}"));
            Status::Parse
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_math_constant(
    font: *const Font,
    which: i32,
    out: *mut f64,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(m) = font.math_constants() else { return Status::Absent };
    let value = match which {
        0 => m.script_percent_scale_down,
        1 => m.script_script_percent_scale_down,
        2 => m.delimited_sub_formula_min_height,
        3 => m.display_operator_min_height,
        4 => m.math_leading,
        5 => m.axis_height,
        6 => m.accent_base_height,
        7 => m.flattened_accent_base_height,
        8 => m.subscript_shift_down,
        9 => m.subscript_top_max,
        10 => m.subscript_baseline_drop_min,
        11 => m.superscript_shift_up,
        12 => m.superscript_shift_up_cramped,
        13 => m.superscript_bottom_min,
        14 => m.superscript_baseline_drop_max,
        15 => m.sub_superscript_gap_min,
        16 => m.superscript_bottom_max_with_subscript,
        17 => m.space_after_script,
        18 => m.upper_limit_gap_min,
        19 => m.upper_limit_baseline_rise_min,
        20 => m.lower_limit_gap_min,
        21 => m.lower_limit_baseline_drop_min,
        22 => m.stack_top_shift_up,
        23 => m.stack_top_display_style_shift_up,
        24 => m.stack_bottom_shift_down,
        25 => m.stack_bottom_display_style_shift_down,
        26 => m.stack_gap_min,
        27 => m.stack_display_style_gap_min,
        28 => m.stretch_stack_top_shift_up,
        29 => m.stretch_stack_bottom_shift_down,
        30 => m.stretch_stack_gap_above_min,
        31 => m.stretch_stack_gap_below_min,
        32 => m.fraction_numerator_shift_up,
        33 => m.fraction_numerator_display_style_shift_up,
        34 => m.fraction_denominator_shift_down,
        35 => m.fraction_denominator_display_style_shift_down,
        36 => m.fraction_numerator_gap_min,
        37 => m.fraction_num_display_style_gap_min,
        38 => m.fraction_rule_thickness,
        39 => m.fraction_denominator_gap_min,
        40 => m.fraction_denom_display_style_gap_min,
        41 => m.skewed_fraction_horizontal_gap,
        42 => m.skewed_fraction_vertical_gap,
        43 => m.overbar_vertical_gap,
        44 => m.overbar_rule_thickness,
        45 => m.overbar_extra_ascender,
        46 => m.underbar_vertical_gap,
        47 => m.underbar_rule_thickness,
        48 => m.underbar_extra_descender,
        49 => m.radical_vertical_gap,
        50 => m.radical_display_style_vertical_gap,
        51 => m.radical_rule_thickness,
        52 => m.radical_extra_ascender,
        53 => m.radical_kern_before_degree,
        54 => m.radical_kern_after_degree,
        55 => m.radical_degree_bottom_raise_percent,
        _ => return Status::Range,
    };
    unsafe { *out = value };
    Status::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn daegun_math_constant_count() -> i32 {
    56
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_math_italics_correction(
    font: *const Font,
    gid: u16,
    out: *mut f64,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(v) = font.math_italics_correction(gid) else { return Status::Absent };
    unsafe { *out = v };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_math_top_accent_attachment(
    font: *const Font,
    gid: u16,
    out: *mut f64,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = font.math_top_accent_attachment(gid) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_math_is_extended_shape(
    font: *const Font,
    gid: u16,
    out: *mut bool,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = font.math_is_extended_shape(gid) };
    Status::Ok
}

pub const MATH_KERN_TOP_RIGHT: i32 = 0;
pub const MATH_KERN_TOP_LEFT: i32 = 1;
pub const MATH_KERN_BOTTOM_RIGHT: i32 = 2;
pub const MATH_KERN_BOTTOM_LEFT: i32 = 3;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_math_kern(
    font: *const Font,
    gid: u16,
    corner: i32,
    height: f64,
    out: *mut f64,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let corner = match corner {
        MATH_KERN_TOP_RIGHT => MathKernCorner::TopRight,
        MATH_KERN_TOP_LEFT => MathKernCorner::TopLeft,
        MATH_KERN_BOTTOM_RIGHT => MathKernCorner::BottomRight,
        MATH_KERN_BOTTOM_LEFT => MathKernCorner::BottomLeft,
        _ => return Status::Range,
    };
    unsafe { *out = font.math_kern(gid, corner, height) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_math_min_connector_overlap(
    font: *const Font,
    out: *mut f64,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(v) = font.math_min_connector_overlap() else { return Status::Absent };
    unsafe { *out = v };
    Status::Ok
}

pub struct MathConstruction {
    variant_gids: Vec<u16>,
    variant_advances: Vec<f64>,
    has_assembly: bool,
    italics_correction: f64,
    part_gids: Vec<u16>,
    part_values: Vec<f64>,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_math_glyph_variants(
    font: *const Font,
    gid: u16,
    vertical: bool,
    out: *mut *mut MathConstruction,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(c) = font.math_glyph_variants(gid, vertical) else { return Status::Absent };
    let asm = c.assembly;
    let built = MathConstruction {
        variant_gids: c.variants.iter().map(|v| v.glyph_id).collect(),
        variant_advances: c.variants.iter().map(|v| v.advance).collect(),
        has_assembly: asm.is_some(),
        italics_correction: asm.as_ref().map_or(0.0, |a| a.italics_correction),
        part_gids: asm.as_ref().map_or_else(Vec::new, |a| a.parts.iter().map(|p| p.glyph_id).collect()),
        part_values: asm.as_ref().map_or_else(Vec::new, |a| {
            a.parts
                .iter()
                .flat_map(|p| {
                    [
                        p.start_connector_length,
                        p.end_connector_length,
                        p.full_advance,
                        f64::from(u8::from(p.is_extender)),
                    ]
                })
                .collect()
        }),
    };
    unsafe { deliver(out, built) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_math_construction_variants(
    c: *const MathConstruction,
    out_count: *mut usize,
    out_gids: *mut *const u16,
    out_advances: *mut *const f64,
) -> Status {
    let Some(c) = (unsafe { borrow(c) }) else { return Status::Null };
    if out_count.is_null() {
        return Status::Null;
    }
    unsafe {
        *out_count = c.variant_gids.len();
        if !out_gids.is_null() {
            *out_gids = c.variant_gids.as_ptr();
        }
        if !out_advances.is_null() {
            *out_advances = c.variant_advances.as_ptr();
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_math_construction_assembly(
    c: *const MathConstruction,
    out_italics_correction: *mut f64,
    out_part_count: *mut usize,
    out_part_gids: *mut *const u16,
    out_part_values: *mut *const f64,
) -> Status {
    let Some(c) = (unsafe { borrow(c) }) else { return Status::Null };
    if !c.has_assembly {
        return Status::Absent;
    }
    unsafe {
        if !out_italics_correction.is_null() {
            *out_italics_correction = c.italics_correction;
        }
        if !out_part_count.is_null() {
            *out_part_count = c.part_gids.len();
        }
        if !out_part_gids.is_null() {
            *out_part_gids = c.part_gids.as_ptr();
        }
        if !out_part_values.is_null() {
            *out_part_values = c.part_values.as_ptr();
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_math_construction_free(c: *mut MathConstruction) {
    unsafe { release(c) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_base_is_glyph_free(
    font: *const Font,
    out: *mut bool,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = font.base_is_glyph_free() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_base_info(
    font: *const Font,
    script_tag: *const c_char,
    vertical: bool,
    out_default_baseline: *mut *mut Text,
    out_baseline_tags: *mut *mut StrList,
    out_baseline_coords: *mut *mut F64List,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(tag) = (unsafe { tag_of(script_tag) }) else { return Status::Null };
    let Some(info) = font.base_info(tag, vertical) else { return Status::Absent };

    if !out_default_baseline.is_null() {
        match &info.default_baseline_tag {
            Some(t) => {
                let st = unsafe { deliver(out_default_baseline, Text::new(t)) };
                if st != Status::Ok {
                    return st;
                }
            }
            None => unsafe { *out_default_baseline = core::ptr::null_mut() },
        }
    }
    if !out_baseline_tags.is_null() {
        let tags: Vec<String> = info.baseline_coords.keys().cloned().collect();
        let st = unsafe { deliver(out_baseline_tags, StrList::new(tags)) };
        if st != Status::Ok {
            return st;
        }
    }
    if !out_baseline_coords.is_null() {
        let coords: Vec<f64> = info.baseline_coords.values().map(|v| f64::from(*v)).collect();
        return unsafe { deliver(out_baseline_coords, F64List(coords)) };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_script_tags(
    font: *const Font,
    out: *mut *mut StrList,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    unsafe { deliver(out, StrList::new(font.script_tags())) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_language_tags(
    font: *const Font,
    script: *const c_char,
    out: *mut *mut StrList,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(script) = (unsafe { tag_of(script) }) else { return Status::Null };
    unsafe { deliver(out, StrList::new(font.language_tags(script))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_feature_tags(
    font: *const Font,
    script: *const c_char,
    language: *const c_char,
    out: *mut *mut StrList,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let script = unsafe { tag_of(script) };
    let language = unsafe { tag_of(language) };
    unsafe { deliver(out, StrList::new(font.feature_tags(script, language))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_justification_glyphs(
    font: *const Font,
    script_tag: *const c_char,
    out: *mut *mut U16List,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(tag) = (unsafe { tag_of(script_tag) }) else { return Status::Null };
    let Some(v) = font.justification_glyphs(tag) else { return Status::Absent };
    unsafe { deliver(out, U16List(v)) }
}

pub struct StatHandle {
    axis_tags: Vec<String>,
    axis_orderings: Vec<u16>,
    values: Vec<StatValueC>,
    names: Vec<Option<OwnedStr>>,
    combos: Vec<AxisValueC>,
    elided_fallback: Option<String>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StatValueC {
    pub kind: i32,
    pub axis_index: u16,
    pub elidable: u8,
    pub has_name: u8,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub linked_value: f64,
    pub combo_start: u32,
    pub combo_count: u32,
}

const _: () = assert!(size_of::<StatValueC>() == 48);
const _: () = assert!(align_of::<StatValueC>() == 8);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AxisValueC {
    pub axis_index: u16,
    pub value: f64,
}

const _: () = assert!(size_of::<AxisValueC>() == 16);

pub const STAT_SINGLE: i32 = 0;
pub const STAT_RANGE: i32 = 1;
pub const STAT_LINKED: i32 = 2;
pub const STAT_COMBO: i32 = 3;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_stat_info(
    font: *const Font,
    out: *mut *mut StatHandle,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(info) = font.stat_info() else { return Status::Absent };
    let mut values = Vec::with_capacity(info.values.len());
    let mut names = Vec::with_capacity(info.values.len());
    let mut combos = Vec::new();
    for v in &info.values {
        let (flat, name) = flatten_stat_value(v, &mut combos);
        values.push(flat);
        names.push(name.map(OwnedStr::new));
    }
    let built = StatHandle {
        axis_tags: info.axes.iter().map(|a| a.tag.clone()).collect(),
        axis_orderings: info.axes.iter().map(|a| a.ordering).collect(),
        values,
        names,
        combos,
        elided_fallback: info.elided_fallback_name,
    };
    unsafe { deliver(out, built) }
}

fn flatten_stat_value<'a>(
    v: &'a crate::StatAxisValue,
    combos: &mut Vec<AxisValueC>,
) -> (StatValueC, Option<&'a str>) {
    use crate::StatAxisValue as V;
    let blank = StatValueC {
        kind: STAT_SINGLE,
        axis_index: 0,
        elidable: 0,
        has_name: 0,
        value: 0.0,
        min: 0.0,
        max: 0.0,
        linked_value: 0.0,
        combo_start: 0,
        combo_count: 0,
    };
    match v {
        V::Single { axis_index, name, value, elidable } => (
            StatValueC {
                kind: STAT_SINGLE,
                axis_index: *axis_index,
                elidable: u8::from(*elidable),
                has_name: u8::from(name.is_some()),
                value: *value,
                ..blank
            },
            name.as_deref(),
        ),
        V::Range { axis_index, name, nominal, min, max, elidable } => (
            StatValueC {
                kind: STAT_RANGE,
                axis_index: *axis_index,
                elidable: u8::from(*elidable),
                has_name: u8::from(name.is_some()),
                value: *nominal,
                min: *min,
                max: *max,
                ..blank
            },
            name.as_deref(),
        ),
        V::Linked { axis_index, name, value, linked_value, elidable } => (
            StatValueC {
                kind: STAT_LINKED,
                axis_index: *axis_index,
                elidable: u8::from(*elidable),
                has_name: u8::from(name.is_some()),
                value: *value,
                linked_value: *linked_value,
                ..blank
            },
            name.as_deref(),
        ),
        V::Combo { name, values, elidable } => {
            let start = combos.len();
            combos.extend(
                values.iter().map(|(axis_index, value)| AxisValueC {
                    axis_index: *axis_index,
                    value: *value,
                }),
            );
            (
                StatValueC {
                    kind: STAT_COMBO,
                    elidable: u8::from(*elidable),
                    has_name: u8::from(name.is_some()),
                    combo_start: u32::try_from(start).unwrap_or(u32::MAX),
                    combo_count: u32::try_from(values.len()).unwrap_or(0),
                    ..blank
                },
                name.as_deref(),
            )
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_stat_axes(
    stat: *const StatHandle,
    out_count: *mut usize,
    out_tags: *mut *mut StrList,
    out_orderings: *mut *const u16,
) -> Status {
    let Some(stat) = (unsafe { borrow(stat) }) else { return Status::Null };
    unsafe {
        if !out_count.is_null() {
            *out_count = stat.axis_tags.len();
        }
        if !out_orderings.is_null() {
            *out_orderings = stat.axis_orderings.as_ptr();
        }
    }
    if !out_tags.is_null() {
        return unsafe { deliver(out_tags, StrList::new(stat.axis_tags.clone())) };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_stat_value_count(
    stat: *const StatHandle,
    out: *mut usize,
) -> Status {
    let Some(stat) = (unsafe { borrow(stat) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = stat.values.len() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_stat_elided_fallback_name(
    stat: *const StatHandle,
    out: *mut *mut Text,
) -> Status {
    let Some(stat) = (unsafe { borrow(stat) }) else { return Status::Null };
    let Some(name) = &stat.elided_fallback else { return Status::Absent };
    unsafe { deliver(out, Text::new(name)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_stat_free(stat: *mut StatHandle) {
    unsafe { release(stat) }
}

const _: Option<core::marker::PhantomData<Blob>> = None;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_stat_value_at(
    stat: *const StatHandle,
    index: usize,
    out: *mut StatValueC,
) -> Status {
    let Some(stat) = (unsafe { borrow(stat) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(v) = stat.values.get(index) else { return Status::Range };
    unsafe { *out = *v };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_stat_value_name(
    stat: *const StatHandle,
    index: usize,
    out: *mut Str,
) -> Status {
    let Some(stat) = (unsafe { borrow(stat) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(slot) = stat.names.get(index) else { return Status::Range };
    let Some(name) = slot.as_ref() else { return Status::Absent };
    unsafe { *out = name.as_str() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_stat_combo_values(
    stat: *const StatHandle,
    out_count: *mut usize,
) -> *const AxisValueC {
    let Some(stat) = (unsafe { borrow(stat) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = stat.combos.len() };
    stat.combos.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_subset_new_gid(
    subset: *const SubsetHandle,
    old_gid: u16,
    out: *mut u16,
) -> Status {
    let Some(s) = (unsafe { borrow(subset) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(new) = s.0.new_gid(old_gid) else { return Status::Absent };
    unsafe { *out = new };
    Status::Ok
}
