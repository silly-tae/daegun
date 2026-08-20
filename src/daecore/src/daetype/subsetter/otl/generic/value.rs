use alloc::boxed::Box;
use alloc::vec::Vec;
use super::super::gdef;
use super::schema::{OffsetWidth, PayloadShape};

#[allow(clippy::enum_variant_names)]
pub(crate) enum Value {
    U16(u16),
    I16(i16),
    Glyph(u16),
    Offset(OffsetWidth, Option<Box<Value>>),
    Array(Vec<Value>),
    OffsetArray(OffsetWidth, Vec<Option<Value>>),
    Struct(Vec<(&'static str, Value)>),
    ValueRecord(u16, [i16; 4], Vec<Option<Vec<u8>>>),
    Coverage(Vec<u16>),
    ClassDef(Vec<(u16, u16)>),
    CoveredArray(Vec<(&'static str, Value)>, PayloadShape, Vec<(u16, Value)>),
    Anchor(i16, i16, Option<u16>, Option<Vec<u8>>, Option<Vec<u8>>),
    ZippedWithBoundCoverage(PayloadShape, Vec<Value>),
    CaretValue(gdef::CaretValue),
    ClassMatrix {
        class_def1: Vec<(u16, u16)>,
        class_def2: Vec<(u16, u16)>,
        class1_count: u16,
        class2_count: u16,
        grid: Vec<Value>,
    },
}

pub(crate) fn value_field_count(bitmask: u16) -> usize {
    (bitmask & 0x000F).count_ones() as usize
}
