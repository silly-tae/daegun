use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use alloc::vec;
use super::super::decoder::{read_u16_be, read_u32_be, write_u16_be, write_u32_be, records_fit};

struct Bitmap<'a> {
    new_gid: u16,
    data: &'a [u8],
}

enum Storage {
    Variable,
    Fixed { image_size: u32, metrics: Vec<u8> },
}

struct SubTable<'a> {
    image_format: u16,
    storage: Storage,
    glyphs: Vec<Bitmap<'a>>,
}

struct Built {
    bytes: Vec<u8>,
    first: u16,
    last: u16,
}

fn push_u16(v: &mut Vec<u8>, x: u16) { v.extend_from_slice(&x.to_be_bytes()); }
fn push_u32(v: &mut Vec<u8>, x: u32) { v.extend_from_slice(&x.to_be_bytes()); }

fn read_subtable<'a>(
    cblc: &[u8],
    cbdt: &'a [u8],
    ist: usize,
    first: u16,
    last: u16,
    active: &GlyphSet,
    gid_map: &[u16],
) -> Option<SubTable<'a>> {
    let index_format = read_u16_be(cblc, ist)?;
    let image_format = read_u16_be(cblc, ist + 2)?;
    let data_off = read_u32_be(cblc, ist + 4)? as usize;
    if last < first { return None; }
    let span = (last - first) as usize + 1;

    let mut glyphs = Vec::new();
    let mut take = |gid: u16, range: Option<(usize, usize)>| -> Option<()> {
        let (s, e) = range?;
        if e <= s { return Some(()); }
        let data = cbdt.get(data_off.checked_add(s)?..data_off.checked_add(e)?)?;
        glyphs.push(Bitmap { new_gid: *gid_map.get(gid as usize)?, data });
        Some(())
    };

    let storage = match index_format {
        1 | 3 => {
            let width = if index_format == 1 { 4 } else { 2 };
            if !records_fit(ist + 8, span + 1, width, cblc.len()) { return None; }
            let at = |i: usize| -> Option<usize> {
                if index_format == 1 { read_u32_be(cblc, ist + 8 + i * 4).map(|v| v as usize) }
                else { read_u16_be(cblc, ist + 8 + i * 2).map(|v| v as usize) }
            };
            for i in 0..span {
                let gid = first + i as u16;
                if !active.contains(&gid) { continue; }
                take(gid, Some((at(i)?, at(i + 1)?)))?;
            }
            Storage::Variable
        }
        2 => {
            let image_size = read_u32_be(cblc, ist + 8)? as usize;
            let metrics = cblc.get(ist + 12..ist + 20)?.to_vec();
            for i in 0..span {
                let gid = first + i as u16;
                if !active.contains(&gid) { continue; }
                let s = i.checked_mul(image_size)?;
                take(gid, Some((s, s.checked_add(image_size)?)))?;
            }
            Storage::Fixed { image_size: image_size as u32, metrics }
        }
        4 => {
            let n = read_u32_be(cblc, ist + 8)? as usize;
            if !records_fit(ist + 12, n + 1, 4, cblc.len()) { return None; }
            for i in 0..n {
                let gid = read_u16_be(cblc, ist + 12 + i * 4)?;
                if !active.contains(&gid) { continue; }
                let s = read_u16_be(cblc, ist + 12 + i * 4 + 2)? as usize;
                let e = read_u16_be(cblc, ist + 12 + (i + 1) * 4 + 2)? as usize;
                take(gid, Some((s, e)))?;
            }
            Storage::Variable
        }
        5 => {
            let image_size = read_u32_be(cblc, ist + 8)? as usize;
            let metrics = cblc.get(ist + 12..ist + 20)?.to_vec();
            let n = read_u32_be(cblc, ist + 20)? as usize;
            if !records_fit(ist + 24, n, 2, cblc.len()) { return None; }
            for i in 0..n {
                let gid = read_u16_be(cblc, ist + 24 + i * 2)?;
                if !active.contains(&gid) { continue; }
                let s = i.checked_mul(image_size)?;
                take(gid, Some((s, s.checked_add(image_size)?)))?;
            }
            Storage::Fixed { image_size: image_size as u32, metrics }
        }
        _ => return None,
    };
    Some(SubTable { image_format, storage, glyphs })
}

