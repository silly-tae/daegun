use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::vec::Vec;
use super::generic::schema::Schema;
use crate::daecore::daetype::decoder::{read_u16_be, read_u32_be, write_u16_be, write_u32_be};

const EXTENSION_RECORD_LEN: usize = 8;

type SubtableSubsetter<'a> = &'a dyn Fn(u16, Option<&Schema>, &[u8], usize, &GlyphSet, &[u16]) -> Option<Vec<u8>>;
type SchemaForType<'a> = &'a dyn Fn(u16) -> Option<Schema>;

pub(crate) fn resolve_effective_type(buf: &[u8], ext_type: u16, lookup_type: u16, sub_off: usize) -> Option<(u16, usize)> {
    if lookup_type != ext_type { return Some((lookup_type, sub_off)); }
    let ext_format = read_u16_be(buf, sub_off)?;
    if ext_format != 1 { return None; }
    let real_type = read_u16_be(buf, sub_off + 2)?;
    if real_type == ext_type { return None; }
    let real_off = read_u32_be(buf, sub_off + 4)? as usize;
    Some((real_type, sub_off + real_off))
}

struct RebuiltLookup {
    real_type: u16,
    flag: u16,
    mark_filtering_set: Option<u16>,
    subtables: Vec<Vec<u8>>,
}

impl RebuiltLookup {
    fn hollow(lookup_type: u16) -> Self {
        RebuiltLookup { real_type: lookup_type, flag: 0, mark_filtering_set: None, subtables: Vec::new() }
    }

    fn is_hollow(&self) -> bool { self.subtables.is_empty() }

    fn trailing(&self) -> usize { if self.flag & 0x0010 != 0 { 2 } else { 0 } }

    fn header_len(&self) -> usize { 6 + self.subtables.len() * 2 + self.trailing() }

    fn inline_len(&self) -> usize {
        if self.is_hollow() { return 6; }
        self.header_len() + self.subtables.iter().map(|t| t.len()).sum::<usize>()
    }

    fn promoted_header_len(&self) -> usize {
        if self.is_hollow() { return 6; }
        self.header_len() + self.subtables.len() * EXTENSION_RECORD_LEN
    }

    fn payload_len(&self) -> usize { self.subtables.iter().map(|t| t.len()).sum() }

