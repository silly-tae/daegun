#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use crate::daecore::daetype::decoder::{read_u16_be, write_u16_be};
use crate::daecore::daetype::instancer::{compute_location, apply_hvar, apply_vvar, apply_mvar, expand_metrics, strip_gdef_var_store, apply_style_metadata, resolve_feature_variations};
use crate::daecore::daetype::subsetter::cff::build::{cff_index_size, cff_index_flat_size, append_cff_index_chunks, encode_cff_index, encode_cff_index_flat, encode_cff_int};
use super::parse::parse_cff2;
use crate::daecore::daetype::TableBytes;

const STRIP_AFTER_CFF2_INSTANCING: &[&str] = &["fvar", "HVAR", "MVAR", "avar", "VVAR", "CFF2"];

pub(crate) fn instance_cff2_from_map<'a>(
    table_map:   &'a BTreeMap<String, TableBytes>,
    axis_values: &[(String, f64)],
) -> Result<BTreeMap<String, Cow<'a, [u8]>>, String> {
    let location  = compute_location(table_map, axis_values)?;
    let needs_var = location.iter().any(|&v| v != 0.0);

    let cff2_data = table_map.get("CFF2").ok_or("missing CFF2")?;
    let cff2 = parse_cff2(cff2_data)?;
    let n_glyphs = cff2.charstrings.len();

    let mut cff2_budget =
        u32::try_from(cff2_data.len().saturating_mul(64)).unwrap_or(u32::MAX);

    let mut scratch = super::charstring::Scratch::default();
    let (chunks, charstring_ends) =
        resolve_all_charstrings(&cff2, &location, n_glyphs, &mut cff2_budget, &mut scratch)?;

    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|t| t.as_slice()).collect();
    let cff1 = build_cff1(&chunk_refs, &charstring_ends, cff2.font_matrix_raw.as_ref(), &instance_font_name(axis_values));

    let num_glyphs_hint = table_map.get("maxp").and_then(|m| read_u16_be(m, 4)).unwrap_or(0) as usize;
    let mut hmtx_data = table_map.get("hmtx").ok_or("missing hmtx")?.to_owned_vec();
    let mut os2_data  = table_map.get("OS/2").ok_or("missing OS/2")?.to_owned_vec();
    let mut hhea_data = table_map.get("hhea").map(TableBytes::to_owned_vec).unwrap_or_default();
    let mut post_data = table_map.get("post").map(TableBytes::to_owned_vec).unwrap_or_default();
    let mut vmtx_data = table_map.get("vmtx").map(TableBytes::to_owned_vec);

    let long_metrics = |tag: &str| {
        table_map.get(tag).and_then(|h| read_u16_be(h, 34)).map_or(0usize, usize::from)
    };
    let mut hmtx_metrics = long_metrics("hhea");
    let mut vmtx_metrics = long_metrics("vhea");

    if needs_var {
        if hmtx_metrics < num_glyphs_hint {
            hmtx_data = expand_metrics(&hmtx_data, num_glyphs_hint, hmtx_metrics);
            hmtx_metrics = num_glyphs_hint;
        }
        if vmtx_metrics < num_glyphs_hint {
            if let Some(ref mut vmtx) = vmtx_data {
                *vmtx = expand_metrics(vmtx, num_glyphs_hint, vmtx_metrics);
            }
            vmtx_metrics = num_glyphs_hint;
        }

        if table_map.contains_key("HVAR") {
            apply_hvar(table_map, &mut hmtx_data, num_glyphs_hint, hmtx_metrics, &location)?;
        }
        if table_map.contains_key("VVAR")
            && let Some(ref mut vmtx) = vmtx_data {
                apply_vvar(table_map, vmtx, num_glyphs_hint, vmtx_metrics, &location)?;
            }
        apply_mvar(table_map, &mut hhea_data, &mut os2_data, &mut post_data, &location)?;
    }

    if hhea_data.len() >= 36 {
        write_u16_be(&mut hhea_data, 34, hmtx_metrics.min(0xFFFF) as u16);
    }
    let vhea_data = vmtx_data.as_ref().and_then(|_| {
        table_map.get("vhea").filter(|v| v.len() >= 36).map(|vhea| {
            let mut vhea = vhea.to_owned_vec();
            write_u16_be(&mut vhea, 34, vmtx_metrics.min(0xFFFF) as u16);
            vhea
        })
    });

    let style = apply_style_metadata(table_map, axis_values, &mut os2_data);

    let mut out_map: BTreeMap<String, Cow<[u8]>> = BTreeMap::new();
    for (tag, data) in table_map {
        if !STRIP_AFTER_CFF2_INSTANCING.contains(&tag.as_str()) {
            out_map.insert(tag.clone(), Cow::Borrowed(data.as_slice()));
        }
    }
    if needs_var
        && let Some(patched) = crate::daecore::daetype::instancer::apply_gpos_var(table_map, &location) {
            out_map.insert("GPOS".to_string(), Cow::Owned(patched));
        }
    if let Some(gdef) = table_map.get("GDEF")
        && let Some(stripped) = strip_gdef_var_store(gdef) {
            out_map.insert("GDEF".to_string(), Cow::Owned(stripped));
        }
    if let Some(gsub) = table_map.get("GSUB")
        && let Some(resolved) = resolve_feature_variations(gsub, &location) {
            out_map.insert("GSUB".to_string(), Cow::Owned(resolved));
        }
    if let Some(colr) = table_map.get("COLR")
        && let Some(instanced) = crate::daecore::daetype::colr_v1::instance_colr_v1(colr, &location) {
            out_map.insert("COLR".to_string(), Cow::Owned(instanced));
        }
    out_map.insert("CFF ".to_string(), Cow::Owned(cff1));
    out_map.insert("hmtx".to_string(), Cow::Owned(hmtx_data));
    out_map.insert("OS/2".to_string(), Cow::Owned(os2_data));
    if let Some(vmtx) = vmtx_data { out_map.insert("vmtx".to_string(), Cow::Owned(vmtx)); }
    if let Some(vhea) = vhea_data { out_map.insert("vhea".to_string(), Cow::Owned(vhea)); }
    if !hhea_data.is_empty() { out_map.insert("hhea".to_string(), Cow::Owned(hhea_data)); }
    if !post_data.is_empty() { out_map.insert("post".to_string(), Cow::Owned(post_data)); }
    if let Some(head) = style.head { out_map.insert("head".to_string(), Cow::Owned(head)); }
    if let Some(name) = style.name { out_map.insert("name".to_string(), Cow::Owned(name)); }
    if let Some(stat) = style.stat { out_map.insert("STAT".to_string(), Cow::Owned(stat)); }

    Ok(out_map)
}

