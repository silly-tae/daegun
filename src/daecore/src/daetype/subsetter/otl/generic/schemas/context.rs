use alloc::boxed::Box;
use super::super::schema::{Schema, StructField, CountSource, DropPolicy, RebuildPolicy, OffsetWidth, PayloadShape, EmptyPolicy, EnvRef};

fn seq_lookup_record_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "sequence_index", schema: Schema::U16, bind: None },
        StructField { name: "lookup_list_index", schema: Schema::U16, bind: None },
    ])
}

fn rule_set_schema(rule_schema: Schema) -> Schema {
    Schema::Struct(vec![
        StructField { name: "rule_count", schema: Schema::U16, bind: Some("rule_count") },
        StructField { name: "rules", schema: Schema::OffsetArray(Box::new(rule_schema), CountSource::Field(EnvRef("rule_count")), RebuildPolicy::CompactSurvivors, OffsetWidth::W16), bind: None },
    ])
}

fn context_rule_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "glyph_count", schema: Schema::U16, bind: Some("glyph_count") },
        StructField { name: "lr_count", schema: Schema::U16, bind: Some("lr_count") },
        StructField { name: "input", schema: Schema::Array(Box::new(Schema::GlyphId), CountSource::FieldMinusOne(EnvRef("glyph_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "lookup_records", schema: Schema::Array(Box::new(seq_lookup_record_schema()), CountSource::Field(EnvRef("lr_count")), DropPolicy::FilterSurvivors), bind: None },
    ])
}

fn context_format1_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "format", schema: Schema::U16, bind: None },
        StructField { name: "entries", schema: Schema::CoveredArray(vec![], Box::new(rule_set_schema(context_rule_schema())), PayloadShape::Offsets(OffsetWidth::W16)), bind: None },
    ])
}

fn chain_context_rule_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "bt_count", schema: Schema::U16, bind: Some("bt_count") },
        StructField { name: "backtrack", schema: Schema::Array(Box::new(Schema::GlyphId), CountSource::Field(EnvRef("bt_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "in_count", schema: Schema::U16, bind: Some("in_count") },
        StructField { name: "input", schema: Schema::Array(Box::new(Schema::GlyphId), CountSource::FieldMinusOne(EnvRef("in_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "la_count", schema: Schema::U16, bind: Some("la_count") },
        StructField { name: "lookahead", schema: Schema::Array(Box::new(Schema::GlyphId), CountSource::Field(EnvRef("la_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "lr_count", schema: Schema::U16, bind: Some("lr_count") },
        StructField { name: "lookup_records", schema: Schema::Array(Box::new(seq_lookup_record_schema()), CountSource::Field(EnvRef("lr_count")), DropPolicy::FilterSurvivors), bind: None },
    ])
}

fn chain_context_format1_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "format", schema: Schema::U16, bind: None },
        StructField { name: "entries", schema: Schema::CoveredArray(vec![], Box::new(rule_set_schema(chain_context_rule_schema())), PayloadShape::Offsets(OffsetWidth::W16)), bind: None },
    ])
}

fn context_class_rule_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "glyph_count", schema: Schema::U16, bind: Some("glyph_count") },
        StructField { name: "lr_count", schema: Schema::U16, bind: Some("lr_count") },
        StructField { name: "classes", schema: Schema::Array(Box::new(Schema::U16), CountSource::FieldMinusOne(EnvRef("glyph_count")), DropPolicy::FilterSurvivors), bind: None },
        StructField { name: "lookup_records", schema: Schema::Array(Box::new(seq_lookup_record_schema()), CountSource::Field(EnvRef("lr_count")), DropPolicy::FilterSurvivors), bind: None },
    ])
}

fn context_format2_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "format", schema: Schema::U16, bind: None },
        StructField { name: "coverage", schema: Schema::Offset(OffsetWidth::W16, Box::new(Schema::Coverage(EmptyPolicy::Fail, None))), bind: None },
        StructField { name: "class_def", schema: Schema::Offset(OffsetWidth::W16, Box::new(Schema::ClassDef(EmptyPolicy::Keep))), bind: None },
        StructField { name: "class_set_count", schema: Schema::U16, bind: Some("class_set_count") },
        StructField { name: "class_sets", schema: Schema::OffsetArray(Box::new(rule_set_schema(context_class_rule_schema())), CountSource::Field(EnvRef("class_set_count")), RebuildPolicy::PreserveSlotPositions, OffsetWidth::W16), bind: None },
    ])
}

