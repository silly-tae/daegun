use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::super::decoder::{read_u16_be, write_u16_be, records_fit};
use super::otl::remap_gid;

struct Blob(Vec<u8>);

impl Blob {
    fn new() -> Self { Blob(Vec::new()) }
    fn u16(&mut self, v: u16) { self.0.extend_from_slice(&v.to_be_bytes()); }
    fn tag(&mut self, t: &[u8]) { self.0.extend_from_slice(t); }
    fn slot(&mut self) -> usize { let at = self.0.len(); self.u16(0); at }
    fn place(&mut self, slot: usize, bytes: &[u8]) {
        let at = self.0.len();
        if let Ok(v) = u16::try_from(at) { write_u16_be(&mut self.0, slot, v); }
        self.0.extend_from_slice(bytes);
    }
}

fn mod_list(jstf: &[u8], off: usize) -> Option<Vec<u8>> {
    let count = read_u16_be(jstf, off)? as usize;
    if !records_fit(off + 2, count, 2, jstf.len()) { return None; }
    jstf.get(off..off + 2 + count * 2).map(<[u8]>::to_vec)
}

fn priority(jstf: &[u8], off: usize) -> Option<Vec<u8>> {
    const MAX_SLOTS: [usize; 2] = [4, 9];
    let mut blob = Blob::new();
    let slots: Vec<usize> = (0..10).map(|_| blob.slot()).collect();
    let mut kept = 0usize;
    let mut lists: Vec<(usize, Vec<u8>)> = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        if MAX_SLOTS.contains(&i) { continue; }
        let Some(rel) = read_u16_be(jstf, off + i * 2).filter(|&r| r != 0) else { continue };
        let Some(list) = mod_list(jstf, off + rel as usize) else { continue };
        kept += 1;
        lists.push((*slot, list));
    }
    if kept == 0 { return None; }
    for (slot, list) in lists { blob.place(slot, &list); }
    Some(blob.0)
}

fn lang_sys(jstf: &[u8], off: usize) -> Option<Vec<u8>> {
    let count = read_u16_be(jstf, off)? as usize;
    if !records_fit(off + 2, count, 2, jstf.len()) { return None; }
    let built: Vec<Vec<u8>> = (0..count)
        .filter_map(|i| read_u16_be(jstf, off + 2 + i * 2).filter(|&r| r != 0))
        .filter_map(|rel| priority(jstf, off + rel as usize))
        .collect();
    if built.is_empty() { return None; }

    let mut blob = Blob::new();
    blob.u16(built.len() as u16);
    let slots: Vec<usize> = (0..built.len()).map(|_| blob.slot()).collect();
    for (slot, bytes) in slots.into_iter().zip(&built) { blob.place(slot, bytes); }
    Some(blob.0)
}

fn script(jstf: &[u8], off: usize, active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let extenders: Vec<u16> = read_u16_be(jstf, off)
        .filter(|&r| r != 0)
        .map(|rel| off + rel as usize)
        .and_then(|at| {
            let count = read_u16_be(jstf, at)? as usize;
            if !records_fit(at + 2, count, 2, jstf.len()) { return None; }
            Some((0..count)
                .filter_map(|i| read_u16_be(jstf, at + 2 + i * 2))
                .filter_map(|g| remap_gid(active, gid_map, g))
                .collect())
        })
        .unwrap_or_default();

    let default = read_u16_be(jstf, off + 2).filter(|&r| r != 0)
        .and_then(|rel| lang_sys(jstf, off + rel as usize));

    let count = read_u16_be(jstf, off + 4)? as usize;
    let mut named: Vec<(&[u8], Vec<u8>)> = Vec::new();
    if records_fit(off + 6, count, 6, jstf.len()) {
        for i in 0..count {
            let rec = off + 6 + i * 6;
            let (Some(tag), Some(rel)) = (jstf.get(rec..rec + 4), read_u16_be(jstf, rec + 4).filter(|&r| r != 0))
            else { continue };
            if let Some(ls) = lang_sys(jstf, off + rel as usize) { named.push((tag, ls)); }
        }
    }

    if extenders.is_empty() && default.is_none() && named.is_empty() { return None; }

    let mut blob = Blob::new();
    let ext_slot = blob.slot();
    let def_slot = blob.slot();
    blob.u16(named.len() as u16);
    let named_slots: Vec<usize> = named.iter().map(|(tag, _)| { blob.tag(tag); blob.slot() }).collect();

    if !extenders.is_empty() {
        let mut ext = Blob::new();
        ext.u16(extenders.len() as u16);
        for g in &extenders { ext.u16(*g); }
        blob.place(ext_slot, &ext.0);
    }
    if let Some(d) = default { blob.place(def_slot, &d); }
    for (slot, (_, bytes)) in named_slots.into_iter().zip(&named) { blob.place(slot, bytes); }
    Some(blob.0)
}

pub fn subset_jstf(jstf: &[u8], active: &GlyphSet, gid_map: &[u16]) -> Option<Vec<u8>> {
    let count = read_u16_be(jstf, 4)? as usize;
    if !records_fit(6, count, 6, jstf.len()) { return None; }

    let mut kept: Vec<(&[u8], Vec<u8>)> = Vec::new();
    for i in 0..count {
        let rec = 6 + i * 6;
        let (Some(tag), Some(rel)) = (jstf.get(rec..rec + 4), read_u16_be(jstf, rec + 4).filter(|&r| r != 0))
        else { continue };
        if let Some(s) = script(jstf, rel as usize, active, gid_map) { kept.push((tag, s)); }
    }
    if kept.is_empty() { return None; }

    let mut blob = Blob::new();
    blob.u16(1);
    blob.u16(0);
    blob.u16(kept.len() as u16);
    let slots: Vec<usize> = kept.iter().map(|(tag, _)| { blob.tag(tag); blob.slot() }).collect();
    for (slot, (_, bytes)) in slots.into_iter().zip(&kept) { blob.place(slot, bytes); }
    Some(blob.0)
}
