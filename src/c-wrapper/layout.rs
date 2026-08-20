// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Anything the caller must uphold beyond that is noted at its site.

use alloc::vec::Vec;
use core::ffi::c_char;

use crate::{Font, JustifyOptions, LayoutOptions};

use crate::ffi::handle::{Status, borrow, deliver, release};
use crate::ffi::list::{Axis, U16List, U32List, axes_of};
use crate::ffi::shape::{Run, str_of};

#[repr(transparent)]
pub struct JstfMods(crate::JstfModLists);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_justification_priorities(
    font: *const Font,
    script_tag: *const c_char,
    lang_sys_tag: *const c_char,
    out: *mut *mut JstfPriorities,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(tag) = (unsafe { str_of(script_tag) }) else { return Status::Null };
    let lang = unsafe { str_of(lang_sys_tag) };
    let Some(levels) = font.justification_priorities(tag, lang) else { return Status::Absent };
    unsafe { deliver(out, JstfPriorities(levels)) }
}

pub struct JstfPriorities(Vec<crate::JstfModLists>);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_jstf_priorities_count(
    p: *const JstfPriorities,
    out: *mut usize,
) -> Status {
    let Some(p) = (unsafe { borrow(p) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = p.0.len() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_jstf_priorities_at(
    p: *const JstfPriorities,
    index: usize,
    out: *mut *const JstfMods,
) -> Status {
    let Some(p) = (unsafe { borrow(p) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    let Some(level) = p.0.get(index) else { return Status::Range };
    // `JstfMods` is `repr(transparent)` over `JstfModLists`, so a reference to one is a reference
    // to the other.
    unsafe { *out = (level as *const crate::JstfModLists).cast::<JstfMods>() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_jstf_priorities_free(p: *mut JstfPriorities) {
    unsafe { release(p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape_justified(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    vertical: bool,
    mods: *const JstfMods,
    shrink: bool,
    out: *mut *mut Run,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let Some(mods) = (unsafe { borrow(mods) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let Some(r) = font.shape_justified(text, &location, vertical, &mods.0, shrink) else {
        return Status::Absent;
    };
    unsafe { deliver(out, Run::of(&r)) }
}

pub struct Justified {
    run: Run,
    has_level: bool,
    level: usize,
    shrink: bool,
    width: f64,
    best_effort: bool,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_justify(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    vertical: bool,
    script_tag: *const c_char,
    lang_sys_tag: *const c_char,
    target_width: f64,
    tolerance: f64,
    out: *mut *mut Justified,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let Some(script) = (unsafe { str_of(script_tag) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let opts = JustifyOptions {
        script_tag: script,
        lang_sys_tag: unsafe { str_of(lang_sys_tag) },
        target_width,
        tolerance,
    };
    let Some(j) = font.justify(text, &location, vertical, &opts) else { return Status::Absent };
    unsafe {
        deliver(
            out,
            Justified {
                run: Run::of(&j.run),
                has_level: j.level.is_some(),
                level: j.level.unwrap_or(0),
                shrink: j.shrink,
                width: j.width,
                best_effort: j.best_effort,
            },
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_justified_run(j: *const Justified) -> *const Run {
    match unsafe { borrow(j) } {
        Some(j) => &raw const j.run,
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_justified_info(
    j: *const Justified,
    out_has_level: *mut bool,
    out_level: *mut usize,
    out_shrink: *mut bool,
    out_width: *mut f64,
    out_best_effort: *mut bool,
) -> Status {
    let Some(j) = (unsafe { borrow(j) }) else { return Status::Null };
    unsafe {
        if !out_has_level.is_null() {
            *out_has_level = j.has_level;
        }
        if !out_level.is_null() {
            *out_level = j.level;
        }
        if !out_shrink.is_null() {
            *out_shrink = j.shrink;
        }
        if !out_width.is_null() {
            *out_width = j.width;
        }
        if !out_best_effort.is_null() {
            *out_best_effort = j.best_effort;
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_justified_free(j: *mut Justified) {
    unsafe { release(j) }
}

pub struct BidiRuns(Vec<BidiRunEntry>);

struct BidiRunEntry {
    run: Run,
    level: u8,
    chars: Vec<usize>,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape_bidi(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    base: i32,
    out: *mut *mut BidiRuns,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let base = match base {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    let Some(runs) = font.shape_bidi(text, &location, base) else { return Status::Absent };
    let built = runs
        .iter()
        .map(|r| BidiRunEntry { run: Run::of(&r.run), level: r.level, chars: r.chars.clone() })
        .collect();
    unsafe { deliver(out, BidiRuns(built)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_shape_bidi_with(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    base: i32,
    opts: *const crate::ffi::shape::ShapeOptionsC,
    out: *mut *mut BidiRuns,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let base = match base {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    let feats;
    let mut built = crate::ShapeOptions::default();
    if let Some(o) = unsafe { borrow(opts) } {
        feats = unsafe { crate::ffi::shape::apply_options(o, &mut built) };
        built.features = &feats;
    }
    let Some(runs) = font.shape_bidi_with(text, &location, base, &built) else {
        return Status::Absent;
    };
    let built_runs = runs
        .iter()
        .map(|r| BidiRunEntry { run: Run::of(&r.run), level: r.level, chars: r.chars.clone() })
        .collect();
    unsafe { deliver(out, BidiRuns(built_runs)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bidi_runs_count(
    runs: *const BidiRuns,
    out: *mut usize,
) -> Status {
    let Some(r) = (unsafe { borrow(runs) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = r.0.len() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bidi_runs_at(
    runs: *const BidiRuns,
    index: usize,
    out_run: *mut *const Run,
    out_level: *mut u8,
    out_chars: *mut *const usize,
    out_chars_count: *mut usize,
) -> Status {
    let Some(r) = (unsafe { borrow(runs) }) else { return Status::Null };
    let Some(entry) = r.0.get(index) else { return Status::Range };
    unsafe {
        if !out_run.is_null() {
            *out_run = &raw const entry.run;
        }
        if !out_level.is_null() {
            *out_level = entry.level;
        }
        if !out_chars.is_null() {
            *out_chars = entry.chars.as_ptr();
        }
        if !out_chars_count.is_null() {
            *out_chars_count = entry.chars.len();
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bidi_runs_free(runs: *mut BidiRuns) {
    unsafe { release(runs) }
}

pub const ALIGN_START: i32 = 0;
pub const ALIGN_END: i32 = 1;
pub const ALIGN_CENTER: i32 = 2;
pub const ALIGN_JUSTIFY: i32 = 3;

pub const WRITING_HORIZONTAL: i32 = 0;
pub const WRITING_VERTICAL_RL: i32 = 1;
pub const WRITING_VERTICAL_LR: i32 = 2;

pub const ORIENTATION_MIXED: i32 = 0;
pub const ORIENTATION_UPRIGHT: i32 = 1;
pub const ORIENTATION_SIDEWAYS: i32 = 2;

pub const BREAK_GREEDY: i32 = 0;
pub const BREAK_OPTIMAL: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LayoutOptionsC {
    pub max_inline_size: f64,
    pub align: i32,
    pub writing_mode: i32,
    pub text_orientation: i32,
    pub base_direction: i32,
    pub language: *const c_char,
    pub has_line_height: bool,
    pub line_height: f64,
    pub strategy: i32,
    pub has_max_lines: bool,
    pub max_lines: usize,
}
const _: () = assert!(size_of::<LayoutOptionsC>() == 64);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_layout_options_default(out: *mut LayoutOptionsC) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    let d = LayoutOptions::default();
    unsafe {
        *out = LayoutOptionsC {
            max_inline_size: d.max_inline_size,
            align: ALIGN_START,
            writing_mode: WRITING_HORIZONTAL,
            text_orientation: ORIENTATION_MIXED,
            base_direction: -1,
            language: core::ptr::null(),
            has_line_height: false,
            line_height: 0.0,
            strategy: BREAK_GREEDY,
            has_max_lines: false,
            max_lines: 0,
        }
    };
    Status::Ok
}

pub struct Layout {
    lines: Vec<LayoutLineEntry>,
    inline_size: f64,
    block_size: f64,
    has_truncated: bool,
    truncated: usize,
}

struct LayoutLineEntry {
    runs: Vec<PositionedRunEntry>,
    char_start: usize,
    char_end: usize,
    baseline: f64,
    inline_size: f64,
    ascent: f64,
    descent: f64,
    hard_break: bool,
}

struct PositionedRunEntry {
    run: Run,
    offset_x: f64,
    offset_y: f64,
    level: u8,
    char_start: usize,
    char_end: usize,
    upright: bool,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_layout(
    font: *const Font,
    text: *const c_char,
    axes: *const Axis,
    axes_len: usize,
    opts: *const LayoutOptionsC,
    out: *mut *mut Layout,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };

    let mut built = LayoutOptions::default();
    if let Some(o) = unsafe { borrow(opts) } {
        built.max_inline_size = o.max_inline_size;
        built.align = match o.align {
            ALIGN_END => crate::Align::End,
            ALIGN_CENTER => crate::Align::Center,
            ALIGN_JUSTIFY => crate::Align::Justify,
            _ => crate::Align::Start,
        };
        built.writing_mode = match o.writing_mode {
            WRITING_VERTICAL_RL => crate::WritingMode::VerticalRl,
            WRITING_VERTICAL_LR => crate::WritingMode::VerticalLr,
            _ => crate::WritingMode::Horizontal,
        };
        built.text_orientation = match o.text_orientation {
            ORIENTATION_UPRIGHT => crate::TextOrientation::Upright,
            ORIENTATION_SIDEWAYS => crate::TextOrientation::Sideways,
            _ => crate::TextOrientation::Mixed,
        };
        built.base_direction = match o.base_direction {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        built.language = unsafe { str_of(o.language) };
        built.line_height = o.has_line_height.then_some(o.line_height);
        built.strategy = match o.strategy {
            BREAK_OPTIMAL => crate::BreakStrategy::Optimal,
            _ => crate::BreakStrategy::Greedy,
        };
        built.max_lines = o.has_max_lines.then_some(o.max_lines);
    }

    let Some(l) = font.layout(text, &location, &built) else { return Status::Absent };
    let lines = l
        .lines
        .iter()
        .map(|line| LayoutLineEntry {
            runs: line
                .runs
                .iter()
                .map(|r| PositionedRunEntry {
                    run: Run::of(&r.run),
                    offset_x: r.offset.0,
                    offset_y: r.offset.1,
                    level: r.level,
                    char_start: r.chars.0,
                    char_end: r.chars.1,
                    upright: r.upright,
                })
                .collect(),
            char_start: line.chars.0,
            char_end: line.chars.1,
            baseline: line.baseline,
            inline_size: line.inline_size,
            ascent: line.ascent,
            descent: line.descent,
            hard_break: line.hard_break,
        })
        .collect();
    unsafe {
        deliver(
            out,
            Layout {
                lines,
                inline_size: l.inline_size,
                block_size: l.block_size,
                has_truncated: l.truncated.is_some(),
                truncated: l.truncated.unwrap_or(0),
            },
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_layout_info(
    layout: *const Layout,
    out_line_count: *mut usize,
    out_inline_size: *mut f64,
    out_block_size: *mut f64,
    out_has_truncated: *mut bool,
    out_truncated: *mut usize,
) -> Status {
    let Some(l) = (unsafe { borrow(layout) }) else { return Status::Null };
    unsafe {
        if !out_line_count.is_null() {
            *out_line_count = l.lines.len();
        }
        if !out_inline_size.is_null() {
            *out_inline_size = l.inline_size;
        }
        if !out_block_size.is_null() {
            *out_block_size = l.block_size;
        }
        if !out_has_truncated.is_null() {
            *out_has_truncated = l.has_truncated;
        }
        if !out_truncated.is_null() {
            *out_truncated = l.truncated;
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_layout_line(
    layout: *const Layout,
    index: usize,
    out_run_count: *mut usize,
    out_char_start: *mut usize,
    out_char_end: *mut usize,
    out_baseline: *mut f64,
    out_inline_size: *mut f64,
    out_ascent: *mut f64,
    out_descent: *mut f64,
    out_hard_break: *mut bool,
) -> Status {
    let Some(l) = (unsafe { borrow(layout) }) else { return Status::Null };
    let Some(line) = l.lines.get(index) else { return Status::Range };
    unsafe {
        if !out_run_count.is_null() {
            *out_run_count = line.runs.len();
        }
        if !out_char_start.is_null() {
            *out_char_start = line.char_start;
        }
        if !out_char_end.is_null() {
            *out_char_end = line.char_end;
        }
        if !out_baseline.is_null() {
            *out_baseline = line.baseline;
        }
        if !out_inline_size.is_null() {
            *out_inline_size = line.inline_size;
        }
        if !out_ascent.is_null() {
            *out_ascent = line.ascent;
        }
        if !out_descent.is_null() {
            *out_descent = line.descent;
        }
        if !out_hard_break.is_null() {
            *out_hard_break = line.hard_break;
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_layout_run(
    layout: *const Layout,
    line: usize,
    index: usize,
    out_run: *mut *const Run,
    out_offset_x: *mut f64,
    out_offset_y: *mut f64,
    out_level: *mut u8,
    out_char_start: *mut usize,
    out_char_end: *mut usize,
    out_upright: *mut bool,
) -> Status {
    let Some(l) = (unsafe { borrow(layout) }) else { return Status::Null };
    let Some(line) = l.lines.get(line) else { return Status::Range };
    let Some(r) = line.runs.get(index) else { return Status::Range };
    unsafe {
        if !out_run.is_null() {
            *out_run = &raw const r.run;
        }
        if !out_offset_x.is_null() {
            *out_offset_x = r.offset_x;
        }
        if !out_offset_y.is_null() {
            *out_offset_y = r.offset_y;
        }
        if !out_level.is_null() {
            *out_level = r.level;
        }
        if !out_char_start.is_null() {
            *out_char_start = r.char_start;
        }
        if !out_char_end.is_null() {
            *out_char_end = r.char_end;
        }
        if !out_upright.is_null() {
            *out_upright = r.upright;
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_layout_free(layout: *mut Layout) {
    unsafe { release(layout) }
}

#[unsafe(no_mangle)]
pub extern "C" fn daegun_writing_mode_is_vertical(mode: i32) -> bool {
    match mode {
        WRITING_VERTICAL_RL => crate::WritingMode::VerticalRl.is_vertical(),
        WRITING_VERTICAL_LR => crate::WritingMode::VerticalLr.is_vertical(),
        _ => crate::WritingMode::Horizontal.is_vertical(),
    }
}

pub struct BidiParagraph {
    para: crate::BidiParagraph,
    text: alloc::string::String,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_text_bidi_paragraph(
    text: *const c_char,
    base: i32,
    out: *mut *mut BidiParagraph,
) -> Status {
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let base = match base {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    let para = crate::resolve_bidi(text, base);
    unsafe { deliver(out, BidiParagraph { para, text: alloc::string::String::from(text) }) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bidi_paragraph_base_level(
    p: *const BidiParagraph,
    out: *mut u8,
) -> Status {
    let Some(p) = (unsafe { borrow(p) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = p.para.base_level };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_bidi_paragraph_free(p: *mut BidiParagraph) {
    unsafe { release(p) }
}

pub struct VisualRuns(Vec<(Vec<usize>, u8)>);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_text_line_visual_runs(
    p: *const BidiParagraph,
    start: usize,
    end: usize,
    out: *mut *mut VisualRuns,
) -> Status {
    let Some(p) = (unsafe { borrow(p) }) else { return Status::Null };
    let runs = crate::line_visual_runs(&p.para, &p.text, start, end);
    let built = runs.into_iter().map(|r| (r.chars, r.level)).collect();
    unsafe { deliver(out, VisualRuns(built)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_visual_runs_count(
    runs: *const VisualRuns,
    out: *mut usize,
) -> Status {
    let Some(r) = (unsafe { borrow(runs) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = r.0.len() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_visual_runs_at(
    runs: *const VisualRuns,
    index: usize,
    out_level: *mut u8,
    out_chars: *mut *const usize,
    out_chars_count: *mut usize,
) -> Status {
    let Some(r) = (unsafe { borrow(runs) }) else { return Status::Null };
    let Some((chars, level)) = r.0.get(index) else { return Status::Range };
    unsafe {
        if !out_level.is_null() {
            *out_level = *level;
        }
        if !out_chars.is_null() {
            *out_chars = chars.as_ptr();
        }
        if !out_chars_count.is_null() {
            *out_chars_count = chars.len();
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_visual_runs_free(runs: *mut VisualRuns) {
    unsafe { release(runs) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_text_grapheme_boundaries(
    text: *const c_char,
    out: *mut *mut U32List,
) -> Status {
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let v: Vec<u32> = crate::grapheme_boundaries(text).iter().map(|n| *n as u32).collect();
    unsafe { deliver(out, U32List(v)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_text_word_boundaries(
    text: *const c_char,
    out: *mut *mut U32List,
) -> Status {
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let v: Vec<u32> = crate::word_boundaries(text).iter().map(|n| *n as u32).collect();
    unsafe { deliver(out, U32List(v)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_text_line_break_opportunities(
    text: *const c_char,
    out_at: *mut *mut U32List,
    out_mandatory: *mut *mut crate::ffi::list::Blob,
) -> Status {
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let breaks = crate::line_break_opportunities(text);
    if !out_at.is_null() {
        let at: Vec<u32> = breaks.iter().map(|b| b.at as u32).collect();
        let st = unsafe { deliver(out_at, U32List(at)) };
        if st != Status::Ok {
            return st;
        }
    }
    if !out_mandatory.is_null() {
        let m: Vec<u8> = breaks.iter().map(|b| u8::from(b.mandatory)).collect();
        return unsafe { deliver(out_mandatory, crate::ffi::list::Blob(m)) };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_text_script_runs(
    text: *const c_char,
    out: *mut *mut U32List,
) -> Status {
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let runs = crate::script_runs(text);
    let mut v = Vec::with_capacity(runs.len() * 3);
    for r in &runs {
        v.push(r.start as u32);
        v.push(r.end as u32);
        v.push(u32::from(r.script.0));
    }
    unsafe { deliver(out, U32List(v)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_text_resolve_bidi(
    text: *const c_char,
    base: i32,
    out_base_level: *mut u8,
    out_levels: *mut *mut crate::ffi::list::Blob,
    out_visual_order: *mut *mut U32List,
) -> Status {
    let Some(text) = (unsafe { str_of(text) }) else { return Status::Null };
    let base = match base {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    let para = crate::resolve_bidi(text, base);
    if !out_base_level.is_null() {
        unsafe { *out_base_level = para.base_level };
    }
    if !out_levels.is_null() {
        let st = unsafe { deliver(out_levels, crate::ffi::list::Blob(para.levels.clone())) };
        if st != Status::Ok {
            return st;
        }
    }
    if !out_visual_order.is_null() {
        let order: Vec<u32> = para.visual_order.iter().map(|n| *n as u32).collect();
        return unsafe { deliver(out_visual_order, U32List(order)) };
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_script_name(
    script: u16,
    out: *mut *mut crate::ffi::list::Text,
) -> Status {
    unsafe { deliver(out, crate::ffi::list::Text::new(crate::Script(script).name())) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_script_is_rtl(script: u16, out: *mut bool) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    let Some(rtl) = crate::Script(script).is_rtl() else { return Status::Absent };
    unsafe { *out = rtl };
    Status::Ok
}

const _: Option<core::marker::PhantomData<U16List>> = None;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_char_general_category(
    codepoint: u32,
    out: *mut i32,
) -> Status {
    let Some(c) = char::from_u32(codepoint) else { return Status::Range };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = crate::general_category(c) as i32 };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_char_is_upright(
    codepoint: u32,
    has_vertical_form: i32,
    out: *mut i32,
) -> Status {
    let Some(c) = char::from_u32(codepoint) else { return Status::Range };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = i32::from(crate::is_upright(c, has_vertical_form != 0)) };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_char_vertical_form(
    codepoint: u32,
    out: *mut u32,
) -> Status {
    let Some(c) = char::from_u32(codepoint) else { return Status::Range };
    let Some(v) = crate::vertical_form(c) else { return Status::Absent };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = v as u32 };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_script_opentype_tags(
    script: u16,
    out: *mut *mut crate::ffi::list::StrList,
) -> Status {
    let tags = crate::Script(script).opentype_tags();
    unsafe { deliver(out, crate::ffi::list::StrList::new(tags)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_script_is_context_dependent(script: u16, out: *mut i32) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = i32::from(crate::Script(script).is_context_dependent()) };
    Status::Ok
}
