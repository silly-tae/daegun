// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Anything the caller must uphold beyond that is noted at its site.

use alloc::vec::Vec;

use crate::Font;

use crate::ffi::handle::{Status, borrow, deliver, release};
use crate::ffi::list::{Axis, axes_of};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColrLayerC {
    pub gid: u16,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub is_foreground: bool,
}

const _: () = assert!(size_of::<ColrLayerC>() == 8);

pub struct ColrLayers(Vec<ColrLayerC>);

fn layers_of(v: Vec<crate::ColrLayer>) -> ColrLayers {
    ColrLayers(
        v.into_iter()
            .map(|(gid, r, g, b, a, fg)| ColrLayerC { gid, r, g, b, a, is_foreground: fg })
            .collect(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_colr_layers(
    font: *const Font,
    gid: u16,
    out: *mut *mut ColrLayers,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(v) = font.colr_layers(gid) else { return Status::Absent };
    unsafe { deliver(out, layers_of(v)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_colr_layers_for_palette(
    font: *const Font,
    gid: u16,
    palette_index: u16,
    out: *mut *mut ColrLayers,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(v) = font.colr_layers_for_palette(gid, palette_index) else { return Status::Absent };
    unsafe { deliver(out, layers_of(v)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_colr_layers_data(
    layers: *const ColrLayers,
    out_count: *mut usize,
) -> *const ColrLayerC {
    let Some(l) = (unsafe { borrow(layers) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = l.0.len() };
    l.0.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_colr_layers_free(layers: *mut ColrLayers) {
    unsafe { release(layers) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_palette_count(font: *const Font, out: *mut u16) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = font.palette_count() };
    Status::Ok
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PaletteInfoC {
    pub index: u16,
    pub light_safe: bool,
    pub dark_safe: bool,
    pub has_name_id: bool,
    pub name_id: u16,
}
const _: () = assert!(size_of::<PaletteInfoC>() == 8);

pub struct Palettes(Vec<PaletteInfoC>);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_palette_info(
    font: *const Font,
    out: *mut *mut Palettes,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let v = font
        .palette_info()
        .into_iter()
        .map(|p| PaletteInfoC {
            index: p.index,
            light_safe: p.light_safe,
            dark_safe: p.dark_safe,
            has_name_id: p.name_id.is_some(),
            name_id: p.name_id.unwrap_or(0),
        })
        .collect();
    unsafe { deliver(out, Palettes(v)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_palettes_data(
    p: *const Palettes,
    out_count: *mut usize,
) -> *const PaletteInfoC {
    let Some(p) = (unsafe { borrow(p) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = p.0.len() };
    p.0.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_palettes_free(p: *mut Palettes) {
    unsafe { release(p) }
}

pub struct GlyphBitmapHandle {
    png: Vec<u8>,
    ppem: u16,
    origin_x: i16,
    origin_y: i16,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_glyph_bitmap(
    font: *const Font,
    gid: u16,
    target_ppem: u16,
    out: *mut *mut GlyphBitmapHandle,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let Some(b) = font.glyph_bitmap(gid, target_ppem) else { return Status::Absent };
    unsafe {
        deliver(
            out,
            GlyphBitmapHandle {
                png: b.png,
                ppem: b.ppem,
                origin_x: b.origin_x,
                origin_y: b.origin_y,
            },
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_glyph_bitmap_png(
    b: *const GlyphBitmapHandle,
    out_len: *mut usize,
    out_ppem: *mut u16,
    out_origin_x: *mut i16,
    out_origin_y: *mut i16,
) -> *const u8 {
    let Some(b) = (unsafe { borrow(b) }) else { return core::ptr::null() };
    if out_len.is_null() {
        return core::ptr::null();
    }
    unsafe {
        *out_len = b.png.len();
        if !out_ppem.is_null() {
            *out_ppem = b.ppem;
        }
        if !out_origin_x.is_null() {
            *out_origin_x = b.origin_x;
        }
        if !out_origin_y.is_null() {
            *out_origin_y = b.origin_y;
        }
    }
    b.png.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_glyph_bitmap_free(b: *mut GlyphBitmapHandle) {
    unsafe { release(b) }
}

pub const PAINT_LAYERS: i32 = 0;
pub const PAINT_GLYPH: i32 = 1;
pub const PAINT_COLR_GLYPH: i32 = 2;
pub const PAINT_SOLID: i32 = 3;
pub const PAINT_LINEAR_GRADIENT: i32 = 4;
pub const PAINT_RADIAL_GRADIENT: i32 = 5;
pub const PAINT_SWEEP_GRADIENT: i32 = 6;
pub const PAINT_TRANSFORM: i32 = 7;
pub const PAINT_TRANSLATE: i32 = 8;
pub const PAINT_SCALE: i32 = 9;
pub const PAINT_SCALE_UNIFORM: i32 = 10;
pub const PAINT_ROTATE: i32 = 11;
pub const PAINT_SKEW: i32 = 12;
pub const PAINT_COMPOSITE: i32 = 13;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PaintNode {
    pub kind: i32,
    pub child_start: u32,
    pub child_count: u32,
    pub stops_start: u32,
    pub stops_count: u32,
    pub glyph_id: u16,
    pub is_foreground: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub alpha: u8,
    pub extend: u8,
    pub composite_mode: u8,
    pub has_center: u8,
    pub numbers: [f64; 8],
}
const _: () = assert!(size_of::<PaintNode>() == 96);

pub struct PaintHandle {
    nodes: Vec<PaintNode>,
    children: Vec<u32>,
    stop_offsets: Vec<f64>,
    stop_colors: Vec<u8>,
}

impl PaintHandle {
    fn flatten(&mut self, paint: &crate::Paint) -> u32 {
        use crate::Paint as P;
        let me = self.nodes.len() as u32;
        self.nodes.push(PaintNode {
            kind: -1,
            child_start: 0,
            child_count: 0,
            stops_start: 0,
            stops_count: 0,
            glyph_id: 0,
            is_foreground: 0,
            r: 0,
            g: 0,
            b: 0,
            alpha: 0,
            extend: 0,
            composite_mode: 0,
            has_center: 0,
            numbers: [0.0; 8],
        });

        let mut node = self.nodes[me as usize];
        let mut kids: Vec<u32> = Vec::new();

        match paint {
            P::Layers(list) => {
                node.kind = PAINT_LAYERS;
                for child in list {
                    kids.push(self.flatten(child));
                }
            }
            P::Glyph { child, glyph_id } => {
                node.kind = PAINT_GLYPH;
                node.glyph_id = *glyph_id;
                kids.push(self.flatten(child));
            }
            P::ColrGlyph { glyph_id, child } => {
                node.kind = PAINT_COLR_GLYPH;
                node.glyph_id = *glyph_id;
                kids.push(self.flatten(child));
            }
            P::Solid { is_foreground, r, g, b, alpha } => {
                node.kind = PAINT_SOLID;
                node.is_foreground = u8::from(*is_foreground);
                node.r = *r;
                node.g = *g;
                node.b = *b;
                node.alpha = *alpha;
            }
            P::LinearGradient { extend, stops, x0, y0, x1, y1, x2, y2 } => {
                node.kind = PAINT_LINEAR_GRADIENT;
                node.extend = *extend;
                node.numbers[..6].copy_from_slice(&[*x0, *y0, *x1, *y1, *x2, *y2]);
                (node.stops_start, node.stops_count) = self.push_stops(stops);
            }
            P::RadialGradient { extend, stops, x0, y0, r0, x1, y1, r1 } => {
                node.kind = PAINT_RADIAL_GRADIENT;
                node.extend = *extend;
                node.numbers[..6].copy_from_slice(&[*x0, *y0, *r0, *x1, *y1, *r1]);
                (node.stops_start, node.stops_count) = self.push_stops(stops);
            }
            P::SweepGradient { extend, stops, cx, cy, start_angle, end_angle } => {
                node.kind = PAINT_SWEEP_GRADIENT;
                node.extend = *extend;
                node.numbers[..4].copy_from_slice(&[*cx, *cy, *start_angle, *end_angle]);
                (node.stops_start, node.stops_count) = self.push_stops(stops);
            }
            P::Transform { child, matrix } => {
                node.kind = PAINT_TRANSFORM;
                node.numbers[..6].copy_from_slice(matrix);
                kids.push(self.flatten(child));
            }
            P::Translate { child, dx, dy } => {
                node.kind = PAINT_TRANSLATE;
                node.numbers[0] = *dx;
                node.numbers[1] = *dy;
                kids.push(self.flatten(child));
            }
            P::Scale { child, sx, sy, center } => {
                node.kind = PAINT_SCALE;
                node.numbers[0] = *sx;
                node.numbers[1] = *sy;
                Self::set_center(&mut node, *center);
                kids.push(self.flatten(child));
            }
            P::ScaleUniform { child, s, center } => {
                node.kind = PAINT_SCALE_UNIFORM;
                node.numbers[0] = *s;
                Self::set_center(&mut node, *center);
                kids.push(self.flatten(child));
            }
            P::Rotate { child, angle, center } => {
                node.kind = PAINT_ROTATE;
                node.numbers[0] = *angle;
                Self::set_center(&mut node, *center);
                kids.push(self.flatten(child));
            }
            P::Skew { child, x_angle, y_angle, center } => {
                node.kind = PAINT_SKEW;
                node.numbers[0] = *x_angle;
                node.numbers[1] = *y_angle;
                Self::set_center(&mut node, *center);
                kids.push(self.flatten(child));
            }
            P::Composite { source, mode, backdrop } => {
                node.kind = PAINT_COMPOSITE;
                node.composite_mode = *mode;
                kids.push(self.flatten(source));
                kids.push(self.flatten(backdrop));
            }
        }

        node.child_start = self.children.len() as u32;
        node.child_count = kids.len() as u32;
        self.children.extend_from_slice(&kids);
        self.nodes[me as usize] = node;
        me
    }

    fn set_center(node: &mut PaintNode, center: Option<(f64, f64)>) {
        if let Some((cx, cy)) = center {
            node.has_center = 1;
            node.numbers[4] = cx;
            node.numbers[5] = cy;
        }
    }

    fn push_stops(&mut self, stops: &[crate::ColorStop]) -> (u32, u32) {
        let start = self.stop_offsets.len() as u32;
        for s in stops {
            self.stop_offsets.push(s.offset);
            self.stop_colors.extend_from_slice(&[s.r, s.g, s.b, s.alpha]);
        }
        (start, stops.len() as u32)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_colr_v1_paint(
    font: *const Font,
    gid: u16,
    axes: *const Axis,
    axes_len: usize,
    palette_index: u16,
    out: *mut *mut PaintHandle,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let Some(paint) = font.colr_v1_paint(gid, &location, palette_index) else {
        return Status::Absent;
    };
    let mut built = PaintHandle {
        nodes: Vec::new(),
        children: Vec::new(),
        stop_offsets: Vec::new(),
        stop_colors: Vec::new(),
    };
    built.flatten(&paint);
    unsafe { deliver(out, built) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_paint_nodes(
    p: *const PaintHandle,
    out_count: *mut usize,
) -> *const PaintNode {
    let Some(p) = (unsafe { borrow(p) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = p.nodes.len() };
    p.nodes.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_paint_children(
    p: *const PaintHandle,
    out_count: *mut usize,
) -> *const u32 {
    let Some(p) = (unsafe { borrow(p) }) else { return core::ptr::null() };
    if out_count.is_null() {
        return core::ptr::null();
    }
    unsafe { *out_count = p.children.len() };
    p.children.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_paint_stops(
    p: *const PaintHandle,
    out_count: *mut usize,
    out_offsets: *mut *const f64,
    out_colors: *mut *const u8,
) -> Status {
    let Some(p) = (unsafe { borrow(p) }) else { return Status::Null };
    unsafe {
        if !out_count.is_null() {
            *out_count = p.stop_offsets.len();
        }
        if !out_offsets.is_null() {
            *out_offsets = p.stop_offsets.as_ptr();
        }
        if !out_colors.is_null() {
            *out_colors = p.stop_colors.as_ptr();
        }
    }
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_paint_free(p: *mut PaintHandle) {
    unsafe { release(p) }
}

pub struct Scene {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    left: i32,
    top: i32,
    skipped_ops: usize,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_font_render_colr_glyph(
    font: *const Font,
    gid: u16,
    px: f32,
    axes: *const Axis,
    axes_len: usize,
    palette_index: u16,
    out: *mut *mut Scene,
) -> Status {
    let Some(font) = (unsafe { borrow(font) }) else { return Status::Null };
    let location = unsafe { axes_of(axes, axes_len) };
    let Some(s) = font.render_colr_glyph(gid, px, &location, palette_index) else {
        return Status::Absent;
    };
    unsafe {
        deliver(
            out,
            Scene {
                width: s.width,
                height: s.height,
                rgba: s.rgba,
                left: s.left,
                top: s.top,
                skipped_ops: s.skipped_ops,
            },
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_rgba(
    s: *const Scene,
    out_len: *mut usize,
    out_width: *mut usize,
    out_height: *mut usize,
    out_left: *mut i32,
    out_top: *mut i32,
    out_skipped_ops: *mut usize,
) -> *const u8 {
    let Some(s) = (unsafe { borrow(s) }) else { return core::ptr::null() };
    if out_len.is_null() {
        return core::ptr::null();
    }
    unsafe {
        *out_len = s.rgba.len();
        if !out_width.is_null() {
            *out_width = s.width;
        }
        if !out_height.is_null() {
            *out_height = s.height;
        }
        if !out_left.is_null() {
            *out_left = s.left;
        }
        if !out_top.is_null() {
            *out_top = s.top;
        }
        if !out_skipped_ops.is_null() {
            *out_skipped_ops = s.skipped_ops;
        }
    }
    s.rgba.as_ptr()
}

pub(crate) fn wrap_scene(s: crate::RenderedScene) -> Scene {
    Scene {
        width: s.width,
        height: s.height,
        rgba: s.rgba,
        left: s.left,
        top: s.top,
        skipped_ops: s.skipped_ops,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_free(s: *mut Scene) {
    unsafe { release(s) }
}

pub struct SceneBuilder(crate::paint::DisplayList);

#[unsafe(no_mangle)]
pub extern "C" fn daegun_scene_builder_new() -> *mut SceneBuilder {
    alloc::boxed::Box::into_raw(alloc::boxed::Box::new(SceneBuilder(crate::paint::DisplayList::default())))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_builder_free(b: *mut SceneBuilder) {
    unsafe { release(b) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_builder_push_path(
    b: *mut SceneBuilder,
    path: *const crate::ffi::outline::Path,
    out_id: *mut usize,
) -> Status {
    if b.is_null() || out_id.is_null() {
        return Status::Null;
    }
    let Some(path) = (unsafe { borrow(path) }) else { return Status::Null };
    let b = unsafe { &mut *b };
    let id = b.0.push_path(path.0.clone());
    unsafe { *out_id = id };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_builder_fill(
    b: *mut SceneBuilder,
    path_id: usize,
    rgba: *const u8,
    rule: i32,
    transform: *const f64,
) -> Status {
    if b.is_null() || rgba.is_null() || transform.is_null() {
        return Status::Null;
    }
    let b = unsafe { &mut *b };
    if b.0.path(path_id).is_none() {
        return Status::Range;
    }
    let rule = match rule {
        0 => crate::FillRule::NonZero,
        1 => crate::FillRule::EvenOdd,
        _ => return Status::Range,
    };
    // The caller promises four bytes and six doubles, per the header.
    let c = unsafe { core::slice::from_raw_parts(rgba, 4) };
    let t = unsafe { core::slice::from_raw_parts(transform, 6) };
    if !t.iter().all(|v| v.is_finite()) {
        return Status::Range;
    }
    b.0.push(crate::paint::Op::Fill {
        path:      path_id,
        paint:     crate::paint::Paint::Solid(crate::paint::Rgba {
            r: c[0], g: c[1], b: c[2], a: c[3],
        }),
        rule,
        transform: [t[0], t[1], t[2], t[3], t[4], t[5]],
    });
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_builder_render(
    b: *const SceneBuilder,
    px: f32,
    upem: f32,
    out: *mut *mut Scene,
) -> Status {
    let Some(b) = (unsafe { borrow(b) }) else { return Status::Null };
    let Some(s) = crate::paint::render(&b.0, px, upem) else { return Status::Range };
    unsafe { deliver(out, wrap_scene(s)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_builder_is_empty(
    b: *const SceneBuilder,
    out: *mut bool,
) -> Status {
    let Some(b) = (unsafe { borrow(b) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = b.0.is_empty() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_builder_op_count(
    b: *const SceneBuilder,
    out: *mut usize,
) -> Status {
    let Some(b) = (unsafe { borrow(b) }) else { return Status::Null };
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = b.0.ops().len() };
    Status::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_scene_builder_path(
    b: *const SceneBuilder,
    path_id: usize,
    out: *mut *mut crate::ffi::outline::Path,
) -> Status {
    let Some(b) = (unsafe { borrow(b) }) else { return Status::Null };
    let Some(p) = b.0.path(path_id) else { return Status::Range };
    unsafe { deliver(out, crate::ffi::outline::Path(p.clone())) }
}
