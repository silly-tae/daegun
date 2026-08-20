use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use crate::daecore::daetype::decoder::{write_u16_be, write_i16_be, write_u32_be};
use super::super::anchor::build_anchor_with_devices;
use super::super::gdef::build_caret_value;
use super::super::{build_coverage, build_classdef};
use super::schema::{OffsetWidth, PayloadShape};
use super::value::{value_field_count, Value};

struct DevicePatch<'v> {
    slot: usize,
    base: usize,
    bytes: &'v [u8],
}

pub(crate) fn generic_build(value: &Value) -> Option<Vec<u8>> {
    let mut overflow = false;
    let bytes = build_blob(value, &mut overflow);
    (!overflow).then_some(bytes)
}

struct Blob<'v, 'o> {
    tail: Vec<u8>,
    patches: Vec<DevicePatch<'v>>,
    shared: BTreeMap<Vec<u8>, usize>,
    overflow: &'o mut bool,
    reserve: usize,
    share: bool,
}

struct Assembled {
    bytes: Vec<u8>,
    unreachable: usize,
    pool_len: usize,
    all_based_at_zero: bool,
}

fn build_blob(value: &Value, overflow: &mut bool) -> Vec<u8> {
    let (bytes, over) = layout(value, false);
    if !over {
        return bytes;
    }
    let (shared, still_over) = layout(value, true);
    *overflow = still_over;
    shared
}

fn layout(value: &Value, share: bool) -> (Vec<u8>, bool) {
    let mut overflow = false;
    let first = assemble(value, 0, share, &mut overflow);
    if first.unreachable == 0 || first.pool_len == 0 || !first.all_based_at_zero {
        return (first.bytes, overflow);
    }
    let mut retried = false;
    let second = assemble(value, first.pool_len, share, &mut retried);
    let no_new_overflow = overflow || !retried;
    if second.unreachable < first.unreachable && no_new_overflow {
        (second.bytes, retried)
    } else {
        (first.bytes, overflow)
    }
}

fn assemble(value: &Value, reserve: usize, share: bool, overflow: &mut bool) -> Assembled {
    let inline = inline_width(value);
    let mut header = vec![0u8; inline];
    let mut blob = Blob {
        tail: Vec::new(),
        patches: Vec::new(),
        shared: BTreeMap::new(),
        overflow,
        reserve,
        share,
    };
    write_into(value, &mut header, 0, &mut blob);

    let patches = core::mem::take(&mut blob.patches);
    let mut placed: BTreeMap<&[u8], usize> = BTreeMap::new();
    let mut pool: Vec<u8> = Vec::new();
    let mut slots: Vec<(usize, usize, usize)> = Vec::with_capacity(patches.len());
    for patch in &patches {
        let pool_len = pool.len();
        let within = *placed.entry(patch.bytes).or_insert(pool_len);
        if within == pool_len {
            pool.extend_from_slice(patch.bytes);
        }
        slots.push((patch.slot, patch.base, within));
    }
    let all_based_at_zero = patches.iter().all(|p| p.base == 0);

    let tail = core::mem::take(&mut blob.tail);
    let reserved = reserve >= pool.len();
    let pool_at = if reserved { inline } else { inline + reserve + tail.len() };
    header.resize(inline + reserve, 0);
    header.extend_from_slice(&tail);
    if reserved {
        header[inline..inline + pool.len()].copy_from_slice(&pool);
    } else {
        header.extend_from_slice(&pool);
    }

    let mut unreachable = 0;
    for (slot, base, within) in slots {
        match (pool_at + within).checked_sub(base).and_then(|d| u16::try_from(d).ok()) {
            Some(v) => write_u16_be(&mut header, slot, v),
            None => unreachable += 1,
        }
    }
    Assembled { bytes: header, unreachable, pool_len: pool.len(), all_based_at_zero }
}

fn append_child<'v>(child: &'v Value, header_len: usize, blob: &mut Blob<'v, '_>) -> usize {
    let (child_bytes, child_patches) = build_child(child, blob.share, blob.overflow);
    if blob.share
        && let Some(&already) = blob.shared.get(&child_bytes) {
            return already;
        }
    let target = header_len + blob.reserve + blob.tail.len();
    blob.tail.extend_from_slice(&child_bytes);
    if blob.share {
        blob.shared.insert(child_bytes, target);
    }
    for mut patch in child_patches {
        patch.slot += target;
        patch.base += target;
        blob.patches.push(patch);
    }
    target
}

