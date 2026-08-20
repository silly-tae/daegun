use super::*;

pub fn subset_ttf(ttf: &[u8], requested: &[u16]) -> Result<SubsetResult, String> {
    let dir = parse_ttf_dir(ttf);

    let head    = slice_table(ttf, &dir, "head").ok_or("subset: missing head")?;
    let maxp    = slice_table(ttf, &dir, "maxp").ok_or("subset: missing maxp")?;
    let loca_fmt   = read_i16_be(head, 50).ok_or("subset: head table truncated")?;
    let num_glyphs = read_u16_be(maxp, 4).ok_or("subset: maxp table truncated")? as usize;
    if num_glyphs == 0 { return Err("subset: maxp reports zero glyphs".into()); }

    let outlines: Option<(&[u8], Vec<usize>)> =
        match (slice_table(ttf, &dir, "glyf"), slice_table(ttf, &dir, "loca")) {
            (Some(glyf), Some(loca_sl)) => Some((glyf, parse_loca(loca_sl, loca_fmt, num_glyphs))),
            (None, None) => None,
            (Some(_), None) => return Err("subset: font has glyf but no loca".into()),
            (None, Some(_)) => return Err("subset: font has loca but no glyf".into()),
        };
    let closure_into = |req: &[u16], outlines: &Option<(&[u8], Vec<usize>)>, set: &mut GlyphSet| match outlines {
        Some((glyf, loca_offs)) => active_gids_into(req, glyf, loca_offs, num_glyphs, set),
        None => set.extend(req.iter().copied().filter(|&g| (g as usize) < num_glyphs)),
    };
    let mut active = GlyphSet::new();
    active.insert(0);
    closure_into(requested, &outlines, &mut active);

    let orig_gsub = slice_table(ttf, &dir, "GSUB");
    let orig_colr = slice_table(ttf, &dir, "COLR");
    let orig_math = slice_table(ttf, &dir, "MATH");
    let orig_morx = slice_table(ttf, &dir, "morx");
    if orig_gsub.is_some() || orig_colr.is_some() || orig_math.is_some() || orig_morx.is_some() {
        const MAX_CLOSURE_PASSES: usize = 64;
        let mut steps = 0usize;
        loop {
            steps += 1;
            if steps > MAX_CLOSURE_PASSES {
                return Err("subset: glyph closure did not converge within its pass budget".into());
            }
            let mut new_gids: Vec<u16> = Vec::new();
            if let Some(gsub) = orig_gsub {
                new_gids.extend(otl::gsub::gsub_closure(gsub, &active));
            }
            if let Some(colr) = orig_colr {
                new_gids.extend(colr::colr_closure(colr, &active));
            }
            if let Some(m) = orig_math {
                new_gids.extend(math::math_closure(m, &active));
            }
            if let Some(m) = orig_morx {
                new_gids.extend(aat::morx::morx_closure(m, &active, num_glyphs as u16));
            }
            new_gids.retain(|g| !active.contains(g));
            if new_gids.is_empty() { break; }
            closure_into(&new_gids, &outlines, &mut active);
        }
    }

    let active_sorted: Vec<u16> = active.iter().collect();
    let n_active = active_sorted.len();

    let max_orig = *active_sorted.last().unwrap_or(&0) as usize;
    let mut gid_map = vec![0u16; max_orig + 1];
    for (compact, &orig) in active_sorted.iter().enumerate() {
        gid_map[orig as usize] = compact as u16;
    }

    let mut new_glyf: Vec<u8> = Vec::new();
    let mut new_loca = vec![0u32; n_active + 1];

    for (compact, &orig_gid) in active_sorted.iter().enumerate() {
        let Some((glyf, loca_offs)) = outlines.as_ref() else { break };
        new_loca[compact] = new_glyf.len() as u32;
        let (s, e) = (loca_offs[orig_gid as usize], loca_offs[orig_gid as usize + 1]);
        if s < e && e <= glyf.len() {
            let glyph_start = new_glyf.len();
            new_glyf.extend_from_slice(&glyf[s..e]);
            let is_compound = read_i16_be(&new_glyf, glyph_start) == Some(-1);
            if is_compound {
                let glyph_end = new_glyf.len();
                patch_compound_gids(&mut new_glyf, glyph_start, glyph_end, &gid_map);
            }
            let mut align_steps = 0usize;
            while !new_glyf.len().is_multiple_of(4) {
                if align_steps >= 4 { break; }
                align_steps += 1;
                new_glyf.push(0);
            }
        }
    }
    new_loca[n_active] = new_glyf.len() as u32;

    let use_short_loca = new_glyf.len() <= 0x1_FFFE;
    let new_loca_bytes = if use_short_loca {
        let mut b = vec![0u8; (n_active + 1) * 2];
        for (i, &off) in new_loca.iter().enumerate() {
            write_u16_be(&mut b, i * 2, (off / 2) as u16);
        }
        b
    } else {
        let mut b = vec![0u8; (n_active + 1) * 4];
        for (i, &off) in new_loca.iter().enumerate() {
            write_u32_be(&mut b, i * 4, off);
        }
        b
    };

    let mut new_head = head.to_vec();
    write_i16_be(&mut new_head, 50, if use_short_loca { 0 } else { 1 });

    let (new_hmtx, new_hhea) = {
        let orig_hmtx   = slice_table(ttf, &dir, "hmtx").unwrap_or(&[]);
        let mut new_hhea = owned_table(ttf, &dir, "hhea").unwrap_or_default();
        let orig_num_hm = if new_hhea.len() >= 36 { read_u16_be(&new_hhea, 34).unwrap_or(0) as usize } else { 0 };
        let h = rebuild_metrics(orig_hmtx, orig_num_hm, &active_sorted);
        if new_hhea.len() >= 36 { write_u16_be(&mut new_hhea, 34, n_active as u16); }
        (h, new_hhea)
    };

    let vertical = {
        match (slice_table(ttf, &dir, "vmtx"), owned_table(ttf, &dir, "vhea")) {
            (Some(vmtx), Some(mut vhea)) if vhea.len() >= 36 => {
                let orig_num_vm = read_u16_be(&vhea, 34).unwrap_or(0) as usize;
                let v = rebuild_metrics(vmtx, orig_num_vm, &active_sorted);
                write_u16_be(&mut vhea, 34, n_active as u16);
                Some((v, vhea))
            }
            _ => None,
        }
    };

    let new_maxp = {
        let mut m = owned_table(ttf, &dir, "maxp").unwrap_or_default();
        if m.len() >= 6 { write_u16_be(&mut m, 4, n_active as u16); }
        m
    };

    let mut tmap: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    if let Some(d) = owned_table(ttf, &dir, "OS/2") { tmap.insert("OS/2".to_string(), d); }
    for tag in ["cvt ", "fpgm", "prep"] {
        if let Some(d) = owned_table(ttf, &dir, tag) { tmap.insert(tag.to_string(), d); }
    }
    if let Some(post) = owned_table(ttf, &dir, "post") {
        tmap.insert("post".to_string(), fix_post_table(post));
    }
    tmap.insert("maxp".to_string(), new_maxp);
    tmap.insert("hmtx".to_string(), new_hmtx);
    tmap.insert("hhea".to_string(), new_hhea);
    tmap.insert("head".to_string(), new_head);
    if outlines.is_some() {
        tmap.insert("glyf".to_string(), new_glyf);
        tmap.insert("loca".to_string(), new_loca_bytes);
    }
    for (data_tag, loc_tag) in [("CBDT", "CBLC"), ("EBDT", "EBLC")] {
        let (Some(data), Some(loc)) = (slice_table(ttf, &dir, data_tag), slice_table(ttf, &dir, loc_tag))
        else { continue };
        if let Some((new_loc, new_data)) = bitmap::subset_bitmap_strikes(loc, data, &active, &gid_map) {
            tmap.insert(loc_tag.to_string(), new_loc);
            tmap.insert(data_tag.to_string(), new_data);
        }
    }
    if let Some(sbix) = slice_table(ttf, &dir, "sbix")
        && let Some(new_sbix) = bitmap::subset_sbix(sbix, num_glyphs, &active_sorted, &gid_map) {
            tmap.insert("sbix".to_string(), new_sbix);
        }
    if let Some((new_vmtx, new_vhea)) = vertical {
        tmap.insert("vmtx".to_string(), new_vmtx);
        tmap.insert("vhea".to_string(), new_vhea);
    }
    if let Some(vorg) = slice_table(ttf, &dir, "VORG")
        && let Some(remapped) = remap_vorg(vorg, &gid_map, &active) {
            tmap.insert("VORG".to_string(), remapped);
        }
    let orig_gdef = slice_table(ttf, &dir, "GDEF");
    let mark_filter_sets_survive = orig_gdef.is_some_and(otl::gdef::has_mark_glyph_sets);
    if let Some(gdef) = orig_gdef
        && let Some(new_gdef) = otl::gdef::subset_gdef(gdef, &active, &gid_map) {
            tmap.insert("GDEF".to_string(), new_gdef);
        }
    if let Some(gsub) = slice_table(ttf, &dir, "GSUB")
        && let Some(new_gsub) = otl::gsub::subset_gsub(gsub, &active, &gid_map, mark_filter_sets_survive) {
            tmap.insert("GSUB".to_string(), new_gsub);
        }
    if let Some(gpos) = slice_table(ttf, &dir, "GPOS")
        && let Some(new_gpos) = otl::gpos::subset_gpos(gpos, &active, &gid_map, mark_filter_sets_survive) {
            tmap.insert("GPOS".to_string(), new_gpos);
        }
    if let Some(m) = orig_math
        && let Some(new_math) = math::subset_math(m, &active, &gid_map) {
            tmap.insert("MATH".to_string(), new_math);
        }
    if let Some(j) = slice_table(ttf, &dir, "JSTF")
        && let Some(new_jstf) = jstf::subset_jstf(j, &active, &gid_map) {
            tmap.insert("JSTF".to_string(), new_jstf);
        }
    if let Some(h) = slice_table(ttf, &dir, "hdmx")
        && let Some(new_hdmx) = device_metrics::subset_hdmx(h, num_glyphs, &active_sorted) {
            tmap.insert("hdmx".to_string(), new_hdmx);
        }
    if let Some(l) = slice_table(ttf, &dir, "LTSH")
        && let Some(new_ltsh) = device_metrics::subset_ltsh(l, &active_sorted) {
            tmap.insert("LTSH".to_string(), new_ltsh);
        }
    if let Some(p) = slice_table(ttf, &dir, "prop")
        && let Some(new_prop) = aat::prop::subset_prop(p, num_glyphs, &active, &gid_map) {
            tmap.insert("prop".to_string(), new_prop);
        }
    for (tag, rebuilt) in [
        ("lcar", slice_table(ttf, &dir, "lcar").and_then(|d| aat::simple::subset_lcar(d, &active, &gid_map, num_glyphs as u16))),
        ("opbd", slice_table(ttf, &dir, "opbd").and_then(|d| aat::simple::subset_opbd(d, &active, &gid_map, num_glyphs as u16))),
        ("ankr", slice_table(ttf, &dir, "ankr").and_then(|d| aat::simple::subset_ankr(d, &active, &gid_map, num_glyphs as u16))),
        ("bsln", slice_table(ttf, &dir, "bsln").and_then(|d| aat::simple::subset_bsln(d, &active, &gid_map, num_glyphs as u16))),
        ("fmtx", slice_table(ttf, &dir, "fmtx").and_then(|d| aat::simple::subset_fmtx(d, &active, &gid_map))),
        ("Zapf", slice_table(ttf, &dir, "Zapf").and_then(|d| aat::zapf::subset_zapf(d, num_glyphs, &active_sorted, &active, &gid_map))),
    ] {
        if let Some(bytes) = rebuilt { tmap.insert(tag.to_string(), bytes); }
    }
    if let (Some(data), Some(loc)) = (slice_table(ttf, &dir, "bdat"), slice_table(ttf, &dir, "bloc"))
        && let Some((new_loc, new_data)) = bitmap::subset_bitmap_strikes(loc, data, &active, &gid_map) {
            tmap.insert("bloc".to_string(), new_loc);
            tmap.insert("bdat".to_string(), new_data);
        }
    if let Some(e) = slice_table(ttf, &dir, "EBSC") {
        let surviving = tmap.get("EBLC").map(|b| aat::descriptive::strike_sizes(b)).unwrap_or_default();
        if let Some(new_ebsc) = aat::descriptive::subset_ebsc(e, &surviving) {
            tmap.insert("EBSC".to_string(), new_ebsc);
        }
    }
    for tag in ["feat", "trak", "fdsc", "ltag"] {
        if let Some(d) = owned_table(ttf, &dir, tag) { tmap.insert(tag.to_string(), d); }
    }
    if let Some(m) = orig_morx
        && let Some(new_morx) = aat::morx::subset_morx(m, &active, &gid_map, num_glyphs as u16) {
            tmap.insert("morx".to_string(), new_morx);
        }
    if let Some(j) = slice_table(ttf, &dir, "just")
        && let Some(new_just) = aat::just::subset_just(j, &active, &gid_map, num_glyphs as u16) {
            tmap.insert("just".to_string(), new_just);
        }
    if let Some(k) = slice_table(ttf, &dir, "kerx")
        && let Some(new_kerx) = aat::kerx::subset_kerx(k, &active, &gid_map, num_glyphs as u16) {
            tmap.insert("kerx".to_string(), new_kerx);
        }
    if let Some(x) = slice_table(ttf, &dir, "xref") {
        let kerx_stable = match (slice_table(ttf, &dir, "kerx"), tmap.get("kerx")) {
            (Some(before), Some(after)) => read_u32_be(before, 4) == read_u32_be(after, 4),
            (None, None) => true,
            _ => false,
        };
        if let Some(new_xref) = aat::descriptive::subset_xref(x, |tag| tag != b"kerx" || kerx_stable) {
            tmap.insert("xref".to_string(), new_xref);
        }
    }
    if let Some(kern) = slice_table(ttf, &dir, "kern")
        && let Some(remapped) = remap_kern(kern, &gid_map, &active) {
            tmap.insert("kern".to_string(), remapped);
        }
    if let Some(colr) = orig_colr
        && let Some(new_colr) = colr::subset_colr(colr, &active, &gid_map) {
            tmap.insert("COLR".to_string(), new_colr);
            if let Some(cpal) = owned_table(ttf, &dir, "CPAL") {
                tmap.insert("CPAL".to_string(), cpal);
            }
        }

    Ok(SubsetResult { ttf: build_ttf(&tmap), gid_map })
}
