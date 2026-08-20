use alloc::boxed::Box;
use alloc::vec::Vec;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OffsetWidth {
    W16,
    W32,
}

impl OffsetWidth {
    pub(crate) fn bytes(self) -> usize {
        match self {
            OffsetWidth::W16 => 2,
            OffsetWidth::W32 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code, reason = "Fixed is matched by parse.rs but only constructed by generic_engine_tests")]
pub enum CountSource {
    Field(EnvRef),
    FieldMinusOne(EnvRef),
    Fixed(usize),
}

#[derive(Clone, Copy, Debug)]
pub enum DropPolicy {
    FilterSurvivors,
    AllOrNothing,
    // Like FilterSurvivors, but an empty result is None rather than a valid empty array – a choice
    // menu with no surviving choices is meaningless, not merely smaller.
    FilterSurvivorsOrFail,
}

#[derive(Clone, Copy, Debug)]
pub enum PayloadShape {
    Inline,
    Offsets(OffsetWidth),
}

#[derive(Clone, Copy, Debug)]
pub enum RebuildPolicy {
    CompactSurvivors,
    PreserveSlotPositions,
}

#[derive(Clone, Copy, Debug)]
pub struct EnvRef(pub(crate) &'static str);

pub struct StructField {
    pub(crate) name: &'static str,
    pub(crate) schema: Schema,
    pub(crate) bind: Option<&'static str>,
}

#[allow(dead_code, reason = "I16 is matched by parse.rs but only constructed by generic_engine_tests")]
pub enum Schema {
    U16,
    I16,
    GlyphId,
    Offset(OffsetWidth, Box<Schema>),
    Array(Box<Schema>, CountSource, DropPolicy),
    OffsetArray(Box<Schema>, CountSource, RebuildPolicy, OffsetWidth),
    Struct(Vec<StructField>),
    FormatSwitch(usize, Vec<(u16, Schema)>),
    ValueRecordField(EnvRef),
    Coverage(EmptyPolicy, Option<EnvRef>),
    ClassDef(EmptyPolicy),
    CoveredArray(Vec<StructField>, Box<Schema>, PayloadShape),
    ValueFormatField(EnvRef),
    Anchor,
    ZippedWithBoundCoverage(EnvRef, Box<Schema>, PayloadShape, DropPolicy),
    CaretValue,
    ClassMatrix(Box<Schema>),
    DeltaCoverageSubst,
}

#[derive(Clone, Copy, Debug)]
pub enum EmptyPolicy {
    Fail,
    Keep,
}
