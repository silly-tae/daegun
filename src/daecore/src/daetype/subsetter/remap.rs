use crate::daecore::daetype::subsetter::GlyphSet;
use super::*;

pub fn fix_post_table(mut post: Vec<u8>) -> Vec<u8> {
    if post.len() < 32 { return post; }
    post.truncate(32);
    write_u32_be(&mut post, 0, 0x0003_0000);
    post
}

pub fn remap_vorg(vorg: &[u8], gid_map: &[u16], active: &GlyphSet) -> Option<Vec<u8>> {
    if vorg.len() < 8 { return None; }
    let major = read_u16_be(vorg, 0)?;
    let minor = read_u16_be(vorg, 2)?;
    let default = read_i16_be(vorg, 4)?;
    let count = read_u16_be(vorg, 6)? as usize;

    let mut kept: Vec<(u16, i16)> = Vec::with_capacity(count.min(active.len()));
    for i in 0..count {
        let rec = 8 + i * 4;
        let orig_gid = read_u16_be(vorg, rec)?;
        if !active.contains(&orig_gid) { continue; }
        let y = read_i16_be(vorg, rec + 2)?;
        let compact_gid = *gid_map.get(orig_gid as usize)?;
        kept.push((compact_gid, y));
    }

    let mut out = vec![0u8; 8 + kept.len() * 4];
    write_u16_be(&mut out, 0, major);
    write_u16_be(&mut out, 2, minor);
    write_i16_be(&mut out, 4, default);
    write_u16_be(&mut out, 6, kept.len() as u16);
    for (i, &(gid, y)) in kept.iter().enumerate() {
        write_u16_be(&mut out, 8 + i * 4, gid);
        write_i16_be(&mut out, 8 + i * 4 + 2, y);
    }
    Some(out)
}

pub fn remap_kern(kern: &[u8], gid_map: &[u16], active: &GlyphSet) -> Option<Vec<u8>> {
    let apple = read_u16_be(kern, 0)? == 1 && read_u16_be(kern, 2)? == 0;
    let (n_tables, mut at) = if apple {
        (read_u32_be(kern, 4)? as usize, 8)
    } else {
        if read_u16_be(kern, 0)? != 0 { return None; }
        (read_u16_be(kern, 2)? as usize, 4)
    };

    let mut subtables: Vec<Vec<u8>> = Vec::new();
    for _ in 0..n_tables {
        let (length, coverage, body) = if apple {
            (read_u32_be(kern, at)? as usize, read_u16_be(kern, at + 4)?, at + 8)
        } else {
            (read_u16_be(kern, at + 2)? as usize, read_u16_be(kern, at + 4)?, at + 6)
        };
        if length < body - at || at.checked_add(length)? > kern.len() {
            return None;
        }
        let format = if apple { coverage & 0x00FF } else { coverage >> 8 };
        let variation = apple && coverage & 0x2000 != 0;

        if format == 0 && !variation
            && let Some(rebuilt) = remap_kern_format0(kern, body, coverage, apple, gid_map, active) {
                subtables.push(rebuilt);
            }
        at += length;
    }

    if subtables.is_empty() {
        return None;
    }

    let mut out = Vec::new();
    if apple {
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(subtables.len()).ok()?.to_be_bytes());
    } else {
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&u16::try_from(subtables.len()).ok()?.to_be_bytes());
    }
    for sub in &subtables {
        out.extend_from_slice(sub);
    }
    Some(out)
}

fn remap_kern_format0(
    kern: &[u8],
    body: usize,
    coverage: u16,
    apple: bool,
    gid_map: &[u16],
    active: &GlyphSet,
) -> Option<Vec<u8>> {
    let n_pairs = read_u16_be(kern, body)? as usize;

    let mut kept: Vec<(u16, u16, i16)> = Vec::with_capacity(n_pairs.min(active.len()));
    for i in 0..n_pairs {
        let rec = body + 8 + i * 6;
        let left = read_u16_be(kern, rec)?;
        let right = read_u16_be(kern, rec + 2)?;
        if !active.contains(&left) || !active.contains(&right) {
            continue;
        }
        let value = read_i16_be(kern, rec + 4)?;
        kept.push((*gid_map.get(left as usize)?, *gid_map.get(right as usize)?, value));
    }
    if kept.is_empty() {
        return None;
    }
    debug_assert!(
        kept.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)),
        "kern format 0 must stay sorted by (left, right) for the binary search to work"
    );

    let header_len = if apple { 8 } else { 6 };
    let length = header_len + 8 + kept.len() * 6;
    let mut out = vec![0u8; header_len + 8];
    if apple {
        write_u32_be(&mut out, 0, u32::try_from(length).ok()?);
        write_u16_be(&mut out, 4, coverage);
        write_u16_be(&mut out, 6, 0);
    } else {
        write_u16_be(&mut out, 0, 0);
        write_u16_be(&mut out, 2, u16::try_from(length).ok()?);
        write_u16_be(&mut out, 4, coverage);
    }

    let pairs = u16::try_from(kept.len()).ok()?;
    let entry_selector = u32::from(pairs).ilog2();
    let search_range = ((1u32 << entry_selector) * 6) & 0xFFFF;
    let range_shift = (u32::from(pairs) * 6).wrapping_sub(search_range) & 0xFFFF;
    write_u16_be(&mut out, header_len, pairs);
    write_u16_be(&mut out, header_len + 2, u16::try_from(search_range).ok()?);
    write_u16_be(&mut out, header_len + 4, u16::try_from(entry_selector).ok()?);
    write_u16_be(&mut out, header_len + 6, u16::try_from(range_shift).ok()?);

    for &(left, right, value) in &kept {
        let mut rec = [0u8; 6];
        write_u16_be(&mut rec, 0, left);
        write_u16_be(&mut rec, 2, right);
        write_i16_be(&mut rec, 4, value);
        out.extend_from_slice(&rec);
    }
    debug_assert_eq!(out.len(), length);
    Some(out)
}

pub fn metric_pair(mtx: &[u8], num_long: usize, last_advance: u16, gid: usize) -> (u16, i16) {
    if gid < num_long {
        let off = gid * 4;
        let adv = if off + 2 <= mtx.len() { read_u16_be(mtx, off).unwrap_or(0) } else { 0 };
        let bearing = if off + 4 <= mtx.len() { read_i16_be(mtx, off + 2).unwrap_or(0) } else { 0 };
        (adv, bearing)
    } else {
        let off = num_long * 4 + (gid - num_long) * 2;
        let bearing = if off + 2 <= mtx.len() { read_i16_be(mtx, off).unwrap_or(0) } else { 0 };
        (last_advance, bearing)
    }
}

pub fn rebuild_metrics(mtx: &[u8], num_long: usize, active_sorted: &[u16]) -> Vec<u8> {
    let last_advance = if num_long > 0 && num_long * 4 <= mtx.len() {
        read_u16_be(mtx, (num_long - 1) * 4).unwrap_or(0)
    } else { 0 };

    let mut out = vec![0u8; active_sorted.len() * 4];
    for (compact, &orig_gid) in active_sorted.iter().enumerate() {
        let (adv, bearing) = metric_pair(mtx, num_long, last_advance, orig_gid as usize);
        write_u16_be(&mut out, compact * 4, adv);
        write_i16_be(&mut out, compact * 4 + 2, bearing);
    }
    out
}
