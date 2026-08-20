use alloc::vec::Vec;
use crate::daecore::daetype::decoder::write_u16_be;

pub(crate) fn cff_index_size(data_len: usize) -> usize {
    let max_off  = data_len + 1;
    let off_size = if max_off <= 0xFF { 1 } else if max_off <= 0xFFFF { 2 } else if max_off <= 0xFF_FFFF { 3 } else { 4 };
    2 + 1 + 2 * off_size + data_len
}

pub fn encode_cff_index(objects: &[Vec<u8>]) -> Vec<u8> {
    let refs: Vec<&[u8]> = objects.iter().map(|t| t.as_slice()).collect();
    encode_cff_index_refs(&refs)
}

pub(crate) fn encode_cff_index_refs(objects: &[&[u8]]) -> Vec<u8> {
    if objects.is_empty() {
        return vec![0, 0];
    }

    let mut offsets = vec![1usize];
    #[allow(clippy::unwrap_used, reason = "offsets is seeded non-empty and never shrinks")]
    for obj in objects {
        offsets.push(offsets.last().unwrap() + obj.len());
    }
    #[allow(clippy::unwrap_used, reason = "offsets is seeded non-empty and never shrinks")]
    let max_off  = *offsets.last().unwrap();
    let off_size = if max_off <= 0xFF        { 1usize }
        else if max_off <= 0xFFFF            { 2 }
        else if max_off <= 0xFFFFFF          { 3 }
        else                                 { 4 };

    let header_size: usize = 2 + 1 + (objects.len() + 1) * off_size;
    let data_size:   usize = objects.iter().map(|o| o.len()).sum();
    let mut out = vec![0u8; header_size + data_size];

    write_u16_be(&mut out, 0, objects.len() as u16);
    out[2] = off_size as u8;

    for (i, &o) in offsets.iter().enumerate() {
        let pos = 3 + i * off_size;
        for j in 0..off_size {
            out[pos + off_size - 1 - j] = ((o >> (j * 8)) & 0xFF) as u8;
        }
    }

    let mut data_pos = header_size;
    for obj in objects {
        out[data_pos..data_pos + obj.len()].copy_from_slice(obj);
        data_pos += obj.len();
    }

    out
}

pub(crate) fn encode_cff_index_flat(data: &[u8], ends: &[usize]) -> Vec<u8> {
    let mut out = Vec::with_capacity(cff_index_flat_size(ends));
    append_cff_index_chunks(&mut out, core::slice::from_ref(&data), ends);
    out
}

pub(crate) fn cff_index_flat_size(ends: &[usize]) -> usize {
    if ends.is_empty() {
        return 2;
    }
    let data_len = ends[ends.len() - 1];
    2 + 1 + (ends.len() + 1) * flat_off_size(ends) + data_len
}

fn flat_off_size(ends: &[usize]) -> usize {
    let max_off = ends[ends.len() - 1] + 1;
    if max_off <= 0xFF { 1 } else if max_off <= 0xFFFF { 2 } else if max_off <= 0xFF_FFFF { 3 } else { 4 }
}

pub(crate) fn append_cff_index_chunks(out: &mut Vec<u8>, chunks: &[&[u8]], ends: &[usize]) {
    if ends.is_empty() {
        out.extend_from_slice(&[0, 0]);
        return;
    }
    let off_size = flat_off_size(ends);
    let start = out.len();
    let header_size = 2 + 1 + (ends.len() + 1) * off_size;
    out.resize(start + header_size, 0);

    write_u16_be(out, start, ends.len() as u16);
    out[start + 2] = off_size as u8;

    let write_off = |out: &mut Vec<u8>, i: usize, o: usize| {
        let pos = start + 3 + i * off_size;
        for j in 0..off_size {
            out[pos + off_size - 1 - j] = ((o >> (j * 8)) & 0xFF) as u8;
        }
    };
    write_off(out, 0, 1);
    for (i, &end) in ends.iter().enumerate() {
        write_off(out, i + 1, end + 1);
    }

    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    debug_assert_eq!(out.len() - start, cff_index_flat_size(ends));
}

pub fn encode_cff_int(n: i32) -> Vec<u8> {
    vec![29, (n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8]
}