fn build_subtable(sub: &SubTable, run: &[&Bitmap], cbdt: &mut Vec<u8>) -> Built {
    let data_off = cbdt.len() as u32;
    let mut bytes = Vec::new();
    match &sub.storage {
        Storage::Variable => {
            push_u16(&mut bytes, 1);
            push_u16(&mut bytes, sub.image_format);
            push_u32(&mut bytes, data_off);
            let mut running = 0u32;
            for b in run {
                push_u32(&mut bytes, running);
                running += b.data.len() as u32;
                cbdt.extend_from_slice(b.data);
            }
            push_u32(&mut bytes, running);
        }
        Storage::Fixed { image_size, metrics } => {
            push_u16(&mut bytes, 5);
            push_u16(&mut bytes, sub.image_format);
            push_u32(&mut bytes, data_off);
            push_u32(&mut bytes, *image_size);
            bytes.extend_from_slice(metrics);
            push_u32(&mut bytes, run.len() as u32);
            for b in run {
                push_u16(&mut bytes, b.new_gid);
                cbdt.extend_from_slice(b.data);
            }
            if run.len() % 2 == 1 { bytes.extend_from_slice(&[0, 0]); }
        }
    }
    Built { bytes, first: run[0].new_gid, last: run[run.len() - 1].new_gid }
}

struct Strike {
    record: Vec<u8>,
    blob: Vec<u8>,
    subtables: usize,
    lo: u16,
    hi: u16,
}

pub fn subset_bitmap_strikes(
    cblc: &[u8],
    cbdt: &[u8],
    active: &GlyphSet,
    gid_map: &[u16],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let num_sizes = read_u32_be(cblc, 4)? as usize;
    if !records_fit(8, num_sizes, 48, cblc.len()) { return None; }

    let mut new_cbdt: Vec<u8> = cbdt.get(0..4)?.to_vec();
    let mut strikes: Vec<Strike> = Vec::new();

    for i in 0..num_sizes {
        let st = 8 + i * 48;
        let (Some(ist_array_off), Some(n_ist)) =
            (read_u32_be(cblc, st).map(|v| v as usize), read_u32_be(cblc, st + 8).map(|v| v as usize))
        else { continue };
        if !records_fit(ist_array_off, n_ist, 8, cblc.len()) { continue; }

        let mut built: Vec<Built> = Vec::new();
        for j in 0..n_ist {
            let rec = ist_array_off + j * 8;
            let (Some(first), Some(last), Some(add)) = (
                read_u16_be(cblc, rec), read_u16_be(cblc, rec + 2), read_u32_be(cblc, rec + 4),
            ) else { continue };
            let Some(ist) = ist_array_off.checked_add(add as usize) else { continue };
            let Some(sub) = read_subtable(cblc, cbdt, ist, first, last, active, gid_map) else { continue };
            if sub.glyphs.is_empty() { continue; }

            let mut sorted: Vec<&Bitmap> = sub.glyphs.iter().collect();
            sorted.sort_unstable_by_key(|b| b.new_gid);
            let mut run: Vec<&Bitmap> = Vec::new();
            for b in sorted {
                if run.last().is_some_and(|p| b.new_gid != p.new_gid + 1) {
                    built.push(build_subtable(&sub, &run, &mut new_cbdt));
                    run.clear();
                }
                run.push(b);
            }
            if !run.is_empty() { built.push(build_subtable(&sub, &run, &mut new_cbdt)); }
        }

        if built.is_empty() { continue; }
        let lo = built.iter().map(|b| b.first).min()?;
        let hi = built.iter().map(|b| b.last).max()?;

        let mut blob = vec![0u8; built.len() * 8];
        for (j, b) in built.iter().enumerate() {
            let at = blob.len() as u32;
            write_u16_be(&mut blob, j * 8, b.first);
            write_u16_be(&mut blob, j * 8 + 2, b.last);
            write_u32_be(&mut blob, j * 8 + 4, at);
            blob.extend_from_slice(&b.bytes);
        }
        strikes.push(Strike {
            record: cblc.get(st..st + 48)?.to_vec(),
            blob,
            subtables: built.len(),
            lo,
            hi,
        });
    }

    if strikes.is_empty() { return None; }

    let mut new_cblc = vec![0u8; 8 + strikes.len() * 48];
    new_cblc[0..4].copy_from_slice(cblc.get(0..4)?);
    write_u32_be(&mut new_cblc, 4, strikes.len() as u32);
    for (i, Strike { record, blob, subtables, lo, hi }) in strikes.iter().enumerate() {
        let at = 8 + i * 48;
        new_cblc[at..at + 48].copy_from_slice(record);
        let blob_at = new_cblc.len() as u32;
        write_u32_be(&mut new_cblc, at, blob_at);
        write_u32_be(&mut new_cblc, at + 4, blob.len() as u32);
        write_u32_be(&mut new_cblc, at + 8, *subtables as u32);
        write_u16_be(&mut new_cblc, at + 40, *lo);
        write_u16_be(&mut new_cblc, at + 42, *hi);
        new_cblc.extend_from_slice(blob);
    }

    Some((new_cblc, new_cbdt))
}

