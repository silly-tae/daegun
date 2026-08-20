pub mod anchor;
pub mod context;
pub mod gdef;
pub mod generic;
pub mod gpos;
pub mod gsub;
pub mod lookup_list;

use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::string::String;
use alloc::vec::Vec;
use super::super::decoder::{read_u16_be, write_u16_be};

pub(crate) fn remap_gid(active: &GlyphSet, gid_map: &[u16], orig: u16) -> Option<u16> {
    if !active.contains(&orig) { return None; }
    gid_map.get(orig as usize).copied()
}

pub fn parse_coverage(buf: &[u8], off: usize) -> Result<Vec<u16>, String> {
    let format = read_u16_be(buf, off).ok_or("Coverage: truncated")?;
    match format {
        1 => {
            let count = read_u16_be(buf, off + 2).ok_or("Coverage format 1: truncated")? as usize;
            let mut gids = Vec::with_capacity(count);
            for i in 0..count {
                gids.push(read_u16_be(buf, off + 4 + i * 2).ok_or("Coverage format 1: glyph array truncated")?);
            }
            Ok(gids)
        }
        2 => {
            let range_count = read_u16_be(buf, off + 2).ok_or("Coverage format 2: truncated")? as usize;
            let mut gids = Vec::new();
            let mut prev_end: Option<u16> = None;
            for i in 0..range_count {
                let r = off + 4 + i * 6;
                let start = read_u16_be(buf, r).ok_or("Coverage format 2: range truncated")?;
                let end   = read_u16_be(buf, r + 2).ok_or("Coverage format 2: range truncated")?;
                if end < start { return Err("Coverage format 2: range end before start".into()); }
                if let Some(pe) = prev_end && start <= pe { return Err("Coverage format 2: ranges not ascending/non-overlapping".into()); }
                prev_end = Some(end);
                gids.extend(start..=end);
            }
            Ok(gids)
        }
        _ => Err(format!("Coverage: unknown format {}", format)),
    }
}

fn to_ranges(sorted: &[u16]) -> impl Iterator<Item = (u16, u16)> + '_ {
    let mut i = 0;
    core::iter::from_fn(move || {
        if i >= sorted.len() { return None; }
        let start = sorted[i];
        let mut end = start;
        let mut j = i + 1;
        while j < sorted.len() && sorted[j] == end + 1 { end = sorted[j]; j += 1; }
        i = j;
        Some((start, end))
    })
}

pub fn build_coverage(gids: &[u16]) -> Vec<u8> {
    let mut sorted: Vec<u16> = gids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let range_count = to_ranges(&sorted).count();
    let format1_size = 4 + 2 * sorted.len();
    let format2_size = 4 + 6 * range_count;

    if format1_size <= format2_size {
        let mut buf = vec![0u8; format1_size];
        write_u16_be(&mut buf, 0, 1);
        write_u16_be(&mut buf, 2, sorted.len() as u16);
        for (i, &g) in sorted.iter().enumerate() { write_u16_be(&mut buf, 4 + i * 2, g); }
        buf
    } else {
        let mut buf = vec![0u8; format2_size];
        write_u16_be(&mut buf, 0, 2);
        write_u16_be(&mut buf, 2, range_count as u16);
        let mut idx = 0u32;
        for (i, (start, end)) in to_ranges(&sorted).enumerate() {
            let r = 4 + i * 6;
            write_u16_be(&mut buf, r, start);
            write_u16_be(&mut buf, r + 2, end);
            write_u16_be(&mut buf, r + 4, idx as u16);
            idx += u32::from(end) - u32::from(start) + 1;
        }
        buf
    }
}

