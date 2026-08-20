use alloc::vec::Vec;
#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;

pub mod blend;
pub mod gradient;
pub mod matrix;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub fn opaque(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }

    pub fn fade(self, by: f64) -> Rgba {
        if !by.is_finite() {
            return self;
        }
        Rgba { a: (f64::from(self.a) * by.clamp(0.0, 1.0)).round() as u8, ..self }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Extend {
    #[default]
    Pad,
    Reflect,
    Repeat,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stop {
    pub offset: f32,
    pub color: Rgba,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GradientKind {
    Linear { x0: f32, y0: f32, x1: f32, y1: f32 },
    Radial { x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32 },
    Sweep { cx: f32, cy: f32, start_angle: f32, end_angle: f32 },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<Stop>,
    pub extend: Extend,
    pub transform: [f64; 6],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Blend {
    Clear,
    Src,
    Dest,
    #[default]
    SrcOver,
    DestOver,
    SrcIn,
    DestIn,
    SrcOut,
    DestOut,
    SrcAtop,
    DestAtop,
    Xor,
    Plus,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Multiply,
    HslHue,
    HslSaturation,
    HslColor,
    HslLuminosity,
}

impl Blend {
    pub fn from_colr(mode: u8) -> Blend {
        use Blend::*;
        const MODES: [Blend; 28] = [
            Clear, Src, Dest, SrcOver, DestOver, SrcIn, DestIn, SrcOut, DestOut, SrcAtop, DestAtop,
            Xor, Plus, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight,
            Difference, Exclusion, Multiply, HslHue, HslSaturation, HslColor, HslLuminosity,
        ];
        MODES.get(mode as usize).copied().unwrap_or(SrcOver)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Stops {
    Nothing,
    Solid(Rgba),
    Many(Vec<Stop>),
}

pub fn resolve_stops(mut stops: Vec<Stop>) -> Stops {
    match stops.len() {
        0 => return Stops::Nothing,
        1 => return Stops::Solid(stops[0].color),
        _ => {}
    }
    for s in &mut stops {
        s.offset = if s.offset.is_nan() { 0.0 } else { s.offset.clamp(0.0, 1.0) };
    }
    stops.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap_or(core::cmp::Ordering::Equal));
    for i in 1..stops.len() {
        if stops[i].offset < stops[i - 1].offset {
            stops[i].offset = stops[i - 1].offset;
        }
    }
    Stops::Many(stops)
}
