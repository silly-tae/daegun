use alloc::string::String;
use alloc::vec::Vec;
use super::super::decoder::read_u16_be;

pub(crate) fn decode_cff_number(data: &[u8], off: usize) -> Result<(i32, usize), String> {
    if off >= data.len() {
        return Err("CFF DICT: truncated number".into());
    }
    let b0 = data[off] as i32;
    match b0 {
        28 => {
            if off + 3 > data.len() { return Err("CFF DICT: short 3-byte int".into()); }
            let v = ((data[off + 1] as i32) << 8) | data[off + 2] as i32;
            Ok((if v & 0x8000 != 0 { v | !0xFFFF } else { v }, 3))
        }
        29 => {
            if off + 5 > data.len() { return Err("CFF DICT: short 5-byte int".into()); }
            let v = ((data[off + 1] as i32) << 24)
                  | ((data[off + 2] as i32) << 16)
                  | ((data[off + 3] as i32) << 8)
                  |   data[off + 4] as i32;
            Ok((v, 5))
        }
        30 => {
            let mut p = off + 1;
            loop {
                if p >= data.len() { return Err("CFF DICT: unterminated real".into()); }
                if p - off > 64 { return Err("CFF DICT: real number too long".into()); }
                let b = data[p]; p += 1;
                if (b & 0x0F) == 0x0F || (b >> 4) == 0x0F { break; }
            }
            Ok((0, p - off))
        }
        32..=246  => Ok((b0 - 139, 1)),
        247..=250 => {
            if off + 2 > data.len() { return Err("CFF DICT: short 2-byte int".into()); }
            Ok(((b0 - 247) * 256 + data[off + 1] as i32 + 108, 2))
        }
        251..=254 => {
            if off + 2 > data.len() { return Err("CFF DICT: short 2-byte int".into()); }
            Ok((-(b0 - 251) * 256 - data[off + 1] as i32 - 108, 2))
        }
        _ => Err(format!("CFF DICT: unexpected byte 0x{:02X} at offset {}", b0, off)),
    }
}

pub(crate) fn decode_charstring_number(data: &[u8], off: usize) -> Result<(f64, usize), String> {
    match decode_charstring_number_opt(data, off) {
        Some(v) => Ok(v),
        None => Err(charstring_number_error(data, off)),
    }
}

#[inline]
pub(crate) fn decode_charstring_number_opt(data: &[u8], off: usize) -> Option<(f64, usize)> {
    let b0 = *data.get(off)? as i32;
    match b0 {
        28 => {
            let b1 = *data.get(off + 1)? as i32;
            let b2 = *data.get(off + 2)? as i32;
            let v = ((b1 << 8) | b2) as i16;
            Some((v as f64, 3))
        }
        32..=246 => Some(((b0 - 139) as f64, 1)),
        247..=250 => {
            let b1 = *data.get(off + 1)? as i32;
            Some((((b0 - 247) * 256 + b1 + 108) as f64, 2))
        }
        251..=254 => {
            let b1 = *data.get(off + 1)? as i32;
            Some(((-(b0 - 251) * 256 - b1 - 108) as f64, 2))
        }
        255 => {
            let b = data.get(off + 1..off + 5)?;
            let raw = i32::from_be_bytes([b[0], b[1], b[2], b[3]]);
            // A charstring number is a different encoding from a DICT number despite sharing the
            // one- and two-byte forms: 255 is a 16.16 fixed-point value here, and a byte a DICT
            // never uses at all.
            Some((raw as f64 / 65536.0, 5))
        }
        _ => None,
    }
}

#[inline]
pub(crate) fn decode_charstring_number_fx(data: &[u8], off: usize) -> Option<(i64, usize)> {
    let b0 = *data.get(off)? as i32;
    match b0 {
        28 => {
            let b1 = *data.get(off + 1)? as i32;
            let b2 = *data.get(off + 2)? as i32;
            let v = ((b1 << 8) | b2) as i16;
            Some(((v as i64) << 16, 3))
        }
        32..=246 => Some((((b0 - 139) as i64) << 16, 1)),
        247..=250 => {
            let b1 = *data.get(off + 1)? as i32;
            Some(((((b0 - 247) * 256 + b1 + 108) as i64) << 16, 2))
        }
        251..=254 => {
            let b1 = *data.get(off + 1)? as i32;
            Some((((-(b0 - 251) * 256 - b1 - 108) as i64) << 16, 2))
        }
        255 => {
            let b = data.get(off + 1..off + 5)?;
            Some((i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64, 5))
        }
        _ => None,
    }
}

