mod constants;
mod glyph_info;
mod variants;

use super::decoder::read_i16_be;

pub use constants::parse_math_constants;
pub use glyph_info::{
    math_is_extended_shape, math_italics_correction, math_kern, math_top_accent_attachment,
    MathKernCorner,
};
pub use variants::{math_glyph_construction, math_min_connector_overlap};

pub(super) fn read_math_value(buf: &[u8], off: usize) -> Option<i16> {
    read_i16_be(buf, off)
}

pub(super) fn coverage_index(buf: &[u8], off: usize, gid: u16) -> Option<usize> {
    super::format::coverage::coverage_index(buf.get(off..)?, gid).map(usize::from)
}
