#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::error::Error;
use core::fmt;
use crate::cache::FontCache;
use crate::glyphcache;

pub use crate::daecore::daetype::subsetter::SubsetResult;
pub use crate::daecore::daetype::colr_v1::{Paint, ColorStop};
pub use crate::daecore::daetype::bitmap::GlyphBitmap;
pub use crate::daecore::daetype::colr_v0::{ColrLayer, PaletteInfo};
pub use crate::daecore::daetype::base::BaseScriptInfo;
pub use crate::daecore::daetype::stat::{StatAxis, StatAxisValue};
pub use crate::daecore::daetype::math_table::MathKernCorner;
pub use crate::daecore::daetype::decoder::{NamedInstance, FvarAxis};
pub use crate::daecore::daetype::jstf::JstfModLists;
pub use crate::text::shape::ShapedRun;
pub use crate::text::justify::{Justified, JustifyOptions};
pub use crate::cache::LineMetrics;
pub use crate::text::layout::{
    Align, BreakStrategy, LayoutLine, LayoutOptions, PositionedRun, TextLayout, TextOrientation,
    WritingMode,
};

#[derive(Debug, Clone)]
pub struct BidiRun {
    pub run: ShapedRun,
    pub level: u8,
    pub chars: Vec<usize>,
}
pub use crate::daerizer::daecpu::rasterize::Metrics;
pub use crate::sync::Shared;
pub use crate::daecore::daemachine::subpixel::{StripeOrder, SubpixelLayout};
pub use crate::daerizer::draw::{route, DeviceKind, DeviceProfile, Policy, Prefer, Refusal, Rendered, Request};
pub use draw::{DrawTarget, DrawnGlyph};
pub use crate::daecore::daemachine::subpixel::MAX_OVERSAMPLE;
pub use crate::daecore::daetype::hinting::HintMode;
pub use crate::daecore::daetype::outline::{Cap, Join, StrokeStyle};
pub use crate::daecore::daetype::outline::{FillRule, Path, TransformPen, Verb};
pub use crate::daecore::daetype::outline::outline_glyf_bytes;
pub use crate::daecore::daetype::subsetter::parse_loca;
pub use crate::daecore::daetype::outline::{stroke, stroke_simplified};
pub use crate::glyphcache::cache::{Rect, ShelfPacker};
pub use crate::daerizer::RenderedScene;

#[repr(C)]
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ColorSlot {
    pub slot: GlyphSlot,
    pub tint: [f32; 4],
}
pub use crate::daerizer::daegpu::{
    binding, eval, Band, CurvePoint, GlyphInstance, GlyphSlot, GpuBatch, ShaderLanguage,
    ShaderStage, SubpixelParams, GpuGlyphError, MAX_CURVES_PER_GLYPH, MAX_SUBPIXEL_WEIGHTS,
    HullVertex, HULL_VERTICES,
    MAX_SUBPIXEL_TAPS, MAX_SUPERSAMPLE,
};
pub use crate::daerizer::daegpu::shader;

#[derive(Debug)]
pub struct FontError(String);

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for FontError {}

impl From<String> for FontError {
    fn from(s: String) -> Self { FontError(s) }
}