#[cold]
#[inline(never)]
pub(crate) fn charstring_number_error(data: &[u8], off: usize) -> String {
    let Some(&b0) = data.get(off) else { return "CFF charstring: truncated number".into() };
    match b0 {
        28 => "CFF charstring: truncated 3-byte int".into(),
        247..=254 => "CFF charstring: truncated 2-byte int".into(),
        255 => "CFF charstring: truncated Fixed16.16".into(),
        _ => format!("CFF charstring: byte 0x{b0:02X} is not a valid number encoding"),
    }
}

pub(crate) fn subr_bias(count: usize) -> i32 {
    if count < 1240 { 107 } else if count < 33900 { 1131 } else { 32768 }
}

pub(crate) fn resolve_fd_select(cff: &[u8], off: usize, n_glyphs: usize) -> Result<Vec<u16>, String> {
    let format = *cff.get(off).ok_or("CFF FDSelect: offset out of bounds")?;
    match format {
        0 => {
            let mut out = Vec::with_capacity(n_glyphs);
            for gid in 0..n_glyphs {
                let fd = *cff.get(off + 1 + gid).ok_or("CFF FDSelect format 0: truncated")?;
                out.push(fd as u16);
            }
            Ok(out)
        }
        3 => {
            let n_ranges = read_u16_be(cff, off + 1).ok_or("CFF FDSelect format 3: truncated")? as usize;
            let mut out = vec![0u16; n_glyphs];
            let ranges_off = off + 3;
            let mut prev_end = 0usize;
            for i in 0..n_ranges {
                let rec   = ranges_off + i * 3;
                let first = read_u16_be(cff, rec).ok_or("CFF FDSelect format 3: truncated")? as usize;
                let fd    = *cff.get(rec + 2).ok_or("CFF FDSelect format 3: truncated")?;
                let next_first = read_u16_be(cff, rec + 3).ok_or("CFF FDSelect format 3: truncated")? as usize;
                if first < prev_end || next_first <= first {
                    continue;
                }
                let end = next_first.min(n_glyphs);
                prev_end = end;
                for slot in out.iter_mut().take(end).skip(first) {
                    *slot = fd as u16;
                }
            }
            Ok(out)
        }
        _ => Err(format!("CFF FDSelect: unsupported format {} (format 4 deferred)", format)),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DictOp {
    Single(u8),
    Escaped(u8),
}

pub(crate) enum DictFlow {
    Continue,
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DictKind {
    Cff1,
    Cff2,
}

impl DictKind {
    fn is_operator(self, b: u8) -> bool {
        match self {
            DictKind::Cff1 => b <= 21,
            DictKind::Cff2 => b <= 21 || b == 22 || b == 24,
        }
    }
}

pub(crate) fn walk_cff_dict<F>(dict: &[u8], kind: DictKind, mut on_op: F)
where
    F: FnMut(DictOp, &[i32], usize, usize) -> DictFlow,
{
    let mut operands = Vec::<i32>::new();
    let mut off = 0usize;
    let mut operand_start = 0usize;
    let mut steps = 0usize;

    while off < dict.len() {
        steps += 1;
        if steps > dict.len() { break; }
        let b = dict[off];

        if kind.is_operator(b) {
            let (op, width) = if b == 12 {
                match dict.get(off + 1) {
                    Some(&b1) => (DictOp::Escaped(b1), 2),
                    None => break,
                }
            } else {
                (DictOp::Single(b), 1)
            };
            let flow = on_op(op, &operands, operand_start, off);
            operands.clear();
            off += width;
            operand_start = off;
            if matches!(flow, DictFlow::Stop) { return; }
        } else {
            match decode_cff_number(dict, off) {
                Ok((v, sz)) => { operands.push(v); off += sz; }
                Err(_) => break,
            }
        }
    }
}