pub fn parse_classdef(buf: &[u8], off: usize) -> Result<Vec<(u16, u16)>, String> {
    let format = read_u16_be(buf, off).ok_or("ClassDef: truncated")?;
    match format {
        1 => {
            let start = read_u16_be(buf, off + 2).ok_or("ClassDef format 1: truncated")?;
            let count = read_u16_be(buf, off + 4).ok_or("ClassDef format 1: truncated")? as usize;
            let mut entries = Vec::with_capacity(count);
            for i in 0..count {
                let class = read_u16_be(buf, off + 6 + i * 2).ok_or("ClassDef format 1: array truncated")?;
                if class != 0 { entries.push((start.wrapping_add(i as u16), class)); }
            }
            Ok(entries)
        }
        2 => {
            let range_count = read_u16_be(buf, off + 2).ok_or("ClassDef format 2: truncated")? as usize;
            let mut entries = Vec::new();
            let mut prev_end: Option<u16> = None;
            for i in 0..range_count {
                let r = off + 4 + i * 6;
                let start = read_u16_be(buf, r).ok_or("ClassDef format 2: range truncated")?;
                let end   = read_u16_be(buf, r + 2).ok_or("ClassDef format 2: range truncated")?;
                let class = read_u16_be(buf, r + 4).ok_or("ClassDef format 2: range truncated")?;
                if end < start { return Err("ClassDef format 2: range end before start".into()); }
                if let Some(pe) = prev_end && start <= pe { return Err("ClassDef format 2: ranges not ascending/non-overlapping".into()); }
                prev_end = Some(end);
                if class != 0 { entries.extend((start..=end).map(|g| (g, class))); }
            }
            Ok(entries)
        }
        _ => Err(format!("ClassDef: unknown format {}", format)),
    }
}

fn to_class_ranges(sorted: &[(u16, u16)]) -> impl Iterator<Item = (u16, u16, u16)> + '_ {
    let mut i = 0;
    core::iter::from_fn(move || {
        if i >= sorted.len() { return None; }
        let (start, class) = sorted[i];
        let mut end = start;
        let mut j = i + 1;
        while j < sorted.len() && sorted[j].0 == end + 1 && sorted[j].1 == class { end = sorted[j].0; j += 1; }
        i = j;
        Some((start, end, class))
    })
}

pub fn build_classdef(entries: &[(u16, u16)]) -> Vec<u8> {
    let mut sorted: Vec<(u16, u16)> = entries.iter().copied().filter(|&(_, c)| c != 0).collect();
    sorted.sort_unstable_by_key(|&(g, _)| g);
    sorted.dedup_by_key(|pair| pair.0);

    if sorted.is_empty() {
        let mut buf = vec![0u8; 4];
        write_u16_be(&mut buf, 0, 2);
        write_u16_be(&mut buf, 2, 0);
        return buf;
    }

    let first = sorted[0].0;
    let last  = sorted[sorted.len() - 1].0;
    let format1_count = (last - first + 1) as usize;
    let format1_size  = 6 + 2 * format1_count;

    let range_count = to_class_ranges(&sorted).count();
    let format2_size = 4 + 6 * range_count;

    if format1_size <= format2_size {
        let mut buf = vec![0u8; format1_size];
        write_u16_be(&mut buf, 0, 1);
        write_u16_be(&mut buf, 2, first);
        write_u16_be(&mut buf, 4, format1_count as u16);
        for &(g, c) in &sorted {
            write_u16_be(&mut buf, 6 + (g - first) as usize * 2, c);
        }
        buf
    } else {
        let mut buf = vec![0u8; format2_size];
        write_u16_be(&mut buf, 0, 2);
        write_u16_be(&mut buf, 2, range_count as u16);
        for (i, (start, end, class)) in to_class_ranges(&sorted).enumerate() {
            let r = 4 + i * 6;
            write_u16_be(&mut buf, r, start);
            write_u16_be(&mut buf, r + 2, end);
            write_u16_be(&mut buf, r + 4, class);
        }
        buf
    }
}

fn langsys_extent(buf: &[u8], off: usize) -> Option<usize> {
    let feature_count = read_u16_be(buf, off + 4)? as usize;
    Some(off + 6 + feature_count * 2)
}

