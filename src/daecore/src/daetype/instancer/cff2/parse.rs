use alloc::string::String;
use alloc::vec::Vec;
use crate::daecore::daetype::decoder::read_u16_be;
use crate::daecore::daetype::format::cff::{resolve_fd_select, walk_cff_dict, DictFlow, DictKind, DictOp};
use crate::daecore::daetype::format::ivs::ItemVariationStore;
use crate::daecore::daetype::subsetter::{parse_cff_index, parse_cff_index_refs};

pub(crate) struct Cff2Fd<'a> {
    pub vsindex:     u16,
    pub local_subrs: Vec<&'a [u8]>,
}

pub(crate) struct Cff2Font<'a> {
    pub charstrings:     Vec<&'a [u8]>,
    pub global_subrs:    Vec<&'a [u8]>,
    pub fd_select:        Vec<u16>,
    pub fds:              Vec<Cff2Fd<'a>>,
    pub vstore:            Option<ItemVariationStore>,
    pub font_matrix_raw:  Option<Vec<u8>>,
}

struct Cff2TopDict {
    font_matrix_raw: Option<Vec<u8>>,
    charstrings_off: usize,
    fd_array_off:    usize,
    fd_select_off:   Option<usize>,
    vstore_off:      Option<usize>,
}

pub(crate) fn parse_cff2(cff2: &[u8]) -> Result<Cff2Font<'_>, String> {
    if cff2.len() < 5 { return Err("CFF2: header too short".into()); }
    let header_size     = cff2[2] as usize;
    let top_dict_length = read_u16_be(cff2, 3).ok_or("CFF2: header truncated")? as usize;
    let top_dict_end    = header_size.checked_add(top_dict_length).ok_or("CFF2: Top DICT length overflow")?;
    if top_dict_end > cff2.len() {
        return Err("CFF2: Top DICT out of bounds".into());
    }
    let top_dict = parse_cff2_top_dict(&cff2[header_size..top_dict_end])?;

    let (global_subrs, _) = parse_cff_index_refs(cff2, top_dict_end, true)?;

    let (charstrings, _) = parse_cff_index_refs(cff2, top_dict.charstrings_off, true)?;
    let n_glyphs = charstrings.len();

    let (fd_dicts, _) = parse_cff_index(cff2, top_dict.fd_array_off, true)?;
    if fd_dicts.is_empty() { return Err("CFF2: empty FDArray".into()); }

    let mut fds = Vec::with_capacity(fd_dicts.len());
    for fd_dict in &fd_dicts {
        let (priv_size, priv_off) = find_private_dict_ptr(fd_dict);
        let priv_end = priv_off.saturating_add(priv_size);

        let (vsindex, subrs_offset) = if priv_off > 0 && priv_end <= cff2.len() {
            parse_cff2_private_dict(&cff2[priv_off..priv_end])
        } else {
            (0, 0)
        };

        let local_subrs = if subrs_offset > 0 {
            let abs = priv_off + subrs_offset;
            if abs < cff2.len() {
                parse_cff_index_refs(cff2, abs, true).map(|(subrs, _)| subrs).unwrap_or_default()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        fds.push(Cff2Fd { vsindex, local_subrs });
    }

    let fd_select = match top_dict.fd_select_off {
        Some(off) => resolve_fd_select(cff2, off, n_glyphs)?,
        None => alloc::vec![0u16; n_glyphs],
    };

    let vstore = match top_dict.vstore_off {
        Some(off) => Some(crate::daecore::daetype::format::ivs::parse_item_variation_store(cff2, off + 2)?),
        None => None,
    };

    Ok(Cff2Font {
        charstrings,
        global_subrs,
        fd_select,
        fds,
        vstore,
        font_matrix_raw: top_dict.font_matrix_raw,
    })
}

fn parse_cff2_top_dict(dict: &[u8]) -> Result<Cff2TopDict, String> {
    let mut charstrings_off: Option<usize> = None;
    let mut fd_array_off:    Option<usize> = None;
    let mut fd_select_off:   Option<usize> = None;
    let mut vstore_off:      Option<usize> = None;
    let mut font_matrix_raw: Option<Vec<u8>> = None;

    walk_cff_dict(dict, DictKind::Cff2, |op, operands, operand_start, op_off| {
        match op {
            DictOp::Escaped(36) => { if let Some(&v) = operands.last() && v >= 0 { fd_array_off  = Some(v as usize); } }
            DictOp::Escaped(37) => { if let Some(&v) = operands.last() && v >= 0 { fd_select_off = Some(v as usize); } }
            DictOp::Escaped(7) => {
                let mut raw = dict[operand_start..op_off].to_vec();
                raw.extend_from_slice(&[12, 7]);
                font_matrix_raw = Some(raw);
            }
            DictOp::Single(17) => { if let Some(&v) = operands.last() && v >= 0 { charstrings_off = Some(v as usize); } }
            DictOp::Single(24) => { if let Some(&v) = operands.last() && v >= 0 { vstore_off = Some(v as usize); } }
            _ => {}
        }
        DictFlow::Continue
    });

    Ok(Cff2TopDict {
        font_matrix_raw,
        charstrings_off: charstrings_off.ok_or("CFF2 Top DICT: missing CharStrings offset")?,
        fd_array_off:    fd_array_off.ok_or("CFF2 Top DICT: missing FDArray offset")?,
        fd_select_off,
        vstore_off,
    })
}

fn find_private_dict_ptr(fd_dict: &[u8]) -> (usize, usize) {
    let mut priv_size = 0usize;
    let mut priv_off  = 0usize;
    walk_cff_dict(fd_dict, DictKind::Cff2, |op, operands, _, _| {
        if op == DictOp::Single(18) && operands.len() >= 2 {
            let sz = operands[operands.len() - 2];
            let po = operands[operands.len() - 1];
            if sz >= 0 && po >= 0 {
                priv_size = sz as usize;
                priv_off  = po as usize;
            }
        }
        DictFlow::Continue
    });
    (priv_size, priv_off)
}

fn parse_cff2_private_dict(private_dict: &[u8]) -> (u16, usize) {
    let mut vsindex: u16 = 0;
    let mut subrs_offset: usize = 0;
    walk_cff_dict(private_dict, DictKind::Cff2, |op, operands, _, _| {
        match op {
            DictOp::Single(19) => { if let Some(&v) = operands.last() { subrs_offset = if v > 0 { v as usize } else { 0 }; } }
            DictOp::Single(22) => { if let Some(&v) = operands.last() { vsindex = v.max(0) as u16; } }
            _ => {}
        }
        DictFlow::Continue
    });
    (vsindex, subrs_offset)
}
