use crate::daecore::daetype::subsetter::GlyphSet;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use crate::daecore::daetype::decoder::{read_u16_be, read_i16_be, read_u32_be};
use super::super::anchor;
use super::super::gdef;
use super::super::{remap_gid, copy_device_table};
use super::schema::{Schema, CountSource, DropPolicy, RebuildPolicy, OffsetWidth, PayloadShape, EmptyPolicy, EnvRef};
use super::value::Value;

pub(crate) struct Env {
    scalars: alloc::vec::Vec<(&'static str, u16)>,
    coverages: alloc::vec::Vec<(&'static str, alloc::vec::Vec<u16>)>,
    keep_devices: bool,
    budget: usize,
}

impl Env {
    pub fn new() -> Self {
        Env {
            scalars: alloc::vec::Vec::new(),
            coverages: alloc::vec::Vec::new(),
            keep_devices: true,
            budget: 4_194_304,
        }
    }

    fn scalar(&self, name: &str) -> Option<u16> {
        self.scalars.iter().find(|(k, _)| *k == name).map(|&(_, v)| v)
    }

    fn set_scalar(&mut self, name: &'static str, value: u16) {
        match self.scalars.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => self.scalars.push((name, value)),
        }
    }

    fn coverage(&self, name: &str) -> Option<&[u16]> {
        self.coverages.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_slice())
    }

    fn set_coverage(&mut self, name: &'static str, gids: alloc::vec::Vec<u16>) {
        match self.coverages.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = gids,
            None => self.coverages.push((name, gids)),
        }
    }

    pub(crate) fn stripping_devices() -> Self {
        Env { keep_devices: false, ..Env::new() }
    }
}

