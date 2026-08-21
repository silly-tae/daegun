pub mod backend;
pub mod band;
pub mod eval;
pub mod extract;
pub mod hull;

#[cfg(target_vendor = "apple")]
mod metal;
#[cfg(target_vendor = "apple")]
mod objc;
#[cfg(target_vendor = "apple")]
pub mod ffi;

mod vulkan;
pub mod vk;

#[cfg(windows)]
mod direct3d;
#[cfg(windows)]
pub mod d3d11;
#[cfg(windows)]
pub mod d3d12;

#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use alloc::vec::Vec;
use band::Banded;
use extract::Quad;
use crate::daecore::daemachine::subpixel::SubpixelLayout;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Grayscale,
    Subpixel,
}

pub use extract::MAX_CURVES_PER_GLYPH;
pub use extract::Reject;
pub use hull::{HULL_VERTICES, HullVertex};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[non_exhaustive]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub struct Band {
    pub first_curve: u32,
    pub curve_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[non_exhaustive]
pub struct GlyphSlot {
    pub band_base: u32,
    pub h_bands: u32,
    pub v_bands: u32,
    pub hull_base: u32,
    pub box_min: [f32; 2],
    pub box_max: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[non_exhaustive]
pub struct GlyphInstance {
    pub glyph_box: [f32; 4],
    pub tint: [f32; 4],
    pub offset: [f32; 2],
    pub em_pixels: [f32; 2],
    pub scale: f32,
    pub band_base: u32,
    pub bands_per_axis: u32,
    pub hull_base: u32,
    // The fragment stage must multiply by this, never divide by `scale`. Numerators are provably
    // bit-identical and the denominator is one constant, so division – the loosest arithmetic these
    // APIs require – is the only place a difference enters. An AMD Vega 2 measured 0.00218 of 255.
    pub inv_scale: f32,
    // Three scalars in the shaders, never a 3-vector: that aligns to 16, lands at offset 80 and
    // makes the shader struct 96 against this 80. The symptom is a wrong *stride*, so the first
    // instance of a draw reads correctly and every one after it does not.
    pub _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
#[non_exhaustive]
pub struct SubpixelParams {
    pub weights: [f32; MAX_SUBPIXEL_WEIGHTS * 3],
    pub oversample: [u32; 2],
    pub taps: [u32; 2],
    pub origin: [i32; 2],
    pub channels: u32,
    pub supersample: u32,
}

// Written out rather than derived, because the same numbers are encoded again in the three shaders
// and in three compiled .spv files, and a shader cannot follow a Rust constant. Deriving would
// propagate a change to here and stop, leaving the GPU indexing one size against another – which
// shows up as color fringing, not as an error. The relationship is asserted below instead.
pub const MAX_SUBPIXEL_WEIGHTS: usize = 64;

const _: () = assert!(
    MAX_SUBPIXEL_WEIGHTS == crate::daecore::daemachine::subpixel::MAX_WEIGHTS,
    "MAX_SUBPIXEL_WEIGHTS no longer matches SubpixelLayout's table; the shaders encode it too",
);
const _: () = assert!(
    MAX_SUBPIXEL_TAPS as usize == crate::daecore::daemachine::subpixel::MAX_TAPS,
    "MAX_SUBPIXEL_TAPS no longer matches SubpixelLayout's table; the shaders encode it too",
);

pub const MAX_SUBPIXEL_TAPS: u32 = 8;

pub const MAX_SUPERSAMPLE: u32 = 4;

impl Default for SubpixelParams {
    fn default() -> Self {
        SubpixelParams::from_layout(&SubpixelLayout::grayscale())
    }
}

impl SubpixelParams {
    pub fn from_layout(layout: &SubpixelLayout) -> SubpixelParams {
        let (ox, oy) = layout.oversample();
        let (taps_x, taps_y) = layout.taps();
        let (origin_x, origin_y) = layout.origin();

        let mut weights = [0.0; MAX_SUBPIXEL_WEIGHTS * 3];
        for c in 0..layout.channels() as usize {
            let Some(src) = layout.weights(c) else { continue };
            let base = c * MAX_SUBPIXEL_WEIGHTS;
            let n = src.len().min(MAX_SUBPIXEL_WEIGHTS);
            if let Some(dst) = weights.get_mut(base..base + n) {
                dst.copy_from_slice(&src[..n]);
            }
        }

        SubpixelParams {
            weights,
            oversample: [ox as u32, oy as u32],
            taps: [taps_x as u32, taps_y as u32],
            origin: [origin_x as i32, origin_y as i32],
            channels: layout.channels() as u32,
            supersample: 1,
        }
    }

    pub fn with_supersampling(mut self, n: u32) -> SubpixelParams {
        self.supersample = n.clamp(1, MAX_SUPERSAMPLE);
        self
    }

    pub fn dilation(&self) -> [f32; 2] {
        let pad = |origin: i32, oversample: u32| {
            if origin >= 0 || oversample == 0 {
                return 0.5;
            }
            let whole = (origin.unsigned_abs() as usize).div_ceil(oversample as usize);
            (whole as f32).max(0.5)
        };
        [
            pad(self.origin[0], self.oversample[0]),
            pad(self.origin[1], self.oversample[1]),
        ]
    }
}

const _: () = assert!(core::mem::size_of::<CurvePoint>() == 8);
const _: () = assert!(core::mem::align_of::<CurvePoint>() == 4);
const _: () = assert!(core::mem::size_of::<Band>() == 8);
const _: () = assert!(core::mem::align_of::<Band>() == 4);
const _: () = assert!(core::mem::size_of::<GlyphInstance>() == 80);
const _: () = assert!(core::mem::align_of::<GlyphInstance>() == 4);
const _: () = assert!(core::mem::size_of::<HullVertex>() == 24);
const _: () = assert!(core::mem::align_of::<HullVertex>() == 4);
const _: () = assert!(core::mem::size_of::<SubpixelParams>() == 800);
const _: () = assert!(core::mem::align_of::<SubpixelParams>() == 4);

pub mod binding {
    pub const CURVES: u32 = 0;
    pub const BAND_CURVES: u32 = 1;
    pub const BANDS: u32 = 2;
    pub const INSTANCES: u32 = 3;
    pub const SUBPIXEL: u32 = 4;
    pub const HULL: u32 = 5;

}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShaderLanguage {
    Glsl,
    Hlsl,
    Metal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    SubpixelFragment,
}

pub const fn shader(language: ShaderLanguage, stage: ShaderStage) -> &'static str {
    match (language, stage) {
        (ShaderLanguage::Glsl, ShaderStage::Vertex) => {
            concat!("#define DAEGUN_VERTEX 1\n", include_str!("shaders/daegun.glsl"))
        }
        (ShaderLanguage::Glsl, ShaderStage::Fragment) => {
            concat!("#define DAEGUN_FRAGMENT 1\n", include_str!("shaders/daegun.glsl"))
        }
        (ShaderLanguage::Glsl, ShaderStage::SubpixelFragment) => {
            concat!("#define DAEGUN_SUBPIXEL 1\n", include_str!("shaders/daegun.glsl"))
        }
        (ShaderLanguage::Hlsl, ShaderStage::Vertex) => {
            concat!("#define DAEGUN_VERTEX 1\n", include_str!("shaders/daegun.hlsl"))
        }
        (ShaderLanguage::Hlsl, ShaderStage::Fragment) => {
            concat!("#define DAEGUN_FRAGMENT 1\n", include_str!("shaders/daegun.hlsl"))
        }
        (ShaderLanguage::Hlsl, ShaderStage::SubpixelFragment) => {
            concat!("#define DAEGUN_SUBPIXEL 1\n", include_str!("shaders/daegun.hlsl"))
        }
        (ShaderLanguage::Metal, ShaderStage::Vertex) => {
            concat!("#define DAEGUN_VERTEX 1\n", include_str!("shaders/daegun.metal"))
        }
        (ShaderLanguage::Metal, ShaderStage::Fragment) => {
            concat!("#define DAEGUN_FRAGMENT 1\n", include_str!("shaders/daegun.metal"))
        }
        (ShaderLanguage::Metal, ShaderStage::SubpixelFragment) => {
            concat!("#define DAEGUN_SUBPIXEL 1\n", include_str!("shaders/daegun.metal"))
        }
    }
}

impl GlyphSlot {
    pub fn instance(
        &self,
        offset: [f32; 2],
        scale: f32,
        em_pixels: [f32; 2],
        tint: [f32; 4],
    ) -> GlyphInstance {
        debug_assert_eq!(self.h_bands, self.v_bands, "the wire format carries one band count");
        GlyphInstance {
            glyph_box: [self.box_min[0], self.box_min[1], self.box_max[0], self.box_max[1]],
            tint,
            offset,
            em_pixels,
            scale,
            band_base: self.band_base,
            bands_per_axis: self.h_bands,
            hull_base: self.hull_base,
            inv_scale: if scale == 0.0 { 0.0 } else { 1.0 / scale },
            _pad: [0.0; 3],
        }
    }

    pub fn instance_affine(
        &self,
        offset: [f32; 2],
        scale: f32,
        transform: [f32; 4],
        tint: [f32; 4],
    ) -> GlyphInstance {
        let [a, b, c, d] = transform;
        let len = |x: f32, y: f32| (x * x + y * y).sqrt();
        self.instance(offset, scale, [len(a, b), len(c, d)], tint)
    }
}

pub(crate) fn draw_uniform(projection: &[f32; 16], default: &[f32; 16], height: f32) -> [f32; 20] {
    let mut out = [0.0f32; 20];
    out[..16].copy_from_slice(projection);
    out[16] = height;
    out[17] = if projection == default { 1.0 } else { 0.0 };
    out
}

#[derive(Clone, Default, Debug)]
pub struct GpuBatch {
    curves: Vec<CurvePoint>,
    band_curves: Vec<u32>,
    bands: Vec<Band>,
    hulls: Vec<HullVertex>,
    slots: alloc::collections::BTreeMap<GpuGlyphKey, GlyphSlot>,
    revision: u64,
}

#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuGlyphError {
    NoOutline,
    TooComplex,
    NonFinite,
    BatchFull,
    NotFlatColor,
}

impl core::fmt::Display for GpuGlyphError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            GpuGlyphError::NoOutline => write!(f, "glyph has no outline"),
            GpuGlyphError::TooComplex => write!(f, "glyph exceeds MAX_CURVES_PER_GLYPH"),
            GpuGlyphError::NonFinite => write!(f, "glyph has a non-finite coordinate"),
            GpuGlyphError::BatchFull => write!(f, "batch cannot address another glyph"),
            GpuGlyphError::NotFlatColor => {
                write!(f, "glyph's color description is a scene, not a tinted outline")
            }
        }
    }
}

