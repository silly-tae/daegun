use super::*;

pub fn cff_charstrings_for_closure(cff: &[u8]) -> Result<(Vec<&[u8]>, Option<usize>), String> {
    if cff.len() < 4 { return Err("CFF: file too short".into()); }
    let hdr_size = cff[2] as usize;
    let (_, after_name) = parse_cff_index_refs(cff, hdr_size, false)?;
    let (top_dicts, _) = parse_cff_index_refs(cff, after_name, false)?;
    let top_dict = top_dicts.into_iter().next().ok_or("CFF: empty Top DICT INDEX")?;
    let fields = parse_top_dict(top_dict)?;
    let (charstrings, _) = parse_cff_index_refs(cff, fields.charstrings_off, false)?;
    Ok((charstrings, fields.charset_off))
}

pub fn subset_cff(cff: &[u8], requested: &[u16]) -> Result<SubsetResult, String> {
    subset_cff_inner(cff, requested, false)
}

pub fn subset_cff_compacting(cff: &[u8], requested: &[u16]) -> Result<SubsetResult, String> {
    subset_cff_inner(cff, requested, true)
}

fn fd_select_per_glyph(fdselect: &[u8], n_glyphs: usize) -> Option<Vec<u8>> {
    let mut fds = vec![0u8; n_glyphs];
    match *fdselect.first()? {
        0 => for (g, f) in fds.iter_mut().enumerate() { *f = *fdselect.get(1 + g)?; },
        3 => {
            let n_ranges = read_u16_be(fdselect, 1)? as usize;
            for i in 0..n_ranges {
                let first = read_u16_be(fdselect, 3 + i * 3)? as usize;
                let fd = *fdselect.get(3 + i * 3 + 2)?;
                let next = read_u16_be(fdselect, 3 + (i + 1) * 3)? as usize;
                for f in fds.get_mut(first..next.min(n_glyphs))? { *f = fd; }
            }
        }
        _ => return None,
    }
    Some(fds)
}

