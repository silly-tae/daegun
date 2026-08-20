use alloc::boxed::Box;
use super::super::schema::{Schema, StructField, CountSource, DropPolicy, RebuildPolicy, OffsetWidth, PayloadShape, EmptyPolicy, EnvRef};

pub(crate) fn glyph_class_def_schema() -> Schema { Schema::ClassDef(EmptyPolicy::Fail) }
pub(crate) fn mark_attach_class_def_schema() -> Schema { Schema::ClassDef(EmptyPolicy::Fail) }

fn attach_point_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "point_count", schema: Schema::U16, bind: Some("point_count") },
        StructField { name: "points", schema: Schema::Array(Box::new(Schema::U16), CountSource::Field(EnvRef("point_count")), DropPolicy::FilterSurvivors), bind: None },
    ])
}

pub(crate) fn attach_list_schema() -> Schema {
    Schema::CoveredArray(vec![], Box::new(attach_point_schema()), PayloadShape::Offsets(OffsetWidth::W16))
}

fn lig_glyph_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "caret_count", schema: Schema::U16, bind: Some("caret_count") },
        StructField { name: "carets", schema: Schema::OffsetArray(Box::new(Schema::CaretValue), CountSource::Field(EnvRef("caret_count")), RebuildPolicy::CompactSurvivors, OffsetWidth::W16), bind: None },
    ])
}

pub(crate) fn lig_caret_list_schema() -> Schema {
    Schema::CoveredArray(vec![], Box::new(lig_glyph_schema()), PayloadShape::Offsets(OffsetWidth::W16))
}

pub(crate) fn mark_glyph_sets_schema() -> Schema {
    Schema::FormatSwitch(0, vec![
        (1, Schema::Struct(vec![
            StructField { name: "format", schema: Schema::U16, bind: None },
            StructField { name: "mark_set_count", schema: Schema::U16, bind: Some("mark_set_count") },
            StructField {
                name: "sets",
                schema: Schema::Array(
                    Box::new(Schema::Offset(OffsetWidth::W32, Box::new(Schema::Coverage(EmptyPolicy::Keep, None)))),
                    CountSource::Field(EnvRef("mark_set_count")),
                    DropPolicy::FilterSurvivors,
                ),
                bind: None,
            },
        ])),
    ])
}
