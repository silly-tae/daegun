use alloc::string::String;
use alloc::vec::Vec;
use crate::daecore::daetype::decoder::{read_u16_be, read_u32_be};
use crate::daecore::daetype::format::cff::{walk_cff_dict, DictFlow, DictKind, DictOp};

pub fn cff_index_spans(data: &[u8], off: usize, count_is_32bit: bool) -> Result<(Vec<(u32, u32)>, usize), String> {
    if data.len() > u32::MAX as usize {
        return Err("CFF INDEX: table larger than a CFF table can be".into());
    }
    let count_size = if count_is_32bit { 4 } else { 2 };
    if off.checked_add(count_size).is_none_or(|end| end > data.len()) {
        return Err("CFF INDEX: truncated".into());
    }
    let count = if count_is_32bit {
        read_u32_be(data, off).ok_or("CFF INDEX: truncated")? as usize
    } else {
        read_u16_be(data, off).ok_or("CFF INDEX: truncated")? as usize
    };
    if count == 0 {
        return Ok((Vec::new(), off + count_size));
    }
    if off + count_size + 1 > data.len() {
        return Err("CFF INDEX: missing offSize".into());
    }
    let off_size = data[off + count_size] as usize;
    if !(1..=4).contains(&off_size) {
        return Err(format!("CFF INDEX: invalid offSize {}", off_size));
    }
    let offsets_start = off + count_size + 1;

    let offsets_count = count.checked_add(1).ok_or("CFF INDEX: count overflows")?;
    let data_start = offsets_count
        .checked_mul(off_size)
        .and_then(|len| offsets_start.checked_add(len))
        .ok_or("CFF INDEX: offsets array overflows the address space")?;
    if data_start > data.len() {
        return Err("CFF INDEX: offsets array does not fit in the buffer".into());
    }

    let mut offsets = Vec::with_capacity(offsets_count);
    for i in 0..offsets_count {
        let o = offsets_start + i * off_size;
        if o + off_size > data.len() {
            return Err("CFF INDEX: offsets truncated".into());
        }
        let mut v = 0usize;
        for j in 0..off_size {
            v = (v << 8) | data[o + j] as usize;
        }
        offsets.push(v);
    }

    let data_len = offsets[count].saturating_sub(1);
    if data_start.checked_add(data_len).is_none_or(|end| end > data.len()) {
        return Err("CFF INDEX: data truncated".into());
    }

    let mut spans = Vec::with_capacity(count);
    for i in 0..count {
        let start = data_start + offsets[i].saturating_sub(1);
        let end   = data_start + offsets[i + 1].saturating_sub(1);
        if start > end || end > data.len() {
            return Err(format!("CFF INDEX: object {} out of bounds", i));
        }
        spans.push((start as u32, end as u32));
    }

    Ok((spans, data_start + data_len))
}

pub fn parse_cff_index_refs(
    data: &[u8],
    off: usize,
    count_is_32bit: bool,
) -> Result<(Vec<&[u8]>, usize), String> {
    let (spans, end) = cff_index_spans(data, off, count_is_32bit)?;
    let mut objects = Vec::with_capacity(spans.len());
    for (s, e) in spans {
        objects.push(data.get(s as usize..e as usize).ok_or("CFF INDEX: object out of bounds")?);
    }
    Ok((objects, end))
}

pub fn parse_cff_index(
    data: &[u8],
    off: usize,
    count_is_32bit: bool,
) -> Result<(Vec<Vec<u8>>, usize), String> {
    let (refs, end) = parse_cff_index_refs(data, off, count_is_32bit)?;
    Ok((refs.into_iter().map(<[u8]>::to_vec).collect(), end))
}

#[derive(Debug)]
pub struct TopDictFields {
    pub charset_off:     Option<usize>,
    pub charset_predefined: u8,
    pub charstrings_off: usize,
    pub private_size:    usize,
    pub private_off:     usize,
    pub fd_array_off:    Option<usize>,
    pub fd_select_off:   Option<usize>,
    pub ros:             Option<(i32, i32, i32)>,
    pub font_matrix_raw: Option<Vec<u8>>,
}

pub fn parse_top_dict(dict: &[u8]) -> Result<TopDictFields, String> {
    let mut charset_off   = None;
    let mut charset_predefined = 0u8;
    let mut cs_off        = 0usize;
    let mut priv_size     = 0usize;
    let mut priv_off      = 0usize;
    let mut fd_array_off  = None;
    let mut fd_select_off = None;
    let mut ros           = None;
    let mut font_matrix_raw: Option<Vec<u8>> = None;

    walk_cff_dict(dict, DictKind::Cff1, |op, operands, operand_start, op_off| {
        match op {
            DictOp::Escaped(30) if operands.len() >= 3 => {
                let n = operands.len();
                ros = Some((operands[n - 3], operands[n - 2], operands[n - 1]));
            }
            DictOp::Escaped(36) => { if let Some(&v) = operands.last() && v >= 0 { fd_array_off  = Some(v as usize); } }
            DictOp::Escaped(37) => { if let Some(&v) = operands.last() && v >= 0 { fd_select_off = Some(v as usize); } }
            DictOp::Escaped(7) => {
                let mut raw = dict[operand_start..op_off].to_vec();
                raw.extend_from_slice(&[12, 7]);
                font_matrix_raw = Some(raw);
            }
            DictOp::Single(15) => { if let Some(&v) = operands.last() {
                if v > 2 { charset_off = Some(v as usize); }
                else if v >= 0 { charset_predefined = v as u8; }
            } }
            DictOp::Single(17) => { if let Some(&v) = operands.last() && v >= 0 { cs_off = v as usize; } }
            DictOp::Single(18) if operands.len() >= 2 => {
                let sz = operands[operands.len() - 2];
                let po = operands[operands.len() - 1];
                if sz >= 0 && po >= 0 {
                    priv_size = sz as usize;
                    priv_off  = po as usize;
                }
            }
            _ => {}
        }
        DictFlow::Continue
    });

    if cs_off == 0 { return Err("CFF Top DICT: missing CharStrings offset".into()); }
    if priv_off == 0 && fd_array_off.is_none() {
        return Err("CFF Top DICT: missing Private DICT and FDArray".into());
    }

    Ok(TopDictFields {
        charset_off,
        charset_predefined,
        charstrings_off: cs_off,
        private_size:    priv_size,
        private_off:     priv_off,
        fd_array_off,
        fd_select_off,
        ros,
        font_matrix_raw,
    })
}

