use alloc::vec::Vec;
use super::super::decoder::{read_u16_be, read_u32_be, write_u32_be};

pub(crate) fn strip_gdef_var_store(gdef: &[u8]) -> Option<Vec<u8>> {
    if gdef.len() < 18 { return None; }
    let major = read_u16_be(gdef, 0)?;
    let minor = read_u16_be(gdef, 2)?;
    if major != 1 || minor < 3 || read_u32_be(gdef, 14)? == 0 { return None; }
    let mut out = gdef.to_vec();
    write_u32_be(&mut out, 14, 0);
    Some(out)
}

pub(crate) fn strip_colr_var_store(colr: &[u8]) -> Option<Vec<u8>> {
    if colr.len() < 34 { return None; }
    let version = read_u16_be(colr, 0)?;
    if version != 1 || read_u32_be(colr, 30)? == 0 { return None; }
    let mut out = colr.to_vec();
    write_u32_be(&mut out, 30, 0);
    Some(out)
}