pub fn subset_sbix(
    sbix: &[u8],
    num_glyphs: usize,
    active_sorted: &[u16],
    gid_map: &[u16],
) -> Option<Vec<u8>> {
    let num_strikes = read_u32_be(sbix, 4)? as usize;
    if !records_fit(8, num_strikes, 4, sbix.len()) { return None; }
    let active: GlyphSet = active_sorted.iter().copied().collect();

    let mut strikes: Vec<Vec<u8>> = Vec::new();
    for i in 0..num_strikes {
        let strike = read_u32_be(sbix, 8 + i * 4)? as usize;
        if !records_fit(strike + 4, num_glyphs + 1, 4, sbix.len()) { continue; }
        let off = |g: usize| read_u32_be(sbix, strike + 4 + g * 4).map(|v| v as usize);

        let mut out = sbix.get(strike..strike + 4)?.to_vec();
        let data_at = 4 + (active_sorted.len() + 1) * 4;
        let mut offsets: Vec<u32> = Vec::with_capacity(active_sorted.len() + 1);
        let mut data: Vec<u8> = Vec::new();
        let mut kept = 0usize;

        for &orig in active_sorted {
            offsets.push((data_at + data.len()) as u32);
            let (Some(s), Some(e)) = (off(orig as usize), off(orig as usize + 1)) else { continue };
            if e <= s { continue; }
            let Some(record) = sbix.get(strike + s..strike + e) else { continue };
            match record.get(4..8) {
                Some(b"dupe") => {
                    let Some(target) = read_u16_be(record, 8) else { continue };
                    if !active.contains(&target) { continue; }
                    let Some(&new_target) = gid_map.get(target as usize) else { continue };
                    data.extend_from_slice(record.get(0..8)?);
                    data.extend_from_slice(&new_target.to_be_bytes());
                }
                _ => data.extend_from_slice(record),
            }
            kept += 1;
        }
        offsets.push((data_at + data.len()) as u32);
        if kept == 0 { continue; }

        for o in &offsets { out.extend_from_slice(&o.to_be_bytes()); }
        out.extend_from_slice(&data);
        strikes.push(out);
    }
    if strikes.is_empty() { return None; }

    let mut out = sbix.get(0..4)?.to_vec();
    push_u32(&mut out, strikes.len() as u32);
    let mut at = 8 + strikes.len() * 4;
    for st in &strikes {
        push_u32(&mut out, at as u32);
        at += st.len();
    }
    for st in &strikes { out.extend_from_slice(st); }
    Some(out)
}