pub enum CharsetFlow { Continue, Stop }

pub fn walk_charset<F>(cff: &[u8], off: usize, n_glyphs: usize, mut on_entry: F) -> Result<usize, String>
where
    F: FnMut(u16, u16) -> CharsetFlow,
{
    if off >= cff.len() { return Err("CFF charset: offset out of range".into()); }
    if n_glyphs <= 1 { return Ok(off); }
    let format = cff[off];
    let mut pos = off + 1;

    match format {
        0 => {
            for gid in 1..n_glyphs {
                if pos + 2 > cff.len() { return Err("CFF charset format 0: truncated".into()); }
                let sid = read_u16_be(cff, pos).ok_or("CFF charset format 0: truncated")?;
                pos += 2;
                if matches!(on_entry(gid as u16, sid), CharsetFlow::Stop) { return Ok(pos); }
            }
        }
        1 | 2 => {
            let mut gid = 1usize;
            let mut steps = 0usize;
            let steps_cap = n_glyphs.max(cff.len());
            while gid < n_glyphs {
                steps += 1;
                if steps > steps_cap { break; }
                let need = if format == 1 { 3 } else { 4 };
                if pos + need > cff.len() {
                    return Err(format!("CFF charset format {}: truncated", format));
                }
                let first = read_u16_be(cff, pos).ok_or("CFF charset: truncated")?;
                let n_left = if format == 1 {
                    let v = *cff.get(pos + 2).ok_or("CFF charset format 1: truncated")? as usize; pos += 3; v
                } else {
                    let v = read_u16_be(cff, pos + 2).ok_or("CFF charset format 2: truncated")? as usize; pos += 4; v
                };
                let span = (n_left + 1).min(n_glyphs - gid);
                for k in 0..span {
                    if matches!(on_entry((gid + k) as u16, first.wrapping_add(k as u16)), CharsetFlow::Stop) {
                        return Ok(pos);
                    }
                }
                gid += span;
            }
        }
        _ => return Err(format!("CFF charset: unknown format {}", format)),
    }
    Ok(pos)
}

pub fn parse_charset_sids(cff: &[u8], off: usize, n_glyphs: usize) -> Result<usize, String> {
    walk_charset(cff, off, n_glyphs, |_, _| CharsetFlow::Continue)
}

pub fn parse_private_subrs_offset(private_dict: &[u8]) -> usize {
    let mut subrs = 0usize;
    walk_cff_dict(private_dict, DictKind::Cff1, |op, operands, _, _| {
        if op == DictOp::Single(19)
            && let Some(&v) = operands.last() {
                subrs = if v > 0 { v as usize } else { 0 };
                return DictFlow::Stop;
            }
        DictFlow::Continue
    });
    subrs
}

pub fn parse_fd_select_bytes(cff: &[u8], off: usize, n_glyphs: usize) -> Result<Vec<u8>, String> {
    if off >= cff.len() { return Err("CFF FDSelect: offset out of bounds".into()); }
    let format = cff[off];
    let end = match format {
        0 => off + 1 + n_glyphs,
        3 => {
            if off + 3 > cff.len() { return Err("CFF FDSelect format 3: truncated".into()); }
            let n_ranges = read_u16_be(cff, off + 1)
                .ok_or("CFF FDSelect format 3: truncated")? as usize;
            off + 1 + 2 + n_ranges * 3 + 2
        }
        _ => return Err(format!("CFF FDSelect: unknown format {}", format)),
    };
    if end > cff.len() { return Err("CFF FDSelect: data truncated".into()); }
    Ok(cff[off..end].to_vec())
}

pub fn parse_fd_dict_private(dict: &[u8]) -> (usize, usize, Option<Vec<u8>>) {
    let mut priv_size = 0usize;
    let mut priv_off  = 0usize;
    let mut font_matrix_raw: Option<Vec<u8>> = None;
    walk_cff_dict(dict, DictKind::Cff1, |op, operands, operand_start, op_off| {
        match op {
            DictOp::Escaped(7) => {
                let mut raw = dict[operand_start..op_off].to_vec();
                raw.extend_from_slice(&[12, 7]);
                font_matrix_raw = Some(raw);
            }
            DictOp::Single(18) if operands.len() >= 2 => {
                let sz = operands[operands.len() - 2];
                let po = operands[operands.len() - 1];
                if sz >= 0 && po >= 0 {
                    priv_size = sz as usize;
                    priv_off  = po as usize;
                }
            }
            _ => {}
        }
        DictFlow::Continue
    });
    (priv_size, priv_off, font_matrix_raw)
}
