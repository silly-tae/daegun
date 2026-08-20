// SAFETY, once for the file. Every entry point null-checks its pointers and answers
// `Status::Null` before dereferencing anything, so the `unsafe` blocks below rest on a check
// immediately above them. Where a call takes a raw buffer, its length is the caller's promise
// from `daegun.h` and is not checkable here.

use crate::{Cap, HintMode, Join, StripeOrder, StrokeStyle, SubpixelLayout};

use crate::ffi::handle::Status;

pub const LAYOUT_GRAYSCALE: i32 = 0;
pub const LAYOUT_RGB_H: i32 = 1;
pub const LAYOUT_BGR_H: i32 = 2;
pub const LAYOUT_RGB_V: i32 = 3;
pub const LAYOUT_BGR_V: i32 = 4;
pub const LAYOUT_RGB_H_UNFILTERED: i32 = 5;
pub const LAYOUT_BGR_H_UNFILTERED: i32 = 6;
pub const LAYOUT_RGB_V_UNFILTERED: i32 = 7;
pub const LAYOUT_BGR_V_UNFILTERED: i32 = 8;

pub(crate) fn layout_of(code: i32) -> SubpixelLayout {
    match code {
        LAYOUT_RGB_H => SubpixelLayout::horizontal(StripeOrder::Rgb),
        LAYOUT_BGR_H => SubpixelLayout::horizontal(StripeOrder::Bgr),
        LAYOUT_RGB_V => SubpixelLayout::vertical(StripeOrder::Rgb),
        LAYOUT_BGR_V => SubpixelLayout::vertical(StripeOrder::Bgr),
        LAYOUT_RGB_H_UNFILTERED => SubpixelLayout::unfiltered(StripeOrder::Rgb, true),
        LAYOUT_BGR_H_UNFILTERED => SubpixelLayout::unfiltered(StripeOrder::Bgr, true),
        LAYOUT_RGB_V_UNFILTERED => SubpixelLayout::unfiltered(StripeOrder::Rgb, false),
        LAYOUT_BGR_V_UNFILTERED => SubpixelLayout::unfiltered(StripeOrder::Bgr, false),
        _ => SubpixelLayout::grayscale(),
    }
}

pub const HINT_NONE: i32 = 0;
pub const HINT_SUBPIXEL: i32 = 1;
pub const HINT_CLASSIC: i32 = 2;
pub const HINT_AUTO: i32 = 3;
pub const HINT_AUTO_FORCE: i32 = 4;

fn hint_of(code: i32) -> HintMode {
    match code {
        HINT_SUBPIXEL => HintMode::Subpixel,
        HINT_CLASSIC => HintMode::Classic,
        HINT_AUTO => HintMode::Auto,
        HINT_AUTO_FORCE => HintMode::AutoForce,
        _ => HintMode::None,
    }
}

pub const JOIN_MITER: i32 = 0;
pub const JOIN_ROUND: i32 = 1;
pub const JOIN_BEVEL: i32 = 2;

pub const CAP_BUTT: i32 = 0;
pub const CAP_ROUND: i32 = 1;
pub const CAP_SQUARE: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RasterOptionsC {
    pub layout: i32,
    pub hinting: i32,
    pub has_gamma: i32,
    pub gamma: f32,
    pub has_transform: i32,
    pub transform: [f32; 6],
    pub has_stroke: i32,
    pub stroke_width: f32,
    pub stroke_join: i32,
    pub stroke_miter_limit: f32,
    pub stroke_cap: i32,
    pub has_embolden: i32,
    pub embolden: f32,
    pub has_oblique: i32,
    pub oblique: f32,
}

const _: () = assert!(size_of::<RasterOptionsC>() == 80);
const _: () = assert!(align_of::<RasterOptionsC>() == 4);

impl RasterOptionsC {
    pub const DEFAULT: RasterOptionsC = RasterOptionsC {
        layout: LAYOUT_GRAYSCALE,
        hinting: HINT_NONE,
        has_gamma: 0,
        gamma: 0.0,
        has_transform: 0,
        transform: [0.0; 6],
        has_stroke: 0,
        stroke_width: 0.0,
        stroke_join: JOIN_MITER,
        stroke_miter_limit: 0.0,
        stroke_cap: CAP_BUTT,
        has_embolden: 0,
        embolden: 0.0,
        has_oblique: 0,
        oblique: 0.0,
    };

    pub fn to_rust(self) -> crate::RasterOptions {
        let mut out = crate::RasterOptions::default()
            .with_layout(layout_of(self.layout))
            .with_hinting(hint_of(self.hinting));
        if self.has_gamma != 0 {
            out = out.with_gamma(self.gamma);
        }
        if self.has_transform != 0 {
            out = out.with_transform(self.transform);
        }
        if self.has_stroke != 0 {
            out = out.with_stroke(StrokeStyle {
                width: self.stroke_width,
                join: match self.stroke_join {
                    JOIN_ROUND => Join::Round,
                    JOIN_BEVEL => Join::Bevel,
                    _ => Join::Miter { limit: self.stroke_miter_limit },
                },
                cap: match self.stroke_cap {
                    CAP_ROUND => Cap::Round,
                    CAP_SQUARE => Cap::Square,
                    _ => Cap::Butt,
                },
            });
        }
        if self.has_embolden != 0 {
            out = out.with_embolden(self.embolden);
        }
        if self.has_oblique != 0 {
            out = out.with_oblique(self.oblique);
        }
        out
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn daegun_hint_mode_may_autohint(mode: i32, out: *mut i32) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    let mode = match mode {
        HINT_SUBPIXEL => HintMode::Subpixel,
        HINT_CLASSIC => HintMode::Classic,
        HINT_AUTO => HintMode::Auto,
        HINT_AUTO_FORCE => HintMode::AutoForce,
        _ => HintMode::None,
    };
    unsafe { *out = i32::from(mode.may_autohint()) };
    Status::Ok
}