fn append_blob(child_bytes: &[u8], header_len: usize, blob: &mut Blob<'_, '_>) -> usize {
    if blob.share
        && let Some(&already) = blob.shared.get(child_bytes) {
            return already;
        }
    let target = header_len + blob.reserve + blob.tail.len();
    blob.tail.extend_from_slice(child_bytes);
    if blob.share {
        blob.shared.insert(child_bytes.to_vec(), target);
    }
    target
}

fn build_child<'v>(value: &'v Value, share: bool, overflow: &mut bool) -> (Vec<u8>, Vec<DevicePatch<'v>>) {
    let mut header = vec![0u8; inline_width(value)];
    let mut blob = Blob {
        tail: Vec::new(),
        patches: Vec::new(),
        shared: BTreeMap::new(),
        overflow,
        reserve: 0,
        share,
    };
    write_into(value, &mut header, 0, &mut blob);
    header.extend(core::mem::take(&mut blob.tail));
    (header, core::mem::take(&mut blob.patches))
}

fn inline_width(value: &Value) -> usize {
    match value {
        Value::U16(_) | Value::I16(_) | Value::Glyph(_) => 2,
        Value::Offset(width, _) => width.bytes(),
        Value::Array(elems) => elems.iter().map(inline_width).sum(),
        Value::OffsetArray(width, slots) => slots.len() * width.bytes(),
        Value::Struct(fields) => fields.iter().map(|(_, v)| inline_width(v)).sum(),
        Value::ValueRecord(bitmask, _vals, devices) => (value_field_count(*bitmask) + devices.len()) * 2,
        Value::Coverage(gids) => build_coverage(gids).len(),
        Value::ClassDef(entries) => build_classdef(entries).len(),
        Value::Anchor(_, _, point, dx, dy) => {
            if dx.is_some() || dy.is_some() {
                10 + dx.as_ref().map_or(0, |d| d.len()) + dy.as_ref().map_or(0, |d| d.len())
            } else if point.is_some() {
                8
            } else {
                6
            }
        }
        Value::CoveredArray(extra_fields, shape, entries) => {
            let extra_width: usize = extra_fields.iter().map(|(_, v)| inline_width(v)).sum();
            let payload_width: usize = payload_inline_width(shape, entries.iter().map(|(_, v)| v));
            4 + extra_width + payload_width
        }
        Value::ZippedWithBoundCoverage(shape, entries) => {
            2 + payload_inline_width(shape, entries.iter())
        }
        Value::CaretValue(_) => 4,
        Value::ClassMatrix { grid, .. } => 8 + grid.iter().map(inline_width).sum::<usize>(),
    }
}

fn payload_inline_width<'a>(shape: &PayloadShape, entries: impl Iterator<Item = &'a Value>) -> usize {
    match shape {
        PayloadShape::Inline => entries.map(inline_width).sum(),
        PayloadShape::Offsets(w) => entries.count() * w.bytes(),
    }
}