fn script_list_extent(buf: &[u8], off: usize, steps_left: &mut usize) -> Option<usize> {
    let count = read_u16_be(buf, off)? as usize;
    let mut max_reach = off + 2 + count * 6;
    for i in 0..count {
        let rec = off + 2 + i * 6;
        let s_rel = read_u16_be(buf, rec + 4)? as usize;
        let s_off = off + s_rel;
        let default_ls_rel = read_u16_be(buf, s_off)? as usize;
        let langsys_count = read_u16_be(buf, s_off + 2)? as usize;
        max_reach = max_reach.max(s_off + 4 + langsys_count * 6);
        if default_ls_rel != 0 {
            max_reach = max_reach.max(langsys_extent(buf, s_off + default_ls_rel)?);
        }
        *steps_left = steps_left.checked_sub(langsys_count)?;
        for j in 0..langsys_count {
            let lrec = s_off + 4 + j * 6;
            let ls_rel = read_u16_be(buf, lrec + 4)? as usize;
            max_reach = max_reach.max(langsys_extent(buf, s_off + ls_rel)?);
        }
    }
    Some(max_reach)
}

fn feature_params_extent(tag_bytes: &[u8], buf: &[u8], off: usize) -> Option<usize> {
    let tag = core::str::from_utf8(tag_bytes).ok()?;
    if tag == "size" { return Some(off + 10); }
    if tag.len() == 4 {
        let (prefix, suffix) = tag.split_at(2);
        let is_two_digits = suffix.len() == 2 && suffix.bytes().all(|b| b.is_ascii_digit());
        if is_two_digits {
            if prefix == "ss" { return Some(off + 4); }
            if prefix == "cv" {
                let char_count = read_u16_be(buf, off + 12)? as usize;
                return Some(off + 14 + char_count * 3);
            }
        }
    }
    None
}

pub(crate) fn feature_params_extent_at(buf: &[u8], record: usize, at: usize) -> Option<usize> {
    let tag_bytes = buf.get(record..record + 4)?;
    feature_params_extent(tag_bytes, buf, at)
}

fn feature_list_extent(buf: &[u8], off: usize) -> Option<usize> {
    let count = read_u16_be(buf, off)? as usize;
    let mut max_reach = off + 2 + count * 6;
    for i in 0..count {
        let rec = off + 2 + i * 6;
        let tag_bytes = buf.get(rec..rec + 4)?;
        let f_rel = read_u16_be(buf, rec + 4)? as usize;
        let f_off = off + f_rel;
        let params_rel = read_u16_be(buf, f_off)?;
        if params_rel != 0 {
            let params_extent = feature_params_extent(tag_bytes, buf, f_off + params_rel as usize)?;
            max_reach = max_reach.max(params_extent);
        }
        let lookup_idx_count = read_u16_be(buf, f_off + 2)? as usize;
        max_reach = max_reach.max(f_off + 4 + lookup_idx_count * 2);
    }
    Some(max_reach)
}

pub fn layout_live_prefix_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 10 { return None; }
    let minor = read_u16_be(buf, 2)?;
    if minor != 0 { return None; }
    let script_off = read_u16_be(buf, 4)? as usize;
    let feature_off = read_u16_be(buf, 6)? as usize;
    let mut steps_left = buf.len() / 6;
    let script_extent = script_list_extent(buf, script_off, &mut steps_left)?;
    let feature_extent = feature_list_extent(buf, feature_off)?;
    Some(script_extent.max(feature_extent).max(10))
}

pub(crate) fn copy_device_table(buf: &[u8], at: usize) -> Option<Vec<u8>> {
    let delta_format = read_u16_be(buf, at + 4)?;
    let len = match delta_format {
        0x8000 => 6,
        1..=3 => {
            let start = read_u16_be(buf, at)? as usize;
            let end = read_u16_be(buf, at + 2)? as usize;
            let count = end.checked_sub(start)?.checked_add(1)?;
            let per_word = 16 >> delta_format;
            6 + count.div_ceil(per_word) * 2
        }
        _ => return None,
    };
    buf.get(at..at + len).map(<[u8]>::to_vec)
}