fn chain_class_rule_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "bt_count", schema: Schema::U16, bind: Some("bt_count") },
        StructField { name: "backtrack", schema: Schema::Array(Box::new(Schema::U16), CountSource::Field(EnvRef("bt_count")), DropPolicy::FilterSurvivors), bind: None },
        StructField { name: "in_count", schema: Schema::U16, bind: Some("in_count") },
        StructField { name: "input", schema: Schema::Array(Box::new(Schema::U16), CountSource::FieldMinusOne(EnvRef("in_count")), DropPolicy::FilterSurvivors), bind: None },
        StructField { name: "la_count", schema: Schema::U16, bind: Some("la_count") },
        StructField { name: "lookahead", schema: Schema::Array(Box::new(Schema::U16), CountSource::Field(EnvRef("la_count")), DropPolicy::FilterSurvivors), bind: None },
        StructField { name: "lr_count", schema: Schema::U16, bind: Some("lr_count") },
        StructField { name: "lookup_records", schema: Schema::Array(Box::new(seq_lookup_record_schema()), CountSource::Field(EnvRef("lr_count")), DropPolicy::FilterSurvivors), bind: None },
    ])
}

fn chain_context_format2_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "format", schema: Schema::U16, bind: None },
        StructField { name: "coverage", schema: Schema::Offset(OffsetWidth::W16, Box::new(Schema::Coverage(EmptyPolicy::Fail, None))), bind: None },
        StructField { name: "backtrack_class_def", schema: Schema::Offset(OffsetWidth::W16, Box::new(Schema::ClassDef(EmptyPolicy::Keep))), bind: None },
        StructField { name: "input_class_def", schema: Schema::Offset(OffsetWidth::W16, Box::new(Schema::ClassDef(EmptyPolicy::Keep))), bind: None },
        StructField { name: "lookahead_class_def", schema: Schema::Offset(OffsetWidth::W16, Box::new(Schema::ClassDef(EmptyPolicy::Keep))), bind: None },
        StructField { name: "class_set_count", schema: Schema::U16, bind: Some("class_set_count") },
        StructField { name: "class_sets", schema: Schema::OffsetArray(Box::new(rule_set_schema(chain_class_rule_schema())), CountSource::Field(EnvRef("class_set_count")), RebuildPolicy::PreserveSlotPositions, OffsetWidth::W16), bind: None },
    ])
}

fn context_format3_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "format", schema: Schema::U16, bind: None },
        StructField { name: "glyph_count", schema: Schema::U16, bind: Some("glyph_count") },
        StructField { name: "lr_count", schema: Schema::U16, bind: Some("lr_count") },
        StructField { name: "positions", schema: Schema::Array(Box::new(Schema::Offset(OffsetWidth::W16, Box::new(Schema::Coverage(EmptyPolicy::Fail, None)))), CountSource::Field(EnvRef("glyph_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "lookup_records", schema: Schema::Array(Box::new(seq_lookup_record_schema()), CountSource::Field(EnvRef("lr_count")), DropPolicy::FilterSurvivors), bind: None },
    ])
}

fn chain_context_format3_schema() -> Schema {
    Schema::Struct(vec![
        StructField { name: "format", schema: Schema::U16, bind: None },
        StructField { name: "bt_count", schema: Schema::U16, bind: Some("bt_count") },
        StructField { name: "backtrack", schema: Schema::Array(Box::new(Schema::Offset(OffsetWidth::W16, Box::new(Schema::Coverage(EmptyPolicy::Fail, None)))), CountSource::Field(EnvRef("bt_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "in_count", schema: Schema::U16, bind: Some("in_count") },
        StructField { name: "input", schema: Schema::Array(Box::new(Schema::Offset(OffsetWidth::W16, Box::new(Schema::Coverage(EmptyPolicy::Fail, None)))), CountSource::Field(EnvRef("in_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "la_count", schema: Schema::U16, bind: Some("la_count") },
        StructField { name: "lookahead", schema: Schema::Array(Box::new(Schema::Offset(OffsetWidth::W16, Box::new(Schema::Coverage(EmptyPolicy::Fail, None)))), CountSource::Field(EnvRef("la_count")), DropPolicy::AllOrNothing), bind: None },
        StructField { name: "lr_count", schema: Schema::U16, bind: Some("lr_count") },
        StructField { name: "lookup_records", schema: Schema::Array(Box::new(seq_lookup_record_schema()), CountSource::Field(EnvRef("lr_count")), DropPolicy::FilterSurvivors), bind: None },
    ])
}

pub(crate) fn context_schema() -> Schema {
    Schema::FormatSwitch(0, vec![
        (1, context_format1_schema()),
        (2, context_format2_schema()),
        (3, context_format3_schema()),
    ])
}

pub(crate) fn chain_context_schema() -> Schema {
    Schema::FormatSwitch(0, vec![
        (1, chain_context_format1_schema()),
        (2, chain_context_format2_schema()),
        (3, chain_context_format3_schema()),
    ])
}
