// Unconditional, including under test, so the compiler enforces it rather than a
// `--no-default-features` build nobody runs. Tests read fixtures off disk, so they get std back.
#![no_std]
// `deny`, not `forbid`, only because two subtrees opt back in – `daerizer`, which talks to Metal,
// Vulkan and Direct3D, and `ffi`, which turns C pointers into references. Everything else,
// including the whole engine in `daecore`, is `forbid`, which no inner `allow` can override.
#![deny(unsafe_code)]
// A font is untrusted input and a panic is a denial of service that `forbid(unsafe_code)` does
// nothing about. Gated on `not(test)` because the test bodies are `#[path]`-included into this
// crate, so ungated it fires on 2,486 `.unwrap()`s in test files and buries the real ones.
#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

#[cfg_attr(not(test), macro_use)]
extern crate alloc;

#[doc(hidden)]
#[path = "../daecore/src/mod.rs"]
pub mod daecore;

#[doc(hidden)]
#[path = "../daerizer/src/mod.rs"]
pub mod daerizer;

#[cfg(feature = "capi")]
#[path = "../c-wrapper/mod.rs"]
mod ffi;

#[cfg(test)]
#[macro_use]
extern crate std;

#[cfg(all(feature = "std", not(test)))]
extern crate std;

pub(crate) use crate::daecore::{cache, daeshaper, sync};

mod glyphcache;
mod text;
mod api;
pub use daerizer as paint;
pub use text::{
    grapheme_boundaries, line_break_opportunities, resolve_bidi, word_boundaries, BidiParagraph,
    LineBreak, ShapeOptions, Ignorables,
    line_visual_runs, script_runs, GeneralCategory, Script, ScriptRun, VisualRun,
    general_category, is_upright, vertical_form,
};
pub use daeshaper::buffer::ClusterLevel;
pub use api::{
    Font, FontError, SubsetResult, Paint, ColorStop, GlyphBitmap, ColrLayer, PaletteInfo,
    BaseScriptInfo, StatAxis, StatAxisValue, StatInfo, MathKernCorner, NamedInstance, ShapedRun,
    GlyphPart, GlyphAssembly, MathGlyphVariant, MathGlyphConstruction, MathConstants, JstfModLists,
    Metrics, RasterizedGlyph, FvarAxis, SubSuperMetrics, TypographicMetrics, GlyphClass,
    RasterOptions, SubpixelLayout, StripeOrder, MAX_OVERSAMPLE, HintMode, Rect, ShelfPacker, Justified, JustifyOptions, BidiRun,
    DrawTarget, DrawnGlyph, Policy, Prefer, Refusal, Rendered, Request, DeviceKind, DeviceProfile, route,
    Cap, Join, StrokeStyle, stroke, stroke_simplified,
    Os2Info, WinMetrics, TypoLineMetrics,
    HintedOutline, draw_hinted, FLAG_ON_CURVE,
    CffHints, CffStem,
    FillRule, Path, TransformPen, Verb,
    RenderedScene,
    binding, eval, shader, Band, ColorSlot, CurvePoint, GlyphInstance, GlyphSlot, GpuBatch,
    ShaderLanguage, ShaderStage, SubpixelParams, GpuGlyphError, MAX_CURVES_PER_GLYPH,
    MAX_SUBPIXEL_WEIGHTS, MAX_SUBPIXEL_TAPS, MAX_SUPERSAMPLE, HullVertex, HULL_VERTICES,
    LineMetrics, Align, BreakStrategy, LayoutLine, LayoutOptions, PositionedRun, TextLayout,
    TextOrientation, WritingMode,
    DEFAULT_POINT_SIZE,
};
pub use crate::daecore::daetype::outline::OutlinePen;
pub use api::{outline_glyf_bytes, parse_loca};
pub use api::bytes;
pub use api::format;
pub use api::build_font;
pub use crate::daerizer::DisplayList as ColorScene;