fn write_into<'v>(value: &'v Value, header: &mut [u8], pos: usize, blob: &mut Blob<'v, '_>) -> usize {
    match value {
        Value::U16(v) => { write_u16_be(header, pos, *v); 2 }
        Value::I16(v) => { write_i16_be(header, pos, *v); 2 }
        Value::Glyph(v) => { write_u16_be(header, pos, *v); 2 }
        Value::Offset(width, child) => {
            if let Some(v) = child {
                let target = append_child(v, header.len(), blob);
                write_offset(header, pos, *width, target, blob.overflow);
            }
            width.bytes()
        }
        Value::Array(elems) => {
            let mut p = pos;
            for e in elems {
                p += write_into(e, header, p, blob);
            }
            p - pos
        }
        Value::OffsetArray(width, slots) => {
            let mut p = pos;
            for slot in slots {
                if let Some(v) = slot {
                    let target = append_child(v, header.len(), blob);
                    write_offset(header, p, *width, target, blob.overflow);
                }
                p += width.bytes();
            }
            p - pos
        }
        Value::Struct(fields) => {
            let mut p = pos;
            for (_, v) in fields {
                p += write_into(v, header, p, blob);
            }
            p - pos
        }
        Value::ValueRecord(bitmask, vals, devices) => {
            let mut p = pos;
            for v in vals.iter().take(value_field_count(*bitmask)) {
                write_i16_be(header, p, *v);
                p += 2;
            }
            for device in devices {
                if let Some(bytes) = device {
                    blob.patches.push(DevicePatch { slot: p, base: 0, bytes });
                }
                p += 2;
            }
            p - pos
        }
        Value::Coverage(gids) => {
            let bytes = build_coverage(gids);
            header[pos..pos + bytes.len()].copy_from_slice(&bytes);
            bytes.len()
        }
        Value::ClassDef(entries) => {
            let bytes = build_classdef(entries);
            header[pos..pos + bytes.len()].copy_from_slice(&bytes);
            bytes.len()
        }
        Value::ClassMatrix { class_def1, class_def2, class1_count, class2_count, grid } => {
            for (slot, entries) in [(pos, class_def1), (pos + 2, class_def2)] {
                let target = append_blob(&build_classdef(entries), header.len(), blob);
                write_offset(header, slot, OffsetWidth::W16, target, blob.overflow);
            }
            write_u16_be(header, pos + 4, *class1_count);
            write_u16_be(header, pos + 6, *class2_count);
            let mut p = pos + 8;
            for cell in grid {
                p += write_into(cell, header, p, blob);
            }
            p - pos
        }
        Value::Anchor(x, y, point, dx, dy) => {
            let bytes = build_anchor_with_devices(*x, *y, *point, dx.as_deref(), dy.as_deref());
            header[pos..pos + bytes.len()].copy_from_slice(&bytes);
            inline_width(value)
        }
        Value::CoveredArray(extra_fields, shape, entries) => {
            let mut p = pos + 2;
            for (_, v) in extra_fields {
                p += write_into(v, header, p, blob);
            }
            write_u16_be(header, p, entries.len() as u16);
            p += 2;
            p += write_payload_entries(shape, entries.iter().map(|(_, v)| v), header, p, blob);
            let gids: Vec<u16> = entries.iter().map(|(g, _)| *g).collect();
            let cov_bytes = build_coverage(&gids);
            let target = header.len() + blob.reserve + blob.tail.len();
            write_offset(header, pos, OffsetWidth::W16, target, blob.overflow);
            blob.tail.extend_from_slice(&cov_bytes);
            p - pos
        }
        Value::ZippedWithBoundCoverage(shape, entries) => {
            write_u16_be(header, pos, entries.len() as u16);
            2 + write_payload_entries(shape, entries.iter(), header, pos + 2, blob)
        }
        Value::CaretValue(cv) => {
            header[pos..pos + 4].copy_from_slice(&build_caret_value(cv));
            4
        }
    }
}

fn write_payload_entries<'v>(shape: &PayloadShape, entries: impl Iterator<Item = &'v Value>, header: &mut [u8], start: usize, blob: &mut Blob<'v, '_>) -> usize {
    let mut p = start;
    match shape {
        PayloadShape::Inline => {
            for v in entries {
                p += write_into(v, header, p, blob);
            }
        }
        PayloadShape::Offsets(width) => {
            for v in entries {
                let target = append_child(v, header.len(), blob);
                write_offset(header, p, *width, target, blob.overflow);
                p += width.bytes();
            }
        }
    }
    p - start
}

fn write_offset(
    header: &mut [u8],
    pos: usize,
    width: OffsetWidth,
    target: usize,
    overflow: &mut bool,
) {
    match width {
        OffsetWidth::W16 => match u16::try_from(target) {
            Ok(v) => write_u16_be(header, pos, v),
            Err(_) => *overflow = true,
        },
        OffsetWidth::W32 => match u32::try_from(target) {
            Ok(v) => write_u32_be(header, pos, v),
            Err(_) => *overflow = true,
        },
    }
}