fn subset_cff_inner(cff: &[u8], requested: &[u16], compact: bool) -> Result<SubsetResult, String> {
    if cff.len() < 4 {
        return Err("CFF: file too short".into());
    }
    let hdr_size = cff[2] as usize;

    let (_, after_name)        = parse_cff_index_refs(cff, hdr_size, false)?;
    let (top_dicts, after_top) = parse_cff_index_refs(cff, after_name, false)?;
    let top_dict = top_dicts.into_iter().next()
        .ok_or("CFF: empty Top DICT INDEX")?;
    let fields = parse_top_dict(top_dict)?;

    let (strings, after_strings) = parse_cff_index_refs(cff, after_top, false)?;
    let (_, after_gsubrs)  = parse_cff_index_refs(cff, after_strings, false)?;

    let name_index_bytes   = &cff[hdr_size..after_name];
    let gsubr_index_bytes  = &cff[after_strings..after_gsubrs];

    let (charstrings, _) = parse_cff_index_refs(cff, fields.charstrings_off, false)?;
    let n_glyphs = charstrings.len();

    let verbatim_charset = match fields.charset_off {
        Some(off) => {
            let end = parse_charset_sids(cff, off, n_glyphs)?;
            cff[off..end].to_vec()
        }
        None => vec![],
    };

    let mut active = BTreeSet::<u16>::new();
    active.insert(0);
    for &gid in requested {
        if (gid as usize) < n_glyphs { active.insert(gid); }
    }

    let format0_map = seac::build_format0_sid_to_gid_map(cff, fields.charset_off, n_glyphs);

    let mut frontier: Vec<u16> = active.iter().copied().collect();
    let mut seac_chase_steps = 0usize;
    while !frontier.is_empty() {
        if seac_chase_steps > n_glyphs { break; }
        seac_chase_steps += 1;
        let comps = seac::seac_component_gids(&charstrings, &frontier, cff, fields.charset_off, format0_map.as_ref());
        let mut next_frontier = Vec::new();
        for c in comps {
            if (c as usize) < n_glyphs && active.insert(c) {
                next_frontier.push(c);
            }
        }
        frontier = next_frontier;
    }

    let active_sorted: Vec<u16> = active.iter().copied().collect();
    let gid_map: Vec<u16> = if compact {
        let mut m = vec![0u16; *active_sorted.last().unwrap_or(&0) as usize + 1];
        for (new, &orig) in active_sorted.iter().enumerate() { m[orig as usize] = new as u16; }
        m
    } else {
        vec![]
    };

    let endchar: &[u8] = &[0x0eu8];
    let new_charstrings: Vec<&[u8]> = if compact {
        active_sorted
            .iter()
            .map(|&g| charstrings.get(g as usize).copied().unwrap_or(endchar))
            .collect()
    } else {
        (0..n_glyphs)
            .map(|gid| {
                if active.contains(&(gid as u16)) { charstrings[gid] }
                else { endchar }
            })
            .collect()
    };
    let new_charstrings_index = encode_cff_index_refs(&new_charstrings);

    const N_STD_STRINGS: u16 = 391;
    let is_cid = fields.fd_array_off.is_some();

    let (charset_bytes, string_index_bytes, ros_sids) = if compact {
        let mut sids = vec![0u16; n_glyphs];
        match fields.charset_off {
            Some(off) => {
                walk_charset(cff, off, n_glyphs, |gid, sid| {
                    if let Some(slot) = sids.get_mut(gid as usize) { *slot = sid; }
                    CharsetFlow::Continue
                })?;
            }
            None => {
                let table: &[u16] = match fields.charset_predefined {
                    1 => &expert_charsets::EXPERT_CHARSET,
                    2 => &expert_charsets::EXPERT_SUBSET_CHARSET,
                    _ => &[],
                };
                for (gid, slot) in sids.iter_mut().enumerate() {
                    *slot = if table.is_empty() { gid as u16 } else { table.get(gid).copied().unwrap_or(0) };
                }
            }
        }

        let mut needed: BTreeSet<u16> = BTreeSet::new();
        if is_cid {
            if let Some((reg, ord, _)) = fields.ros {
                for v in [reg, ord] {
                    if (0..=u16::MAX as i32).contains(&v) && v as u16 >= N_STD_STRINGS { needed.insert(v as u16); }
                }
            }
        } else {
            for &orig in active_sorted.iter().skip(1) {
                if sids[orig as usize] >= N_STD_STRINGS { needed.insert(sids[orig as usize]); }
            }
        }
        let kept: Vec<u16> = needed.into_iter().collect();
        let remap = |sid: u16| -> u16 {
            match kept.binary_search(&sid) {
                Ok(i) => N_STD_STRINGS + i as u16,
                Err(_) => sid,
            }
        };
        let refs: Vec<&[u8]> = kept.iter()
            .map(|&sid| strings.get((sid - N_STD_STRINGS) as usize).map_or(&[][..], |v| *v))
            .collect();

        let mut b = Vec::with_capacity(1 + 2 * active_sorted.len());
        b.push(0);
        for &orig in active_sorted.iter().skip(1) {
            let sid = sids[orig as usize];
            b.extend_from_slice(&if is_cid { sid } else { remap(sid) }.to_be_bytes());
        }
        let ros = fields.ros.map(|(r, o, s)| (remap(r as u16) as i32, remap(o as u16) as i32, s));
        (b, encode_cff_index_refs(&refs), ros)
    } else {
        (verbatim_charset, cff[after_top..after_strings].to_vec(), fields.ros)
    };
    let string_index_bytes = &string_index_bytes[..];

    if let Some(fd_array_off) = fields.fd_array_off {
        let fd_select_off = fields.fd_select_off
            .ok_or("CFF CID: missing FDSelect offset")?;
        let ros = ros_sids.ok_or("CFF CID: missing ROS")?;

        let fdselect_bytes = {
            let src = parse_fd_select_bytes(cff, fd_select_off, n_glyphs)?;
            if compact {
                let per_glyph = fd_select_per_glyph(&src, n_glyphs)
                    .ok_or("CFF CID: unrecognized FDSelect format")?;
                let mut b = Vec::with_capacity(1 + active_sorted.len());
                b.push(0);
                for &orig in &active_sorted { b.push(per_glyph[orig as usize]); }
                b
            } else {
                src
            }
        };

        let (fd_dicts, _) = parse_cff_index(cff, fd_array_off, false)?;
        let n_fds = fd_dicts.len();
        if n_fds == 0 { return Err("CFF CID: empty FDArray".into()); }

        let mut fd_priv_sizes   = Vec::with_capacity(n_fds);
        let mut fd_priv_bytes   = Vec::with_capacity(n_fds);
        let mut fd_lsubrs_bytes = Vec::with_capacity(n_fds);
        let mut fd_subrs_rels   = Vec::with_capacity(n_fds);
        let mut fd_matrices     = Vec::with_capacity(n_fds);

        for fd_dict in &fd_dicts {
            let (priv_size, priv_off, fd_matrix) = parse_fd_dict_private(fd_dict);
            let priv_end = priv_off.saturating_add(priv_size);
            if priv_end > cff.len() {
                return Err("CFF CID: FD Private DICT out of bounds".into());
            }
            let priv_data = cff[priv_off..priv_end].to_vec();
            let subrs_rel = match parse_private_subrs_offset(&priv_data) {
                r if r >= priv_size => r,
                _ => 0,
            };
            let lsubrs = if subrs_rel > 0 {
                let abs = priv_off + subrs_rel;
                if abs < cff.len() {
                    let (_, end) = parse_cff_index(cff, abs, false)?;
                    cff[abs..end].to_vec()
                } else { vec![] }
            } else { vec![] };
            fd_priv_sizes.push(priv_size);
            fd_lsubrs_bytes.push(lsubrs);
            fd_subrs_rels.push(subrs_rel);
            fd_priv_bytes.push(priv_data);
            fd_matrices.push(fd_matrix);
        }

        let fd_placeholder: Vec<Vec<u8>> = (0..n_fds)
            .map(|i| vec![0u8; 11 + fd_matrices[i].as_ref().map_or(0, |m: &Vec<u8>| m.len())])
            .collect();
        let fdarray_size = encode_cff_index(&fd_placeholder).len();

        let fm_len = fields.font_matrix_raw.as_ref().map_or(0, |m| m.len());
        let top_dict_data_len   = 43 + fm_len;
        let top_dict_index_size = cff_index_size(top_dict_data_len);
        let base = hdr_size + name_index_bytes.len() + top_dict_index_size
            + string_index_bytes.len() + gsubr_index_bytes.len();

        let charset_val         = if charset_bytes.is_empty() { 0i32 } else { base as i32 };
        let new_fdselect_off    = base + charset_bytes.len();
        let new_charstrings_off = new_fdselect_off + fdselect_bytes.len();
        let new_fdarray_off     = new_charstrings_off + new_charstrings_index.len();

        let mut fd_new_priv_offs = Vec::with_capacity(n_fds);
        let mut cur = new_fdarray_off + fdarray_size;
        for i in 0..n_fds {
            fd_new_priv_offs.push(cur);
            if fd_subrs_rels[i] > 0 && !fd_lsubrs_bytes[i].is_empty() {
                cur += fd_subrs_rels[i] + fd_lsubrs_bytes[i].len();
            } else {
                cur += fd_priv_sizes[i];
            }
        }

        let fd_dict_entries: Vec<Vec<u8>> = (0..n_fds)
            .map(|i| {
                let mut d = Vec::with_capacity(fd_placeholder[i].len());
                if let Some(ref fm) = fd_matrices[i] {
                    d.extend_from_slice(fm);
                }
                d.extend(encode_cff_int(fd_priv_sizes[i] as i32));
                d.extend(encode_cff_int(fd_new_priv_offs[i] as i32));
                d.push(18);
                d
            })
            .collect();
        let new_fdarray_index = encode_cff_index(&fd_dict_entries);
        debug_assert_eq!(new_fdarray_index.len(), fdarray_size);

        let mut top_dict_data = Vec::with_capacity(top_dict_data_len);
        top_dict_data.extend(encode_cff_int(ros.0));
        top_dict_data.extend(encode_cff_int(ros.1));
        top_dict_data.extend(encode_cff_int(ros.2));
        top_dict_data.extend_from_slice(&[12u8, 30u8]);
        if let Some(ref fm) = fields.font_matrix_raw {
            top_dict_data.extend_from_slice(fm);
        }
        top_dict_data.extend(encode_cff_int(charset_val));
        top_dict_data.push(15);
        top_dict_data.extend(encode_cff_int(new_charstrings_off as i32));
        top_dict_data.push(17);
        top_dict_data.extend(encode_cff_int(new_fdarray_off as i32));
        top_dict_data.extend_from_slice(&[12u8, 36u8]);
        top_dict_data.extend(encode_cff_int(new_fdselect_off as i32));
        top_dict_data.extend_from_slice(&[12u8, 37u8]);

        let new_top_dict_index = encode_cff_index(&[top_dict_data]);
        debug_assert_eq!(new_top_dict_index.len(), top_dict_index_size);

        let mut out = Vec::new();
        out.extend_from_slice(&cff[..hdr_size]);
        out.extend_from_slice(name_index_bytes);
        out.extend_from_slice(&new_top_dict_index);
        out.extend_from_slice(string_index_bytes);
        out.extend_from_slice(gsubr_index_bytes);
        out.extend_from_slice(&charset_bytes);
        out.extend_from_slice(&fdselect_bytes);
        out.extend_from_slice(&new_charstrings_index);
        out.extend_from_slice(&new_fdarray_index);

        for i in 0..n_fds {
            out.extend_from_slice(&fd_priv_bytes[i]);
            if fd_subrs_rels[i] > 0 && !fd_lsubrs_bytes[i].is_empty() {
                let target = fd_new_priv_offs[i] + fd_subrs_rels[i];
                let pad_limit = out.len() + cff.len() * 2 + 4096;
                while out.len() < target {
                    if out.len() >= pad_limit {
                        return Err("CFF CID: FD padding target implausibly large".into());
                    }
                    out.push(0);
                }
                out.extend_from_slice(&fd_lsubrs_bytes[i]);
            }
        }

        return Ok(SubsetResult { ttf: out, gid_map });
    }

    let priv_end = fields.private_off.saturating_add(fields.private_size);
    if priv_end > cff.len() {
        return Err("CFF: Private DICT out of bounds".into());
    }
    let private_bytes     = cff[fields.private_off..priv_end].to_vec();
    let subrs_rel         = match parse_private_subrs_offset(&private_bytes) {
        r if r >= fields.private_size => r,
        _ => 0,
    };
    let local_subrs_bytes = if subrs_rel > 0 {
        let subrs_abs = fields.private_off + subrs_rel;
        if subrs_abs < cff.len() {
            let (_, end) = parse_cff_index(cff, subrs_abs, false)?;
            cff[subrs_abs..end].to_vec()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    let fm_len = fields.font_matrix_raw.as_ref().map_or(0, |m| m.len());
    let top_dict_data_len   = 23 + fm_len;
    let top_dict_index_size = cff_index_size(top_dict_data_len);
    let base = hdr_size
        + name_index_bytes.len()
        + top_dict_index_size
        + string_index_bytes.len()
        + gsubr_index_bytes.len();

    let charset_value       = if charset_bytes.is_empty() { 0i32 } else { base as i32 };
    let new_charstrings_off = base + charset_bytes.len();
    let new_private_off     = new_charstrings_off + new_charstrings_index.len();

    let mut top_dict_data = Vec::with_capacity(top_dict_data_len);
    if let Some(ref fm) = fields.font_matrix_raw {
        top_dict_data.extend_from_slice(fm);
    }
    top_dict_data.extend(encode_cff_int(charset_value));
    top_dict_data.push(15);
    top_dict_data.extend(encode_cff_int(new_charstrings_off as i32));
    top_dict_data.push(17);
    top_dict_data.extend(encode_cff_int(fields.private_size as i32));
    top_dict_data.extend(encode_cff_int(new_private_off as i32));
    top_dict_data.push(18);

    let new_top_dict_index = encode_cff_index(&[top_dict_data]);
    debug_assert_eq!(new_top_dict_index.len(), top_dict_index_size);

    let mut out = Vec::new();
    out.extend_from_slice(&cff[..hdr_size]);
    out.extend_from_slice(name_index_bytes);
    out.extend_from_slice(&new_top_dict_index);
    out.extend_from_slice(string_index_bytes);
    out.extend_from_slice(gsubr_index_bytes);
    out.extend_from_slice(&charset_bytes);
    out.extend_from_slice(&new_charstrings_index);
    out.extend_from_slice(&private_bytes);

    if !local_subrs_bytes.is_empty() && subrs_rel > 0 {
        let target = new_private_off + subrs_rel;
        let pad_limit = out.len() + cff.len() * 2 + 4096;
        while out.len() < target {
            if out.len() >= pad_limit {
                return Err("CFF: Private DICT padding target implausibly large".into());
            }
            out.push(0);
        }
        out.extend_from_slice(&local_subrs_bytes);
    }

    Ok(SubsetResult { ttf: out, gid_map })
}