pub(crate) fn generic_parse(
    buf: &[u8],
    pos: usize,
    anchor: usize,
    schema: &Schema,
    env: &mut Env,
    active: &GlyphSet,
    gid_map: &[u16],
) -> Result<(Option<Value>, usize), String> {
    env.budget = env
        .budget
        .checked_sub(1)
        .ok_or("generic: schema node budget exhausted")?;

    match schema {
        Schema::U16 => {
            let v = read_u16_be(buf, pos).ok_or("generic: truncated (U16)")?;
            Ok((Some(Value::U16(v)), 2))
        }
        Schema::I16 => {
            let v = read_i16_be(buf, pos).ok_or("generic: truncated (I16)")?;
            Ok((Some(Value::I16(v)), 2))
        }
        Schema::GlyphId => {
            let g = read_u16_be(buf, pos).ok_or("generic: truncated (GlyphId)")?;
            Ok((remap_gid(active, gid_map, g).map(Value::Glyph), 2))
        }
        Schema::Offset(width, child_schema) => {
            let (rel, consumed) = match width {
                OffsetWidth::W16 => (read_u16_be(buf, pos).ok_or("generic: truncated (Offset16)")? as u32, 2),
                OffsetWidth::W32 => (read_u32_be(buf, pos).ok_or("generic: truncated (Offset32)")?, 4),
            };
            if rel == 0 {
                return Ok((Some(Value::Offset(*width, None)), consumed));
            }
            let child_anchor = anchor + rel as usize;
            let (child_val, _child_consumed) =
                generic_parse(buf, child_anchor, child_anchor, child_schema, env, active, gid_map)?;
            match child_val {
                Some(v) => Ok((Some(Value::Offset(*width, Some(Box::new(v)))), consumed)),
                None => Ok((None, consumed)),
            }
        }
        Schema::Array(elem_schema, count_source, drop_policy) => {
            let count = resolve_count(count_source, env)?;
            let mut elems: Vec<Option<Value>> = Vec::with_capacity(count.min(256));
            let mut cursor = pos;
            for i in 0..count {
                let (v, consumed) = generic_parse(buf, cursor, anchor, elem_schema, env, active, gid_map)?;
                if consumed == 0 && i + 1 < count {
                    return Err("generic: array element consumes no input, so its count is unbounded".into());
                }
                elems.push(v);
                cursor += consumed;
            }
            let total_consumed = cursor - pos;
            match drop_policy {
                DropPolicy::AllOrNothing => {
                    if elems.iter().any(Option::is_none) {
                        Ok((None, total_consumed))
                    } else {
                        write_back_count(count_source, env, count);
                        Ok((Some(Value::Array(elems.into_iter().map(Option::unwrap).collect())), total_consumed))
                    }
                }
                DropPolicy::FilterSurvivors => {
                    let mut survivors: Vec<Value> = Vec::with_capacity(elems.len());
                    survivors.extend(elems.into_iter().flatten());
                    write_back_count(count_source, env, survivors.len());
                    Ok((Some(Value::Array(survivors)), total_consumed))
                }
                DropPolicy::FilterSurvivorsOrFail => {
                    let mut survivors: Vec<Value> = Vec::with_capacity(elems.len());
                    survivors.extend(elems.into_iter().flatten());
                    if survivors.is_empty() { Ok((None, total_consumed)) }
                    else {
                        write_back_count(count_source, env, survivors.len());
                        Ok((Some(Value::Array(survivors)), total_consumed))
                    }
                }
            }
        }
        Schema::OffsetArray(child_schema, count_source, rebuild_policy, width) => {
            let count = resolve_count(count_source, env)?;
            let elem_width = width.bytes();
            let fits = buf.len().saturating_sub(pos) / elem_width.max(1);
            let mut slots: Vec<Option<Value>> = Vec::with_capacity(count.min(fits));
            let mut cursor = pos;
            for _ in 0..count {
                let rel = match width {
                    OffsetWidth::W16 => read_u16_be(buf, cursor).ok_or("generic: truncated (OffsetArray entry, W16)")? as u32,
                    OffsetWidth::W32 => read_u32_be(buf, cursor).ok_or("generic: truncated (OffsetArray entry, W32)")?,
                };
                if rel == 0 {
                    slots.push(None);
                } else {
                    let child_anchor = anchor + rel as usize;
                    let (child_val, _) =
                        generic_parse(buf, child_anchor, child_anchor, child_schema, env, active, gid_map)?;
                    slots.push(child_val);
                }
                cursor += elem_width;
            }
            let total_consumed = cursor - pos;
            let final_slots = match rebuild_policy {
                RebuildPolicy::CompactSurvivors => { let mut s = slots; s.retain(Option::is_some); s }
                RebuildPolicy::PreserveSlotPositions => {
                    let mut s = slots;
                    while matches!(s.last(), Some(None)) { s.pop(); }
                    s
                }
            };
            if final_slots.is_empty() {
                Ok((None, total_consumed))
            } else {
                write_back_count(count_source, env, final_slots.len());
                Ok((Some(Value::OffsetArray(*width, final_slots)), total_consumed))
            }
        }
        Schema::Struct(fields) => {
            let mut cursor = pos;
            let mut out_fields: Vec<(&'static str, Value)> = Vec::with_capacity(fields.len());
            let mut any_failed = false;
            for field in fields {
                let (v, consumed) = generic_parse(buf, cursor, anchor, &field.schema, env, active, gid_map)?;
                cursor += consumed;
                match v {
                    Some(val) => {
                        bind_field(env, field.bind, &val);
                        out_fields.push((field.name, val));
                    }
                    None => any_failed = true,
                }
            }
            let total_consumed = cursor - pos;
            if any_failed { return Ok((None, total_consumed)); }
            for (field, (_, value)) in fields.iter().zip(out_fields.iter_mut()) {
                if let (Some(bind_name), Value::U16(_)) = (field.bind, &*value)
                    && let Some(actual) = env.scalar(bind_name) {
                        *value = Value::U16(actual);
                    }
            }
            Ok((Some(Value::Struct(out_fields)), total_consumed))
        }
        Schema::FormatSwitch(peek_offset, variants) => {
            let tag = read_u16_be(buf, pos + peek_offset).ok_or("generic: truncated (FormatSwitch tag)")?;
            let variant = variants.iter().find(|(t, _)| *t == tag)
                .ok_or_else(|| format!("generic: unknown format tag {tag}"))?;
            generic_parse(buf, pos, anchor, &variant.1, env, active, gid_map)
        }
        Schema::ValueRecordField(EnvRef(name)) => {
            let raw = env.scalar(name).ok_or_else(|| format!("generic: unbound env ref '{name}'"))?;
            let value_bits = raw & 0x000F;
            let mut vals = [0i16; 4];
            let mut cursor = pos;
            for slot in vals.iter_mut().take(value_bits.count_ones() as usize) {
                *slot = read_i16_be(buf, cursor).ok_or("generic: truncated (ValueRecordField)")?;
                cursor += 2;
            }

            let mut devices = Vec::with_capacity((raw & 0x00F0).count_ones() as usize);
            for bit in [0x0010u16, 0x0020, 0x0040, 0x0080] {
                if raw & bit == 0 {
                    continue;
                }
                let rel = read_u16_be(buf, cursor).ok_or("generic: truncated (ValueRecord device offset)")? as usize;
                cursor += 2;
                if env.keep_devices {
                    devices.push(if rel == 0 { None } else { copy_device_table(buf, anchor + rel) });
                }
            }

            let out_bitmask = if env.keep_devices { raw } else { value_bits };
            let consumed = raw.count_ones() as usize * 2;
            Ok((Some(Value::ValueRecord(out_bitmask, vals, devices)), consumed))
        }
        Schema::Coverage(policy, raw_bind) => {
            let gids = super::super::parse_coverage(buf, pos).map_err(|e| format!("generic: {e}"))?;
            if let Some(EnvRef(name)) = raw_bind {
                env.set_coverage(name, gids.clone());
            }
            let mut survivors: Vec<u16> = Vec::with_capacity(gids.len());
            survivors.extend(gids.iter().filter_map(|&g| remap_gid(active, gid_map, g)));
            match policy {
                EmptyPolicy::Fail if survivors.is_empty() => Ok((None, 0)),
                _ => Ok((Some(Value::Coverage(survivors)), 0)),
            }
        }
        Schema::ClassMatrix(cell_schema) => {
            let cd1_rel = read_u16_be(buf, pos).ok_or("generic: truncated (ClassMatrix classDef1)")? as usize;
            let cd2_rel = read_u16_be(buf, pos + 2).ok_or("generic: truncated (ClassMatrix classDef2)")? as usize;
            let c1 = read_u16_be(buf, pos + 4).ok_or("generic: truncated (ClassMatrix class1Count)")? as usize;
            let c2 = read_u16_be(buf, pos + 6).ok_or("generic: truncated (ClassMatrix class2Count)")? as usize;
            let grid_at = pos + 8;

            let filtered = |rel: usize| -> Result<Vec<(u16, u16)>, String> {
                if rel == 0 { return Ok(Vec::new()); }
                let entries = super::super::parse_classdef(buf, anchor + rel).map_err(|e| format!("generic: {e}"))?;
                Ok(entries.into_iter()
                    .filter_map(|(g, c)| remap_gid(active, gid_map, g).map(|ng| (ng, c)))
                    .collect())
            };
            let cd1 = filtered(cd1_rel)?;
            let cd2 = filtered(cd2_rel)?;

            let surviving = |entries: &[(u16, u16)], count: usize| -> (Vec<usize>, BTreeMap<u16, u16>) {
                let mut classes: Vec<usize> = entries
                    .iter()
                    .map(|&(_, c)| c as usize)
                    .filter(|&c| c != 0 && c < count)
                    .collect();
                classes.sort_unstable();
                classes.dedup();
                let mut order: Vec<usize> = Vec::with_capacity(classes.len() + 1);
                if count > 0 { order.push(0); }
                order.extend(classes);
                let map = order.iter().enumerate().map(|(new, &old)| (old as u16, new as u16)).collect();
                (order, map)
            };
            let (rows, map1) = surviving(&cd1, c1);
            let (cols, map2) = surviving(&cd2, c2);

            let mut grid: Vec<Value> = Vec::new();
            if !rows.is_empty() && !cols.is_empty() {
                let (_, stride) = generic_parse(buf, grid_at, anchor, cell_schema, env, active, gid_map)?;
                if stride == 0 {
                    return Err("generic: class matrix cell consumes no input, so its counts are unbounded".into());
                }
                grid.reserve(rows.len() * cols.len());
                for &r in &rows {
                    for &c in &cols {
                            let at = grid_at + (r * c2 + c) * stride;
                        let (cell, _) = generic_parse(buf, at, anchor, cell_schema, env, active, gid_map)?;
                        grid.push(cell.ok_or("generic: class matrix cell did not resolve")?);
                    }
                }
            }

            let renumber = |entries: Vec<(u16, u16)>, map: &BTreeMap<u16, u16>| -> Vec<(u16, u16)> {
                entries.into_iter().filter_map(|(g, c)| map.get(&c).map(|&nc| (g, nc))).collect()
            };
            Ok((
                Some(Value::ClassMatrix {
                    class_def1: renumber(cd1, &map1),
                    class_def2: renumber(cd2, &map2),
                    class1_count: rows.len() as u16,
                    class2_count: cols.len() as u16,
                    grid,
                }),
                8,
            ))
        }
        Schema::ClassDef(policy) => {
            let entries = super::super::parse_classdef(buf, pos).map_err(|e| format!("generic: {e}"))?;
            let survivors: Vec<(u16, u16)> = entries.into_iter()
                .filter_map(|(g, c)| remap_gid(active, gid_map, g).map(|ng| (ng, c)))
                .collect();
            match policy {
                EmptyPolicy::Fail if survivors.is_empty() => Ok((None, 0)),
                _ => Ok((Some(Value::ClassDef(survivors)), 0)),
            }
        }
        Schema::ValueFormatField(EnvRef(name)) => {
            let raw = read_u16_be(buf, pos).ok_or("generic: truncated (ValueFormatField)")?;
            env.set_scalar(name, raw);
            Ok((Some(Value::U16(if env.keep_devices { raw } else { raw & 0x000F })), 2))
        }
        Schema::Anchor => {
            let (x, y) = anchor::parse_anchor(buf, pos).ok_or("generic: Anchor: truncated or unrecognized format")?;
            let point = anchor::parse_anchor_point(buf, pos);
            let (mut dx, mut dy) = (None, None);
            if env.keep_devices && read_u16_be(buf, pos) == Some(3) {
                for (slot, out) in [(6usize, &mut dx), (8, &mut dy)] {
                    if let Some(rel) = read_u16_be(buf, pos + slot).filter(|&r| r != 0) {
                        *out = copy_device_table(buf, pos + rel as usize);
                    }
                }
            }
            Ok((Some(Value::Anchor(x, y, point, dx, dy)), 6))
        }
        Schema::CoveredArray(extra_fields, payload_schema, shape) => {
            let cov_rel = read_u16_be(buf, pos).ok_or("generic: truncated (CoveredArray Coverage offset)")? as usize;
            let coverage: Vec<u16> = if cov_rel == 0 {
                Vec::new()
            } else {
                super::super::parse_coverage(buf, anchor + cov_rel).map_err(|e| format!("generic: {e}"))?
            };
            let mut cursor = pos + 2;
            let mut extra_values: Vec<(&'static str, Value)> = Vec::with_capacity(extra_fields.len());
            for field in extra_fields {
                let (v, consumed) = generic_parse(buf, cursor, anchor, &field.schema, env, active, gid_map)?;
                cursor += consumed;
                let val = v.ok_or("generic: CoveredArray: an extra field (never GID-tagged in any real case) unexpectedly failed to survive")?;
                bind_field(env, field.bind, &val);
                extra_values.push((field.name, val));
            }
            let count = read_u16_be(buf, cursor).ok_or("generic: truncated (CoveredArray Count)")? as usize;
            cursor += 2;
            let mut entries: Vec<(u16, Value)> = Vec::with_capacity(count.min(buf.len().saturating_sub(cursor)));
            for i in 0..count {
                let (payload_opt, consumed) = match shape {
                    PayloadShape::Inline => generic_parse(buf, cursor, anchor, payload_schema, env, active, gid_map)?,
                    PayloadShape::Offsets(width) => {
                        let rel = match width {
                            OffsetWidth::W16 => read_u16_be(buf, cursor).ok_or("generic: truncated (CoveredArray payload offset, W16)")? as u32,
                            OffsetWidth::W32 => read_u32_be(buf, cursor).ok_or("generic: truncated (CoveredArray payload offset, W32)")?,
                        };
                        if rel == 0 {
                            (None, width.bytes())
                        } else {
                            let child_anchor = anchor + rel as usize;
                            let (v, _) = generic_parse(buf, child_anchor, child_anchor, payload_schema, env, active, gid_map)?;
                            (v, width.bytes())
                        }
                    }
                };
                cursor += consumed;
                if let (Some(pv), Some(&orig_gid)) = (payload_opt, coverage.get(i))
                    && let Some(new_gid) = remap_gid(active, gid_map, orig_gid) {
                        entries.push((new_gid, pv));
                    }
            }
            let total_consumed = cursor - pos;
            if entries.is_empty() { Ok((None, total_consumed)) } else { Ok((Some(Value::CoveredArray(extra_values, *shape, entries)), total_consumed)) }
        }
        Schema::ZippedWithBoundCoverage(EnvRef(cov_name), payload_schema, shape, drop_policy) => {
            let coverage = env.coverage(cov_name).map(<[u16]>::to_vec)
                .ok_or_else(|| format!("generic: unbound coverage ref '{cov_name}'"))?;
            let count = read_u16_be(buf, pos).ok_or("generic: truncated (ZippedWithBoundCoverage Count)")? as usize;
            let mut cursor = pos + 2;
            let mut elems: Vec<Option<Value>> = Vec::with_capacity(count.min(buf.len().saturating_sub(cursor)));
            for i in 0..count {
                let (payload_opt, consumed) = match shape {
                    PayloadShape::Inline => generic_parse(buf, cursor, anchor, payload_schema, env, active, gid_map)?,
                    PayloadShape::Offsets(width) => {
                        let rel = match width {
                            OffsetWidth::W16 => read_u16_be(buf, cursor).ok_or("generic: truncated (ZippedWithBoundCoverage payload offset, W16)")? as u32,
                            OffsetWidth::W32 => read_u32_be(buf, cursor).ok_or("generic: truncated (ZippedWithBoundCoverage payload offset, W32)")?,
                        };
                        if rel == 0 {
                            (None, width.bytes())
                        } else {
                            let child_anchor = anchor + rel as usize;
                            let (v, _) = generic_parse(buf, child_anchor, child_anchor, payload_schema, env, active, gid_map)?;
                            (v, width.bytes())
                        }
                    }
                };
                cursor += consumed;
                let zipped = match (payload_opt, coverage.get(i)) {
                    (Some(pv), Some(&orig_gid)) => remap_gid(active, gid_map, orig_gid).map(|_| pv),
                    _ => None,
                };
                elems.push(zipped);
            }
            let total_consumed = cursor - pos;
            match drop_policy {
                DropPolicy::AllOrNothing => {
                    if elems.iter().any(Option::is_none) { Ok((None, total_consumed)) }
                    else { Ok((Some(Value::ZippedWithBoundCoverage(*shape, elems.into_iter().map(Option::unwrap).collect())), total_consumed)) }
                }
                DropPolicy::FilterSurvivors => {
                    let mut survivors: Vec<Value> = Vec::with_capacity(elems.len());
                    survivors.extend(elems.into_iter().flatten());
                    Ok((Some(Value::ZippedWithBoundCoverage(*shape, survivors)), total_consumed))
                }
                DropPolicy::FilterSurvivorsOrFail => {
                    let mut survivors: Vec<Value> = Vec::with_capacity(elems.len());
                    survivors.extend(elems.into_iter().flatten());
                    if survivors.is_empty() { Ok((None, total_consumed)) }
                    else { Ok((Some(Value::ZippedWithBoundCoverage(*shape, survivors)), total_consumed)) }
                }
            }
        }
        Schema::CaretValue => {
            let cv = gdef::parse_caret_value(buf, pos).ok_or("generic: CaretValue: truncated or unrecognized format")?;
            Ok((Some(Value::CaretValue(cv)), 4))
        }
        Schema::DeltaCoverageSubst => {
            let cov_rel = read_u16_be(buf, pos + 2).ok_or("generic: truncated (DeltaCoverageSubst Coverage offset)")? as usize;
            let coverage: Vec<u16> = if cov_rel == 0 {
                Vec::new()
            } else {
                super::super::parse_coverage(buf, anchor + cov_rel).map_err(|e| format!("generic: {e}"))?
            };
            let delta = read_i16_be(buf, pos + 4).ok_or("generic: truncated (DeltaCoverageSubst delta)")?;
            let mut entries: Vec<(u16, Value)> = Vec::with_capacity(coverage.len());
            for &g in &coverage {
                let sub = (g as i32 + delta as i32).rem_euclid(65536) as u16;
                if let (Some(new_g), Some(new_sub)) = (remap_gid(active, gid_map, g), remap_gid(active, gid_map, sub)) {
                    entries.push((new_g, Value::Glyph(new_sub)));
                }
            }
            if entries.is_empty() {
                Ok((None, 6))
            } else {
                let out = Value::Struct(vec![
                    ("format", Value::U16(2)),
                    ("entries", Value::CoveredArray(Vec::new(), PayloadShape::Inline, entries)),
                ]);
                Ok((Some(out), 6))
            }
        }
    }
}

fn bind_field(env: &mut Env, bind: Option<&'static str>, val: &Value) {
    let Some(bind_name) = bind else { return };
    if let Value::U16(raw) = val {
        env.set_scalar(bind_name, *raw);
    }
}

fn resolve_count(source: &CountSource, env: &Env) -> Result<usize, String> {
    match source {
        CountSource::Fixed(n) => Ok(*n),
        CountSource::Field(EnvRef(name)) => env.scalar(name)
            .map(|v| v as usize)
            .ok_or_else(|| format!("generic: unbound env ref '{name}' for count")),
        CountSource::FieldMinusOne(EnvRef(name)) => env.scalar(name)
            .map(|v| v as usize)
            .ok_or_else(|| format!("generic: unbound env ref '{name}' for count"))
            .and_then(|v| v.checked_sub(1).ok_or_else(|| format!("generic: count field '{name}' was 0, can't subtract 1")))
    }
}

fn write_back_count(source: &CountSource, env: &mut Env, actual_len: usize) {
    match source {
        CountSource::Fixed(_) => {}
        CountSource::Field(EnvRef(name)) => { env.set_scalar(name, actual_len as u16); }
        CountSource::FieldMinusOne(EnvRef(name)) => { env.set_scalar(name, (actual_len + 1) as u16); }
    }
}