impl core::error::Error for GpuGlyphError {}

pub struct BuiltGlyph {
    pub curves: Vec<Quad>,
    pub banded: Banded,
}

impl BuiltGlyph {
    pub fn cost(&self) -> usize {
        self.curves.capacity() * core::mem::size_of::<Quad>()
            + self.banded.band_curves.capacity() * core::mem::size_of::<u32>()
            + self.banded.bands.capacity() * core::mem::size_of::<(u32, u32)>()
            + core::mem::size_of::<hull::Hull>()
            + 64
    }
}

// `font` keeps two faces apart: a glyph id and an axis position say nothing about which font they
// came from, so a shared batch would hand back the wrong outline.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct GpuGlyphKey {
    pub font: usize,
    pub gid: u16,
    pub axes: crate::daecore::sync::Shared<crate::daecore::cache::AxisKey>,
    pub shape: u32,
}

impl GpuBatch {
    pub fn new() -> GpuBatch {
        GpuBatch::default()
    }

    pub fn curves(&self) -> &[CurvePoint] {
        &self.curves
    }

    pub fn band_curves(&self) -> &[u32] {
        &self.band_curves
    }

    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    pub fn hulls(&self) -> &[HullVertex] {
        &self.hulls
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn clear(&mut self) {
        self.curves.clear();
        self.band_curves.clear();
        self.bands.clear();
        self.hulls.clear();
        self.slots.clear();
        self.revision += 1;
    }

    pub fn slot_for(&self, key: &GpuGlyphKey) -> Option<GlyphSlot> {
        self.slots.get(key).copied()
    }

    pub fn remember(&mut self, key: GpuGlyphKey, slot: GlyphSlot) {
        self.slots.insert(key, slot);
    }

    pub fn append(&mut self, curves: &mut [Quad]) -> Option<GlyphSlot> {
        let banded = Self::build_glyph(curves)?;
        self.append_prebuilt(curves, &banded)
    }

    pub fn build_glyph(curves: &mut [Quad]) -> Option<Banded> {
        if curves.len() > MAX_CURVES_PER_GLYPH {
            return None;
        }

        extract::normalize_winding(curves);
        band::build(curves)
    }

    pub fn append_prebuilt(&mut self, curves: &[Quad], banded: &Banded) -> Option<GlyphSlot> {
        let curve_base = u32::try_from(self.curves.len() / 3).ok()?;
        let list_base = u32::try_from(self.band_curves.len()).ok()?;
        let band_base = u32::try_from(self.bands.len()).ok()?;
        let hull_base = u32::try_from(self.hulls.len()).ok()?;

        curve_base.checked_add(u32::try_from(curves.len()).ok()?)?;
        list_base.checked_add(u32::try_from(banded.band_curves.len()).ok()?)?;
        band_base.checked_add(u32::try_from(banded.bands.len()).ok()?)?;
        hull_base.checked_add(u32::try_from(HULL_VERTICES).ok()?)?;

        self.curves.reserve(curves.len() * 3);
        for c in curves {
            for p in c {
                self.curves.push(CurvePoint { x: p[0], y: p[1] });
            }
        }
        self.band_curves.extend(banded.band_curves.iter().map(|i| curve_base + i));
        self.bands.extend(banded.bands.iter().map(|&(first, count)| Band {
            first_curve: list_base + first,
            curve_count: count,
        }));
        self.hulls.extend_from_slice(&banded.hull.verts);
        self.revision += 1;

        Some(GlyphSlot {
            band_base,
            h_bands: banded.bands_per_axis,
            v_bands: banded.bands_per_axis,
            hull_base,
            box_min: banded.box_min,
            box_max: banded.box_max,
        })
    }
}

pub fn collector(units_per_em: f32) -> extract::Collector {
    extract::Collector::new(units_per_em)
}
