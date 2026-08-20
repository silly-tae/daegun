use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::string::String;
use alloc::vec::Vec;
pub mod schema;
mod value;
mod parse;
mod build;
pub mod schemas;

pub use schema::{Schema, StructField, CountSource, DropPolicy, RebuildPolicy, OffsetWidth, PayloadShape, EmptyPolicy, EnvRef};
pub(crate) use value::Value;
pub(crate) use build::generic_build;

use parse::{generic_parse, Env};

pub(crate) fn parse_subtable(
    buf: &[u8],
    off: usize,
    schema: &Schema,
    active: &GlyphSet,
    gid_map: &[u16],
) -> Result<Option<Value>, String> {
    let mut env = Env::new();
    let (value, _consumed) = generic_parse(buf, off, off, schema, &mut env, active, gid_map)?;
    Ok(value)
}

pub(crate) fn subset_subtable(
    buf: &[u8],
    off: usize,
    schema: &Schema,
    active: &GlyphSet,
    gid_map: &[u16],
) -> Option<Vec<u8>> {
    parse_subtable(buf, off, schema, active, gid_map).ok().flatten().and_then(|v| generic_build(&v))
}

pub fn subset_subtable_stripping_devices(
    buf: &[u8],
    off: usize,
    schema: &Schema,
    active: &GlyphSet,
    gid_map: &[u16],
) -> Option<Vec<u8>> {
    let mut env = Env::stripping_devices();
    let (value, _consumed) = generic_parse(buf, off, off, schema, &mut env, active, gid_map).ok()?;
    value.and_then(|v| generic_build(&v))
}