struct Resolved {
    chunks: Vec<Vec<u8>>,
    ends:   Vec<usize>,
}

const CHUNK_TARGET: usize = 128 << 10;

const CHUNK_CUT: usize = CHUNK_TARGET - (16 << 10);

fn resolve_all_charstrings(
    cff2:     &super::parse::Cff2Font<'_>,
    location: &[f64],
    n_glyphs: usize,
    budget:   &mut u32,
    scratch:  &mut super::charstring::Scratch,
) -> Result<(Vec<Vec<u8>>, Vec<usize>), String> {
    #[cfg(feature = "threading")]
    if let Some(ranges) = parallel_ranges(cff2, n_glyphs) {
        return resolve_in_parallel(cff2, location, &ranges, budget);
    }
    let r = resolve_charstrings(cff2, location, 0..n_glyphs, budget, scratch, 0)?;
    Ok((r.chunks, r.ends))
}

#[cfg(feature = "threading")]
const PARALLEL_FLOOR: usize = 512 << 10;

#[cfg(feature = "threading")]
fn parallel_ranges(cff2: &super::parse::Cff2Font<'_>, n_glyphs: usize) -> Option<Vec<core::ops::Range<usize>>> {
    let total: usize = cff2.charstrings.iter().map(|c| c.len()).sum();
    if total < PARALLEL_FLOOR || n_glyphs < 2 {
        return None;
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(n_glyphs);
    if threads < 2 {
        return None;
    }
    let mut ranges = Vec::with_capacity(threads);
    let (mut start, mut acc, mut cut) = (0usize, 0usize, 1usize);
    for gid in 0..n_glyphs {
        acc += cff2.charstrings[gid].len();
        if cut < threads && acc * threads >= cut * total {
            ranges.push(start..gid + 1);
            start = gid + 1;
            cut += 1;
        }
    }
    if start < n_glyphs {
        ranges.push(start..n_glyphs);
    }
    (ranges.len() > 1).then_some(ranges)
}

#[cfg(feature = "threading")]
fn resolve_in_parallel(
    cff2:     &super::parse::Cff2Font<'_>,
    location: &[f64],
    ranges:   &[core::ops::Range<usize>],
    budget:   &mut u32,
) -> Result<(Vec<Vec<u8>>, Vec<usize>), String> {
    let total: usize = cff2.charstrings.iter().map(|c| c.len()).sum::<usize>().max(1);
    let whole = *budget;

    let results: Vec<Result<(Resolved, u32), String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ranges
            .iter()
            .map(|range| {
                let range = range.clone();
                let bytes: usize = cff2.charstrings[range.clone()].iter().map(|c| c.len()).sum();
                let share = ((u64::from(whole) * bytes as u64) / total as u64) as u32;
                scope.spawn(move || {
                    let mut own = share;
                    let mut scratch = super::charstring::Scratch::default();
                    resolve_charstrings(cff2, location, range, &mut own, &mut scratch, 0)
                        .map(|r| (r, share - own))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(String::from("CFF2: charstring worker failed"))))
            .collect()
    });

    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut ends:   Vec<usize>   = Vec::with_capacity(cff2.charstrings.len());
    let mut base = 0usize;
    let mut spent = 0u32;
    for result in results {
        let (resolved, used) = result?;
        for end in &resolved.ends {
            ends.push(base + end);
        }
        base += resolved.chunks.iter().map(|t| t.len()).sum::<usize>();
        chunks.extend(resolved.chunks);
        spent = spent.saturating_add(used);
    }
    *budget -= spent.min(whole);
    Ok((chunks, ends))
}

fn resolve_charstrings(
    cff2:    &super::parse::Cff2Font<'_>,
    location: &[f64],
    range:    core::ops::Range<usize>,
    budget:   &mut u32,
    scratch:  &mut super::charstring::Scratch,
    base:     usize,
) -> Result<Resolved, String> {
    let mut ends: Vec<usize> = Vec::with_capacity(range.len());
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::with_capacity(CHUNK_TARGET);
    let mut done = base;
    for gid in range {
        if cur.len() >= CHUNK_CUT {
            done += cur.len();
            chunks.push(core::mem::replace(&mut cur, Vec::with_capacity(CHUNK_TARGET)));
        }
        let fd_idx = *cff2.fd_select.get(gid).unwrap_or(&0) as usize;
        let fd = cff2.fds.get(fd_idx)
            .ok_or("CFF2: FDSelect references an FD index out of range")?;
        super::charstring::evaluate_charstring_into(
            cff2.charstrings[gid],
            &cff2.global_subrs,
            &fd.local_subrs,
            fd.vsindex,
            cff2.vstore.as_ref(),
            location,
            budget,
            scratch,
            &mut cur,
        )?;
        cur.push(0x0e);
        ends.push(done + cur.len());
    }
    if cur.capacity() > cur.len().saturating_mul(2) {
        cur.shrink_to_fit();
    }
    chunks.push(cur);
    Ok(Resolved { chunks, ends })
}

fn instance_font_name(axis_values: &[(String, f64)]) -> Vec<u8> {
    let mut name = String::from("DaegunInstance");
    for (tag, v) in axis_values.iter().take(16) {
        name.push('-');
        name.extend(tag.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(8));
        name.push_str(&((v * 100.0).round() as i64).to_string());
    }
    name.truncate(100);
    name.into_bytes()
}

const FIRST_CUSTOM_SID: u16 = 391;

fn build_cff1(charstring_chunks: &[&[u8]], charstring_ends: &[usize], font_matrix_raw: Option<&Vec<u8>>, font_name: &[u8]) -> Vec<u8> {
    let hdr_size = 4usize;
    let n_glyphs = charstring_ends.len();
    let named = n_glyphs.saturating_sub(1);

    let mut string_data: Vec<u8> = Vec::with_capacity(named * 6);
    let mut string_ends: Vec<usize> = Vec::with_capacity(named);
    let mut digits = [0u8; 20];
    for i in 1..=named {
        string_data.push(b'g');
        let mut n = i;
        let mut d = 0;
        while {
            digits[d] = b'0' + (n % 10) as u8;
            n /= 10;
            d += 1;
            n != 0
        } {}
        string_data.extend(digits[..d].iter().rev());
        string_ends.push(string_data.len());
    }
    let charset = build_charset(named);

    let name_index_bytes   = encode_cff_index(&[font_name.to_vec()]);
    let string_index_bytes = encode_cff_index_flat(&string_data, &string_ends);
    let gsubr_index_bytes  = encode_cff_index(&[]);
    let charstrings_index_len = cff_index_flat_size(charstring_ends);

    let fm_len = font_matrix_raw.map_or(0, |m| m.len());
    let top_dict_data_len   = 23 + fm_len;
    let top_dict_index_size = cff_index_size(top_dict_data_len);

    let base = hdr_size + name_index_bytes.len() + top_dict_index_size
        + string_index_bytes.len() + gsubr_index_bytes.len();
    let new_charset_off     = base;
    let new_charstrings_off = new_charset_off + charset.len();
    let new_private_off     = new_charstrings_off + charstrings_index_len;

    let mut top_dict_data = Vec::with_capacity(top_dict_data_len);
    if let Some(fm) = font_matrix_raw {
        top_dict_data.extend_from_slice(fm);
    }
    top_dict_data.extend(encode_cff_int(new_charset_off as i32));
    top_dict_data.push(15);
    top_dict_data.extend(encode_cff_int(new_charstrings_off as i32));
    top_dict_data.push(17);
    top_dict_data.extend(encode_cff_int(0));
    top_dict_data.extend(encode_cff_int(new_private_off as i32));
    top_dict_data.push(18);

    let top_dict_index = encode_cff_index(&[top_dict_data]);
    debug_assert_eq!(top_dict_index.len(), top_dict_index_size);

    let mut out = Vec::with_capacity(new_private_off);
    out.extend_from_slice(&[1, 0, hdr_size as u8, 4]);
    out.extend_from_slice(&name_index_bytes);
    out.extend_from_slice(&top_dict_index);
    out.extend_from_slice(&string_index_bytes);
    out.extend_from_slice(&gsubr_index_bytes);
    out.extend_from_slice(&charset);
    append_cff_index_chunks(&mut out, charstring_chunks, charstring_ends);

    out
}

fn build_charset(named: usize) -> Vec<u8> {
    if named == 0 {
        return vec![0];
    }
    let mut out = vec![2u8];
    out.extend_from_slice(&FIRST_CUSTOM_SID.to_be_bytes());
    out.extend_from_slice(&u16::try_from(named - 1).unwrap_or(u16::MAX).to_be_bytes());
    out
}
