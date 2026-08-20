use core::mem::offset_of;

use crate::{Band, ColorSlot, CurvePoint, GlyphInstance, GlyphSlot, HullVertex, SubpixelParams};

// Offsets, not only sizes: GlyphSlot is 32 bytes however its six fields are arranged, so a
// reordering passes size_of and hands a C caller the wrong field. These types are defined in
// daecore and daerizer, which can reorder without anyone editing daegun.h. Mirrored by offsetof.
const _: () = assert!(size_of::<GlyphSlot>() == 32);
const _: () = assert!(offset_of!(GlyphSlot, band_base) == 0);
const _: () = assert!(offset_of!(GlyphSlot, h_bands) == 4);
const _: () = assert!(offset_of!(GlyphSlot, v_bands) == 8);
const _: () = assert!(offset_of!(GlyphSlot, hull_base) == 12);
const _: () = assert!(offset_of!(GlyphSlot, box_min) == 16);
const _: () = assert!(offset_of!(GlyphSlot, box_max) == 24);

const _: () = assert!(size_of::<GlyphInstance>() == 80);
const _: () = assert!(offset_of!(GlyphInstance, glyph_box) == 0);
const _: () = assert!(offset_of!(GlyphInstance, tint) == 16);
const _: () = assert!(offset_of!(GlyphInstance, offset) == 32);
const _: () = assert!(offset_of!(GlyphInstance, em_pixels) == 40);
const _: () = assert!(offset_of!(GlyphInstance, scale) == 48);
const _: () = assert!(offset_of!(GlyphInstance, band_base) == 52);
const _: () = assert!(offset_of!(GlyphInstance, bands_per_axis) == 56);
const _: () = assert!(offset_of!(GlyphInstance, hull_base) == 60);
const _: () = assert!(offset_of!(GlyphInstance, inv_scale) == 64);

const _: () = assert!(size_of::<SubpixelParams>() == 800);
const _: () = assert!(offset_of!(SubpixelParams, weights) == 0);
const _: () = assert!(offset_of!(SubpixelParams, oversample) == 768);
const _: () = assert!(offset_of!(SubpixelParams, taps) == 776);
const _: () = assert!(offset_of!(SubpixelParams, origin) == 784);
const _: () = assert!(offset_of!(SubpixelParams, channels) == 792);
const _: () = assert!(offset_of!(SubpixelParams, supersample) == 796);

const _: () = assert!(size_of::<Band>() == 8);
const _: () = assert!(offset_of!(Band, first_curve) == 0);
const _: () = assert!(offset_of!(Band, curve_count) == 4);

const _: () = assert!(size_of::<CurvePoint>() == 8);
const _: () = assert!(offset_of!(CurvePoint, x) == 0);
const _: () = assert!(offset_of!(CurvePoint, y) == 4);

const _: () = assert!(size_of::<HullVertex>() == 24);
const _: () = assert!(offset_of!(HullVertex, pos) == 0);
const _: () = assert!(offset_of!(HullVertex, dilate) == 8);

const _: () = assert!(size_of::<ColorSlot>() == 48);
const _: () = assert!(offset_of!(ColorSlot, slot) == 0);
const _: () = assert!(offset_of!(ColorSlot, tint) == 32);