    fn write_header_into(&self, out: &mut [u8], at: usize, declared_type: u16) {
        write_u16_be(out, at, declared_type);
        write_u16_be(out, at + 2, self.flag);
        write_u16_be(out, at + 4, self.subtables.len() as u16);
        if self.trailing() > 0 {
            write_u16_be(out, at + 6 + self.subtables.len() * 2, self.mark_filtering_set.unwrap_or(0));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_one_lookup(
    buf: &[u8],
    lookup_start: usize,
    ext_type: u16,
    active: &GlyphSet,
    gid_map: &[u16],
    mark_filter_sets_survive: bool,
    subset_subtable: SubtableSubsetter,
    schema_for_type: SchemaForType,
) -> RebuiltLookup {
    let Some(lookup_type) = read_u16_be(buf, lookup_start) else { return RebuiltLookup::hollow(1) };
    let Some(orig_flag) = read_u16_be(buf, lookup_start + 2) else { return RebuiltLookup::hollow(lookup_type) };
    let Some(sub_count) = read_u16_be(buf, lookup_start + 4) else { return RebuiltLookup::hollow(lookup_type) };

    let uses_mark_filter = orig_flag & 0x0010 != 0;
    let keep_mark_filter = uses_mark_filter && mark_filter_sets_survive;
    let sub_table_array_len = sub_count as usize * 2;
    let mark_filtering_set = if keep_mark_filter {
        read_u16_be(buf, lookup_start + 6 + sub_table_array_len)
    } else { None };
    let flag = if keep_mark_filter && mark_filtering_set.is_some() { orig_flag } else { orig_flag & !0x0010u16 };

    let mut effective_type: Option<u16> = None;
    let mut plan: Option<Option<Schema>> = None;
    let mut subtables: Vec<Vec<u8>> = Vec::new();
    for i in 0..sub_count as usize {
        let Some(rel) = read_u16_be(buf, lookup_start + 6 + i * 2) else { break };
        let Some((this_type, this_off)) = resolve_effective_type(buf, ext_type, lookup_type, lookup_start + rel as usize) else { continue };
        if *effective_type.get_or_insert(this_type) != this_type { continue; }
        if plan.is_none() { plan = Some(schema_for_type(this_type)); }
        let schema = plan.as_ref().and_then(Option::as_ref);
        if let Some(rebuilt) = subset_subtable(this_type, schema, buf, this_off, active, gid_map) {
            subtables.push(rebuilt);
        }
    }
    let (Some(real_type), false) = (effective_type, subtables.is_empty()) else {
        return RebuiltLookup::hollow(lookup_type);
    };

    RebuiltLookup { real_type, flag, mark_filtering_set, subtables }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_lookups(
    buf: &[u8],
    lookup_list_off: usize,
    ext_type: u16,
    active: &GlyphSet,
    gid_map: &[u16],
    mark_filter_sets_survive: bool,
    subset_subtable: SubtableSubsetter,
    schema_for_type: SchemaForType,
) -> Option<Vec<RebuiltLookup>> {
    let count = read_u16_be(buf, lookup_list_off)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let rel = read_u16_be(buf, lookup_list_off + 2 + i * 2)?;
        out.push(rebuild_one_lookup(
            buf, lookup_list_off + rel as usize, ext_type, active, gid_map,
            mark_filter_sets_survive, subset_subtable, schema_for_type,
        ));
    }
    Some(out)
}

fn assemble_inline(lookups: &[RebuiltLookup]) -> Option<Vec<u8>> {
    let header_len = 2 + lookups.len() * 2;
    let mut out = vec![0u8; header_len];
    write_u16_be(&mut out, 0, u16::try_from(lookups.len()).ok()?);

    let mut pos = header_len;
    for (i, lk) in lookups.iter().enumerate() {
        write_u16_be(&mut out, 2 + i * 2, u16::try_from(pos).ok()?);
        pos = pos.checked_add(lk.inline_len())?;
    }

    for lk in lookups {
        let at = out.len();
        out.resize(at + lk.header_len(), 0);
        if lk.is_hollow() {
            write_u16_be(&mut out, at, lk.real_type);
            continue;
        }
        lk.write_header_into(&mut out, at, lk.real_type);
        let mut sub_pos = lk.header_len();
        for (i, st) in lk.subtables.iter().enumerate() {
            write_u16_be(&mut out, at + 6 + i * 2, u16::try_from(sub_pos).ok()?);
            sub_pos = sub_pos.checked_add(st.len())?;
        }
        for st in &lk.subtables { out.extend_from_slice(st); }
    }
    Some(out)
}

fn assemble_promoted(lookups: &[RebuiltLookup], ext_type: u16) -> Option<Vec<u8>> {
    let list_header = 2 + lookups.len() * 2;

    let mut lookup_at = Vec::with_capacity(lookups.len());
    let mut pos = list_header;
    for lk in lookups {
        lookup_at.push(pos);
        pos = pos.checked_add(lk.promoted_header_len())?;
    }
    let payload_base = pos;

    let mut out = vec![0u8; payload_base];
    write_u16_be(&mut out, 0, u16::try_from(lookups.len()).ok()?);
    for (i, &at) in lookup_at.iter().enumerate() {
        write_u16_be(&mut out, 2 + i * 2, u16::try_from(at).ok()?);
    }

    let mut payload_pos = payload_base;
    for (lk, &at) in lookups.iter().zip(&lookup_at) {
        if lk.is_hollow() {
            write_u16_be(&mut out, at, lk.real_type);
            continue;
        }
        lk.write_header_into(&mut out, at, ext_type);
        let records_at = at + lk.header_len();
        for (i, st) in lk.subtables.iter().enumerate() {
            let record = records_at + i * EXTENSION_RECORD_LEN;
            write_u16_be(&mut out, at + 6 + i * 2, u16::try_from(record - at).ok()?);
            write_u16_be(&mut out, record, 1);
            write_u16_be(&mut out, record + 2, lk.real_type);
            write_u32_be(&mut out, record + 4, u32::try_from(payload_pos - record).ok()?);
            payload_pos = payload_pos.checked_add(st.len())?;
        }
    }

    out.reserve(lookups.iter().map(RebuiltLookup::payload_len).sum());
    for lk in lookups {
        for st in &lk.subtables { out.extend_from_slice(st); }
    }
    Some(out)
}

#[allow(clippy::too_many_arguments)]
pub fn subset_lookup_table(
    buf: &[u8],
    ext_type: u16,
    active: &GlyphSet,
    gid_map: &[u16],
    mark_filter_sets_survive: bool,
    subset_subtable: SubtableSubsetter,
    schema_for_type: SchemaForType,
) -> Option<Vec<u8>> {
    if buf.len() < 10 { return None; }
    let lookup_off = read_u16_be(buf, 8)? as usize;
    let lookups = rebuild_lookups(
        buf, lookup_off, ext_type, active, gid_map, mark_filter_sets_survive, subset_subtable, schema_for_type,
    )?;

    let new_lookup_off = super::layout_live_prefix_len(buf).unwrap_or(buf.len());
    if new_lookup_off > 0xFFFF { return None; }

    let fits = |list: &Vec<u8>| new_lookup_off + list.len() <= 0xFFFF;
    let list = match assemble_inline(&lookups).filter(fits) {
        Some(inline) => inline,
        None => assemble_promoted(&lookups, ext_type)?,
    };
    new_lookup_off.checked_add(list.len())?;

    let live: Vec<bool> = lookups.iter().map(|l| !l.subtables.is_empty()).collect();
    let (prefix, lookup_off) = match rebuild_prefix(buf, &live) {
        Some(rebuilt) if rebuilt.len() <= new_lookup_off => {
            let off = rebuilt.len();
            (rebuilt, off)
        }
        _ => (buf.get(..new_lookup_off)?.to_vec(), new_lookup_off),
    };
    let mut out = prefix;
    write_u16_be(&mut out, 8, u16::try_from(lookup_off).ok()?);
    out.extend_from_slice(&list);
    Some(out)
}

struct LangSys {
    tag: u32,
    features: Vec<u16>,
    required: Option<u16>,
}

struct Script {
    tag: u32,
    default_features: Option<Vec<u16>>,
    default_required: Option<u16>,
    langs: Vec<LangSys>,
}

fn rebuild_prefix(buf: &[u8], live: &[bool]) -> Option<Vec<u8>> {
    if read_u16_be(buf, 2)? != 0 { return None; }
    let script_off = read_u16_be(buf, 4)? as usize;
    let feature_off = read_u16_be(buf, 6)? as usize;

    let feature_count = read_u16_be(buf, feature_off)? as usize;
    let mut kept: Vec<(u32, Vec<u16>, Option<Vec<u8>>)> = Vec::new();
    let mut remap: Vec<Option<u16>> = alloc::vec![None; feature_count];
    for (i, slot) in remap.iter_mut().enumerate() {
        let rec = feature_off + 2 + i * 6;
        let tag = read_u32_be(buf, rec)?;
        let f = feature_off + read_u16_be(buf, rec + 4)? as usize;
        let params_rel = read_u16_be(buf, f)? as usize;
        let n_idx = read_u16_be(buf, f + 2)? as usize;

        let mut indices: Vec<u16> = Vec::new();
        for k in 0..n_idx {
            let idx = read_u16_be(buf, f + 4 + k * 2)?;
            if live.get(idx as usize).copied().unwrap_or(false) {
                indices.push(idx);
            }
        }
        let params = if params_rel == 0 {
            None
        } else {
            let at = f + params_rel;
            let end = super::feature_params_extent_at(buf, rec, at)?;
            Some(buf.get(at..end)?.to_vec())
        };
        if indices.is_empty() && params.is_none() { continue; }
        *slot = Some(u16::try_from(kept.len()).ok()?);
        kept.push((tag, indices, params));
    }
    if kept.len() == feature_count { return None; }

    let script_count = read_u16_be(buf, script_off)? as usize;
    let mut scripts: Vec<Script> = Vec::new();
    for i in 0..script_count {
        let rec = script_off + 2 + i * 6;
        let tag = read_u32_be(buf, rec)?;
        let s = script_off + read_u16_be(buf, rec + 4)? as usize;
        let default_rel = read_u16_be(buf, s)? as usize;
        let lang_count = read_u16_be(buf, s + 2)? as usize;

        let read_langsys = |at: usize| -> Option<(Vec<u16>, Option<u16>)> {
            let required = read_u16_be(buf, at + 2)?;
            let n = read_u16_be(buf, at + 4)? as usize;
            let mut out = Vec::new();
            for k in 0..n {
                let old = read_u16_be(buf, at + 6 + k * 2)? as usize;
                if let Some(Some(new)) = remap.get(old) { out.push(*new); }
            }
            let new_required = match required {
                0xFFFF => None,
                r => remap.get(r as usize).copied().flatten(),
            };
            Some((out, new_required))
        };

        let (default_features, default_required) = if default_rel == 0 {
            (None, None)
        } else {
            let (f, r) = read_langsys(s + default_rel)?;
            (Some(f), r)
        };
        let mut langs = Vec::new();
        for j in 0..lang_count {
            let lrec = s + 4 + j * 6;
            let ltag = read_u32_be(buf, lrec)?;
            let at = s + read_u16_be(buf, lrec + 4)? as usize;
            let (f, r) = read_langsys(at)?;
            langs.push(LangSys { tag: ltag, features: f, required: r });
        }
        scripts.push(Script { tag, default_features, default_required, langs });
    }

    let mut script_list: Vec<u8> = Vec::new();
    script_list.extend_from_slice(&u16::try_from(scripts.len()).ok()?.to_be_bytes());
    let mut script_bodies: Vec<u8> = Vec::new();
    let script_records_len = 2 + scripts.len() * 6;
    for Script { tag, default_features, default_required, langs } in &scripts {
        let body_at = script_records_len + script_bodies.len();
        script_list.extend_from_slice(&tag.to_be_bytes());
        script_list.extend_from_slice(&u16::try_from(body_at).ok()?.to_be_bytes());

        let header_len = 4 + langs.len() * 6;
        let mut body: Vec<u8> = Vec::new();
        let mut tail: Vec<u8> = Vec::new();
        let push_langsys = |tail: &mut Vec<u8>, features: &[u16], required: Option<u16>| -> usize {
            let at = header_len + tail.len();
            tail.extend_from_slice(&0u16.to_be_bytes());
            tail.extend_from_slice(&required.unwrap_or(0xFFFF).to_be_bytes());
            tail.extend_from_slice(&(features.len() as u16).to_be_bytes());
            for f in features { tail.extend_from_slice(&f.to_be_bytes()); }
            at
        };
        let default_at = default_features.as_ref()
            .map(|f| push_langsys(&mut tail, f, *default_required));
        body.extend_from_slice(&u16::try_from(default_at.unwrap_or(0)).ok()?.to_be_bytes());
        body.extend_from_slice(&u16::try_from(langs.len()).ok()?.to_be_bytes());
        for LangSys { tag: ltag, features, required } in langs {
            let at = push_langsys(&mut tail, features, *required);
            body.extend_from_slice(&ltag.to_be_bytes());
            body.extend_from_slice(&u16::try_from(at).ok()?.to_be_bytes());
        }
        body.extend_from_slice(&tail);
        script_bodies.extend_from_slice(&body);
    }
    script_list.extend_from_slice(&script_bodies);

    let mut feature_list: Vec<u8> = Vec::new();
    feature_list.extend_from_slice(&u16::try_from(kept.len()).ok()?.to_be_bytes());
    let feature_records_len = 2 + kept.len() * 6;
    let mut feature_bodies: Vec<u8> = Vec::new();
    for (tag, indices, params) in &kept {
        let body_at = feature_records_len + feature_bodies.len();
        feature_list.extend_from_slice(&tag.to_be_bytes());
        feature_list.extend_from_slice(&u16::try_from(body_at).ok()?.to_be_bytes());

        let own_len = 4 + indices.len() * 2;
        feature_bodies.extend_from_slice(&u16::try_from(params.as_ref().map_or(0, |_| own_len)).ok()?.to_be_bytes());
        feature_bodies.extend_from_slice(&u16::try_from(indices.len()).ok()?.to_be_bytes());
        for idx in indices { feature_bodies.extend_from_slice(&idx.to_be_bytes()); }
        if let Some(p) = params { feature_bodies.extend_from_slice(p); }
    }
    feature_list.extend_from_slice(&feature_bodies);

    let mut out: Vec<u8> = alloc::vec![0u8; 10];
    out[..4].copy_from_slice(&read_u32_be(buf, 0)?.to_be_bytes());
    let script_at = 10usize;
    let feature_at = script_at + script_list.len();
    write_u16_be(&mut out, 4, u16::try_from(script_at).ok()?);
    write_u16_be(&mut out, 6, u16::try_from(feature_at).ok()?);
    out.extend_from_slice(&script_list);
    out.extend_from_slice(&feature_list);
    Some(out)
}