#[derive(Debug, PartialEq)]
pub struct StatInfo {
    pub axes:                 Vec<StatAxis>,
    pub values:                Vec<StatAxisValue>,
    pub elided_fallback_name: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct GlyphPart {
    pub glyph_id:               u16,
    pub start_connector_length: f64,
    pub end_connector_length:   f64,
    pub full_advance:           f64,
    pub is_extender:            bool,
}

#[derive(Debug, PartialEq)]
pub struct GlyphAssembly {
    pub italics_correction: f64,
    pub parts:              Vec<GlyphPart>,
}

#[derive(Debug, PartialEq)]
pub struct MathGlyphVariant {
    pub glyph_id: u16,
    pub advance:  f64,
}

#[derive(Debug, PartialEq)]
pub struct MathGlyphConstruction {
    pub assembly: Option<GlyphAssembly>,
    pub variants: Vec<MathGlyphVariant>,
}

#[derive(Debug)]
pub struct MathConstants {
    pub script_percent_scale_down: f64,
    pub script_script_percent_scale_down: f64,
    pub delimited_sub_formula_min_height: f64,
    pub display_operator_min_height: f64,
    pub math_leading: f64,
    pub axis_height: f64,
    pub accent_base_height: f64,
    pub flattened_accent_base_height: f64,
    pub subscript_shift_down: f64,
    pub subscript_top_max: f64,
    pub subscript_baseline_drop_min: f64,
    pub superscript_shift_up: f64,
    pub superscript_shift_up_cramped: f64,
    pub superscript_bottom_min: f64,
    pub superscript_baseline_drop_max: f64,
    pub sub_superscript_gap_min: f64,
    pub superscript_bottom_max_with_subscript: f64,
    pub space_after_script: f64,
    pub upper_limit_gap_min: f64,
    pub upper_limit_baseline_rise_min: f64,
    pub lower_limit_gap_min: f64,
    pub lower_limit_baseline_drop_min: f64,
    pub stack_top_shift_up: f64,
    pub stack_top_display_style_shift_up: f64,
    pub stack_bottom_shift_down: f64,
    pub stack_bottom_display_style_shift_down: f64,
    pub stack_gap_min: f64,
    pub stack_display_style_gap_min: f64,
    pub stretch_stack_top_shift_up: f64,
    pub stretch_stack_bottom_shift_down: f64,
    pub stretch_stack_gap_above_min: f64,
    pub stretch_stack_gap_below_min: f64,
    pub fraction_numerator_shift_up: f64,
    pub fraction_numerator_display_style_shift_up: f64,
    pub fraction_denominator_shift_down: f64,
    pub fraction_denominator_display_style_shift_down: f64,
    pub fraction_numerator_gap_min: f64,
    pub fraction_num_display_style_gap_min: f64,
    pub fraction_rule_thickness: f64,
    pub fraction_denominator_gap_min: f64,
    pub fraction_denom_display_style_gap_min: f64,
    pub skewed_fraction_horizontal_gap: f64,
    pub skewed_fraction_vertical_gap: f64,
    pub overbar_vertical_gap: f64,
    pub overbar_rule_thickness: f64,
    pub overbar_extra_ascender: f64,
    pub underbar_vertical_gap: f64,
    pub underbar_rule_thickness: f64,
    pub underbar_extra_descender: f64,
    pub radical_vertical_gap: f64,
    pub radical_display_style_vertical_gap: f64,
    pub radical_rule_thickness: f64,
    pub radical_extra_ascender: f64,
    pub radical_kern_before_degree: f64,
    pub radical_kern_after_degree: f64,
    pub radical_degree_bottom_raise_percent: f64,
}

#[derive(Clone, PartialEq, Debug)]
pub struct RasterizedGlyph {
    pub metrics: Metrics,
    pub bitmap: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinMetrics {
    pub ascent:  i32,
    pub descent: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypoLineMetrics {
    pub ascender:  i32,
    pub descender: i32,
    pub line_gap:  i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Os2Info {
    pub version: u16,
    pub family_class: Option<u16>,
    pub selection: Option<u16>,
    pub win_metrics: Option<WinMetrics>,
    pub typo_metrics: Option<TypoLineMetrics>,
}

impl Os2Info {
    fn selected(&self, bit: u16) -> bool {
        self.selection.is_some_and(|v| v & bit != 0)
    }

    pub fn is_italic(&self) -> bool {
        self.selected(0x0001)
    }

    pub fn is_bold(&self) -> bool {
        self.selected(0x0020)
    }

    pub fn is_regular(&self) -> bool {
        self.selected(0x0040)
    }

    pub fn is_oblique(&self) -> bool {
        self.selected(0x0200)
    }

    pub fn uses_typo_metrics(&self) -> bool {
        self.selected(0x0080)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphClass {
    Base,
    Ligature,
    Mark,
    Component,
}

#[derive(Debug, Default, PartialEq)]
pub struct SubSuperMetrics {
    pub x_size:   i32,
    pub y_size:   i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

#[derive(Debug, PartialEq)]
pub struct TypographicMetrics {
    pub x_height:            i32,
    pub underline_position:  i32,
    pub underline_thickness: i32,
    pub strikeout_size:      i32,
    pub strikeout_position:  i32,
    pub subscript:           SubSuperMetrics,
    pub superscript:         SubSuperMetrics,
}

#[non_exhaustive]
#[derive(Clone, Copy, Default, Debug)]
pub struct RasterOptions {
    pub layout: SubpixelLayout,
    pub gamma: Option<f32>,
    pub transform: Option<[f32; 6]>,
    pub hinting: HintMode,
    pub stroke: Option<StrokeStyle>,
    pub embolden: Option<f32>,
    pub oblique: Option<f32>,
}

impl RasterOptions {
    pub fn with_layout(mut self, layout: SubpixelLayout) -> RasterOptions {
        self.layout = layout;
        self
    }

    pub fn with_gamma(mut self, gamma: f32) -> RasterOptions {
        self.gamma = Some(gamma);
        self
    }

    pub fn with_transform(mut self, transform: [f32; 6]) -> RasterOptions {
        self.transform = Some(transform);
        self
    }

    pub fn with_hinting(mut self, hinting: HintMode) -> RasterOptions {
        self.hinting = hinting;
        self
    }

    pub fn with_stroke(mut self, stroke: StrokeStyle) -> RasterOptions {
        self.stroke = Some(stroke);
        self
    }

    pub fn with_embolden(mut self, units: f32) -> RasterOptions {
        self.embolden = Some(units);
        self
    }

    pub fn with_oblique(mut self, tangent: f32) -> RasterOptions {
        self.oblique = Some(tangent);
        self
    }

}

fn transform_max_scale(t: &[f32; 6]) -> f32 {
    let abs = |v: f32| f32::from_bits(v.to_bits() & 0x7fff_ffff);
    let row0 = abs(t[0]) + abs(t[1]);
    let row1 = abs(t[2]) + abs(t[3]);
    row0.max(row1).max(f32::MIN_POSITIVE)
}

fn four_bytes(tag: &str) -> Option<[u8; 4]> {
    tag.as_bytes().try_into().ok()
}

fn owned_axes(axes: &[(&str, f64)]) -> Vec<(String, f64)> {
    axes.iter()
        .map(|&(tag, v)| (crate::daecore::cache::normalize_tag(tag), v))
        .collect()
}

fn draw_glyph_outline(cache: &FontCache, gid: u16, pen: &mut dyn crate::daecore::daetype::outline::OutlinePen) -> Option<()> {
    match cache.outline_format {
        crate::daecore::cache::OutlineFormat::Cff => {
            let cff = cache.cff()?;
            let outlines = cache.cff_outlines()?;
            crate::daecore::daetype::outline::outline_cff_glyph_with(&outlines, cff, gid, pen).ok()
        }
        crate::daecore::cache::OutlineFormat::Glyf => {
            let loca = cache.loca_offsets()?;
            cache.draw_glyf_reusing(&loca, gid, pen).ok()
        }
        crate::daecore::cache::OutlineFormat::Neither => None,
    }
}

// A counter rather than anything derived from the font's bytes: an address can be reused once a
// `Font` drops, and a batch can outlive the font that filled it.
static NEXT_FONT_ID: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(1);

pub struct Font {
    id: usize,
    cache: FontCache,
    is_cff2: bool,
    glyphs: crate::sync::Mutable<glyphcache::cache::GlyphCache>,
    gpu_curves: crate::sync::Mutable<glyphcache::cache::ByteLru<
        crate::daerizer::daegpu::GpuGlyphKey,
        crate::sync::Shared<crate::daerizer::daegpu::BuiltGlyph>,
    >>,
    outlines: crate::sync::Mutable<OutlineCache>,
    raster_scratch: crate::sync::Mutable<Option<(crate::daerizer::daecpu::rasterize::Raster, crate::daerizer::daecpu::math::Glyph)>>,
    gamma: crate::sync::Mutable<Option<(u32, [u8; 256])>>,
}

// A full 12 to 96 pixel page zoom of a text document holds about 6.9 MB of rasterized glyphs, so a
// 4 MB budget evicted continuously through one. Measured: 8 MB removes that, and 16 adds nothing.
const DEFAULT_GLYPH_CACHE_BYTES: usize = 8 * 1024 * 1024;

mod metrics;
mod glyphs;
mod text;
mod color;
mod raster;
mod curve;
mod draw;
mod tables;
mod raw;

pub use raw::{build_font, bytes, format};

pub use crate::daecore::daetype::hinting::{draw_hinted, HintedOutline, FLAG_ON_CURVE};
pub use crate::daecore::daetype::outline::{CffHints, CffStem};

pub use crate::daecore::daetype::trak::DEFAULT_POINT_SIZE;

const CURVE_CACHE_BYTES: usize = 4 * 1024 * 1024;

type OutlineCache = glyphcache::cache::ByteLru<
    (u16, crate::sync::Shared<crate::daecore::cache::AxisKey>),
    crate::sync::Shared<crate::daecore::daetype::outline::Path>,
>;

const OUTLINE_CACHE_BYTES: usize = 4 * 1024 * 1024;

impl Font {
    fn wrap(cache: FontCache) -> Font {
        let is_cff2 = cache.table_map.contains_key("CFF2");
        Font {
            id: NEXT_FONT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed),
            cache,
            is_cff2,
            glyphs: crate::sync::mutable(glyphcache::cache::glyph_cache(DEFAULT_GLYPH_CACHE_BYTES)),
            gpu_curves: crate::sync::mutable(glyphcache::cache::ByteLru::new(
                CURVE_CACHE_BYTES,
                |g: &crate::sync::Shared<crate::daerizer::daegpu::BuiltGlyph>| g.cost(),
            )),
            outlines: crate::sync::mutable(glyphcache::cache::ByteLru::new(
                OUTLINE_CACHE_BYTES,
                |p: &crate::sync::Shared<crate::daecore::daetype::outline::Path>| p.cost() + 64,
            )),
            raster_scratch: crate::sync::mutable(None),
            gamma: crate::sync::mutable(None),
        }
    }

    pub fn set_glyph_cache_bytes(&self, bytes: usize) {
        crate::sync::write(&self.glyphs).set_budget(bytes);
    }

    pub fn clear_glyph_cache(&self) {
        crate::sync::write(&self.glyphs).clear();
    }

    pub fn glyph_cache_stats(&self) -> (usize, usize) {
        let c = crate::sync::read(&self.glyphs);
        (c.len(), c.bytes())
    }

    // Each is a ceiling grown into, never reserved, and each is dead weight for some caller: a
    // CPU-only app never fills the curve cache, nor a fixed-weight one the variable instances.
    pub fn set_curve_cache_bytes(&self, bytes: usize) {
        crate::sync::write(&self.gpu_curves).set_budget(bytes);
    }

    pub fn clear_curve_cache(&self) {
        crate::sync::write(&self.gpu_curves).clear();
    }

    pub fn curve_cache_stats(&self) -> (usize, usize) {
        let c = crate::sync::read(&self.gpu_curves);
        (c.len(), c.bytes())
    }

    // Emptied by `clear_prewarm`, which is what fills it.
    pub fn set_outline_cache_bytes(&self, bytes: usize) {
        crate::sync::write(&self.outlines).set_budget(bytes);
    }

    pub fn outline_cache_stats(&self) -> (usize, usize) {
        let c = crate::sync::read(&self.outlines);
        (c.len(), c.bytes())
    }

    pub fn set_shape_cache_bytes(&self, bytes: usize) {
        self.cache.set_shape_cache_bytes(bytes);
    }

    pub fn clear_shape_cache(&self) {
        self.cache.clear_shape_cache();
    }

    pub fn shape_cache_stats(&self) -> (usize, usize) {
        self.cache.shape_cache_stats()
    }

    pub fn set_instance_cache_bytes(&self, bytes: usize) {
        self.cache.set_instance_cache_bytes(bytes);
    }

    // Two figures, because a variable font is cached twice over: the location it was instanced to
    // and the instanced tables themselves.
    pub fn instance_cache_stats(&self) -> (usize, usize) {
        self.cache.instance_cache_stats()
    }

    // What is left to spend rather than a ceiling: building a cmap index draws this down and never
    // returns it, so setting it grants a fresh allowance.
    pub fn set_cmap_index_allowance(&self, bytes: usize) {
        self.cache.set_cmap_index_allowance(bytes);
    }

    pub fn cmap_index_allowance(&self) -> usize {
        self.cache.cmap_index_allowance()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Font, FontError> {
        let table_map = crate::daecore::daetype::decoder::extract_ttf_tables(bytes)?;
        Ok(Font::wrap(FontCache::new(table_map)))
    }

    pub fn from_vec(bytes: alloc::vec::Vec<u8>) -> Result<Font, FontError> {
        let table_map = crate::daecore::daetype::decoder::extract_ttf_tables_owned(bytes)?;
        Ok(Font::wrap(FontCache::new(table_map)))
    }

    pub fn from_ttc(bytes: &[u8], index: usize) -> Result<Font, FontError> {
        let table_map = crate::daecore::daetype::decoder::extract_ttc_tables(bytes, index)?;
        Ok(Font::wrap(FontCache::new(table_map)))
    }

    pub fn ttc_font_count(bytes: &[u8]) -> usize {
        crate::daecore::daetype::decoder::ttc_font_count(bytes)
    }
}
