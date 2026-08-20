use alloc::vec::Vec;
use alloc::vec;
use super::super::super::decoder::write_u16_be;

fn bin_srch(out: &mut Vec<u8>, unit_size: usize, n_units: usize) {
    let selector = usize::BITS - 1 - n_units.max(1).leading_zeros();
    let search_range = unit_size * (1usize << selector);
    for v in [unit_size, n_units, search_range, selector as usize, unit_size * n_units - search_range] {
        out.extend_from_slice(&(v as u16).to_be_bytes());
    }
}

pub(crate) fn build_aat_lookup(entries: &[(u16, u16)]) -> Option<Vec<u8>> {
    if entries.is_empty() { return None; }
    let mut sorted = entries.to_vec();
    sorted.sort_unstable_by_key(|(g, _)| *g);
    sorted.dedup_by_key(|(g, _)| *g);

    let mut out = vec![0u8; 2];
    write_u16_be(&mut out, 0, 6);
    bin_srch(&mut out, 4, sorted.len());
    for (g, v) in sorted {
        out.extend_from_slice(&g.to_be_bytes());
        out.extend_from_slice(&v.to_be_bytes());
    }
    Some(out)
}
