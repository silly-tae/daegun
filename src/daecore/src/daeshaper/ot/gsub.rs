use super::apply::{
    apply_chain_context, apply_context, ligate_input, match_backtrack, match_input, match_lookahead,
    new_match_positions, offset_class_def, offset_coverage, ApplyContext, CoverageList,
    MaySkip, SkippingIterator, MAX_NESTING_LEVEL, U16Array,
};
use crate::daecore::daeshaper::buffer::glyph_props;
use super::map::MAX_VALUE;
use super::{resolve_extension, Coverage, LayoutTable, Lookup};
use crate::daecore::daetype::decoder::{read_u16_be, window};

pub(crate) const SINGLE: u16 = 1;
pub(crate) const MULTIPLE: u16 = 2;
pub(crate) const ALTERNATE: u16 = 3;
pub(crate) const LIGATURE: u16 = 4;
pub(crate) const CONTEXT: u16 = 5;
pub(crate) const CHAIN_CONTEXT: u16 = 6;
pub(crate) const EXTENSION: u16 = 7;
pub(crate) const REVERSE_CHAIN_SINGLE: u16 = 8;

pub(crate) const IN_PLACE: bool = false;

pub(crate) fn apply(ctx: &mut ApplyContext, index: u16) -> bool {
    let Some(table) = ctx.gsub else { return false };
    let lookup = match ctx.lookup {
        Some((at, l, props)) if at == index => {
            ctx.lookup_props = props;
            l
        }
        _ => match table.lookup(index) {
            Some(l) => {
                ctx.lookup_props = l.props();
                l
            }
            None => return false,
        },
    };

    let already_cleared = lookup.subtable_count == 1 && ctx.glyph_digest.is_some();
    let cur = (!already_cleared).then(|| ctx.buffer.cur(0).id as u16);

    for i in 0..lookup.subtable_count {
        if cur.is_some_and(|g| !ctx.subtable_may_have(i as usize, g)) {
            continue;
        }
        let Some(data) = lookup.subtable(i) else { continue };
        let Some((kind, data)) = resolve_extension(data, lookup.kind, EXTENSION) else { continue };
        ctx.subtable = i as usize;
        if apply_subtable(ctx, kind, data) {
            return true;
        }
    }
    false
}

pub(crate) fn is_reverse(lookup: &Lookup) -> bool {
    resolved_kind(lookup) == REVERSE_CHAIN_SINGLE
}

fn resolved_kind(lookup: &Lookup) -> u16 {
    if lookup.kind != EXTENSION {
        return lookup.kind;
    }
    for i in 0..lookup.subtable_count {
        if let Some((kind, _)) = lookup
            .subtable(i)
            .and_then(|s| resolve_extension(s, EXTENSION, EXTENSION))
        {
            return kind;
        }
    }
    EXTENSION
}

fn apply_subtable(ctx: &mut ApplyContext, kind: u16, data: &[u8]) -> bool {
    match kind {
        SINGLE => single(ctx, data),
        MULTIPLE => multiple(ctx, data),
        ALTERNATE => alternate(ctx, data),
        LIGATURE => ligature(ctx, data),
        CONTEXT => apply_context(ctx, data),
        CHAIN_CONTEXT => apply_chain_context(ctx, data),
        REVERSE_CHAIN_SINGLE => reverse_chain_single(ctx, data),
        _ => false,
    }
}

fn single(ctx: &mut ApplyContext, data: &[u8]) -> bool {
    let glyph = ctx.buffer.cur(0).id as u16;
    let Some(h) = window::<4>(data, 0) else { return false };
    let format = u16::from_be_bytes([h[0], h[1]]);
    let Some(coverage) = ctx.coverage_resolved(data, u16::from_be_bytes([h[2], h[3]]), true) else {
        return false;
    };

    let subst = match format {
        1 => {
            if !coverage.contains(glyph) {
                return false;
            }
            let Some(delta) = read_u16_be(data, 4) else { return false };
            glyph.wrapping_add(delta)
        }
        2 => {
            let Some(index) = coverage.index_of(glyph) else { return false };
            let Some(count) = read_u16_be(data, 4) else { return false };
            let Some(rest) = data.get(6..) else { return false };
            let Some(g) = (U16Array { data: rest, len: count }).get(index) else { return false };
            g
        }
        _ => return false,
    };

    ctx.replace_glyph(subst);
    true
}

fn multiple(ctx: &mut ApplyContext, data: &[u8]) -> bool {
    let glyph = ctx.buffer.cur(0).id as u16;
    if read_u16_be(data, 0) != Some(1) {
        return false;
    }
    let Some(cov) = ctx.coverage(data, 2) else { return false };
    let Some(seq) = coverage_indexed_offset(&cov, data, glyph) else { return false };
    let Some(count) = read_u16_be(seq, 0) else { return false };
    let Some(rest) = seq.get(2..) else { return false };
    let substitutes = U16Array { data: rest, len: count };

    match count {
        0 => {
            ctx.buffer.delete_glyph();
            true
        }
        1 => match substitutes.get(0) {
            Some(g) => {
                ctx.replace_glyph(g);
                true
            }
            None => false,
        },
        _ => {
            let cur = ctx.buffer.cur(0);
            let class = if cur.is_ligature() { glyph_props::BASE_GLYPH } else { 0 };
            let lig_id = cur.lig_id();

            for i in 0..count {
                let Some(g) = substitutes.get(i) else { return false };
                if lig_id == 0 {
                    ctx.buffer.cur_mut(0).set_lig_props_for_component(i as u8);
                }
                ctx.output_glyph_for_component(g, class);
            }

            ctx.buffer.skip_glyph();
            true
        }
    }
}

fn alternate(ctx: &mut ApplyContext, data: &[u8]) -> bool {
    let glyph = ctx.buffer.cur(0).id as u16;
    if read_u16_be(data, 0) != Some(1) {
        return false;
    }
    let Some(cov) = ctx.coverage(data, 2) else { return false };
    let Some(set) = coverage_indexed_offset(&cov, data, glyph) else { return false };
    let Some(count) = read_u16_be(set, 0) else { return false };
    if count == 0 {
        return false;
    }
    let Some(rest) = set.get(2..) else { return false };
    let alternates = U16Array { data: rest, len: count };

    let glyph_mask = ctx.buffer.cur(0).mask;
    let shift = ctx.lookup_mask().trailing_zeros();
    let mut alt_index = (ctx.lookup_mask() & glyph_mask) >> shift;

    if alt_index == MAX_VALUE && ctx.random {
        let len = ctx.buffer.len;
        ctx.buffer.unsafe_to_break(0, len);
        alt_index = ctx.random_number() % u32::from(count) + 1;
    }

    let Ok(alt_index) = u16::try_from(alt_index) else { return false };
    let Some(index) = alt_index.checked_sub(1) else { return false };
    let Some(g) = alternates.get(index) else { return false };

    ctx.replace_glyph(g);
    true
}

fn ligature(ctx: &mut ApplyContext, data: &[u8]) -> bool {
    let glyph = ctx.buffer.cur(0).id as u16;
    if read_u16_be(data, 0) != Some(1) {
        return false;
    }
    let Some(cov) = ctx.coverage(data, 2) else { return false };
    let Some(set) = coverage_indexed_offset(&cov, data, glyph) else { return false };
    let Some(count) = read_u16_be(set, 0) else { return false };

    if count > 1 {
        let always = |_: u16, _: u16| true;
        let peek = {
            let mut iter = SkippingIterator::with_matching(ctx, ctx.buffer.idx, true, &always);
            iter.set_glyph_data(0);
            if iter.next(None) {
                let at = iter.index();
                let info = &ctx.buffer.info[at];
                (iter.may_skip(info) == MaySkip::No).then(|| (info.id as u16, at + 1))
            } else {
                None
            }
        };

        if let Some((second, unsafe_to)) = peek {
            let start = ctx.buffer.idx;
            let mut unsafe_to_concat = false;
            for i in 0..count {
                let Some(off) = read_u16_be(set, 2 + i as usize * 2) else { break };
                let at = off as usize;
                let components = read_u16_be(set, at + 2).unwrap_or(0);
                if components <= 1 || read_u16_be(set, at + 4) == Some(second) {
                    let Some(lig) = set.get(at..) else { continue };
                    if apply_ligature(ctx, lig) {
                        if unsafe_to_concat {
                            ctx.buffer.unsafe_to_concat(start, unsafe_to);
                        }
                        return true;
                    }
                } else if components > 1 {
                    unsafe_to_concat = true;
                }
            }
            if unsafe_to_concat {
                ctx.buffer.unsafe_to_concat(start, unsafe_to);
            }
            return false;
        }
    }

    for i in 0..count {
        let Some(off) = read_u16_be(set, 2 + i as usize * 2) else { break };
        let Some(lig) = set.get(off as usize..) else { continue };
        if apply_ligature(ctx, lig) {
            return true;
        }
    }
    false
}

fn apply_ligature(ctx: &mut ApplyContext, lig: &[u8]) -> bool {
    let Some(h) = window::<4>(lig, 0) else { return false };
    let lig_glyph = u16::from_be_bytes([h[0], h[1]]);
    let component_count = u16::from_be_bytes([h[2], h[3]]);
    if component_count == 0 {
        return false;
    }

    let input_len = component_count - 1;
    if input_len == 0 {
        ctx.replace_glyph(lig_glyph);
        return true;
    }

    let Some(rest) = lig.get(4..) else { return false };
    let components = U16Array { data: rest, len: input_len };
    let f = |g: u16, i: u16| components.get(i) == Some(g);

    let mut match_end = 0;
    let mut positions = new_match_positions();
    let mut total_component_count = 0;

    if !match_input(
        ctx,
        input_len,
        &f,
        &mut match_end,
        &mut positions,
        Some(&mut total_component_count),
    ) {
        let idx = ctx.buffer.idx;
        ctx.buffer.unsafe_to_concat(idx, match_end);
        return false;
    }

    ligate_input(
        ctx,
        usize::from(component_count),
        &positions,
        match_end,
        total_component_count,
        lig_glyph,
    );
    true
}

fn reverse_chain_single(ctx: &mut ApplyContext, data: &[u8]) -> bool {
    if read_u16_be(data, 0) != Some(1) {
        return false;
    }
    let Some(coverage) = ctx.coverage(data, 2) else { return false };
    let Some(index) = coverage.index_of(ctx.buffer.cur(0).id as u16) else { return false };

    if ctx.nesting_level_left != MAX_NESTING_LEVEL {
        return false;
    }

    let Some(back_count) = read_u16_be(data, 4) else { return false };
    let back = CoverageList { data, at: 6, len: back_count };

    let at = 6 + back_count as usize * 2;
    let Some(ahead_count) = read_u16_be(data, at) else { return false };
    let ahead = CoverageList { data, at: at + 2, len: ahead_count };

    let at = at + 2 + ahead_count as usize * 2;
    let Some(subst_count) = read_u16_be(data, at) else { return false };
    let Some(rest) = data.get(at + 2..) else { return false };
    let Some(subst) = (U16Array { data: rest, len: subst_count }).get(index) else {
        return false;
    };

    let mut start_index = 0;
    let mut end_index = 0;

    if match_backtrack(ctx, back_count, &|g, i| back.contains(i, g), &mut start_index) {
        let after = ctx.buffer.idx + 1;
        if match_lookahead(ctx, ahead_count, &|g, i| ahead.contains(i, g), after, &mut end_index) {
            ctx.buffer.unsafe_to_break_from_outbuffer(start_index, end_index);
            ctx.replace_glyph_inplace(subst);
            return true;
        }
    }

    ctx.buffer.unsafe_to_concat_from_outbuffer(start_index, end_index);
    false
}

fn coverage_indexed_offset<'d>(coverage: &Coverage, data: &'d [u8], glyph: u16) -> Option<&'d [u8]> {
    let index = coverage.index_of(glyph)?;
    if index >= read_u16_be(data, 4)? {
        return None;
    }
    let off = read_u16_be(data, 6 + index as usize * 2)? as usize;
    if off == 0 {
        return None;
    }
    data.get(off..)
}

pub(crate) fn would_apply(ctx: &WouldApplyContext, table: &LayoutTable, index: u16) -> bool {
    if ctx.glyphs.is_empty() {
        return false;
    }
    let Some(lookup) = table.lookup(index) else { return false };

    (0..lookup.subtable_count).any(|i| {
        lookup
            .subtable(i)
            .and_then(|data| resolve_extension(data, lookup.kind, EXTENSION))
            .is_some_and(|(kind, data)| would_apply_subtable(ctx, kind, data))
    })
}

pub(crate) struct WouldApplyContext<'a> {
    pub(crate) glyphs: &'a [u16],
    pub(crate) zero_context: bool,
}

fn would_apply_subtable(ctx: &WouldApplyContext, kind: u16, data: &[u8]) -> bool {
    let first = ctx.glyphs[0];
    match kind {
        SINGLE | MULTIPLE | ALTERNATE | REVERSE_CHAIN_SINGLE => {
            ctx.glyphs.len() == 1
                && offset_coverage(data, 2).and_then(|c| c.index_of(first)).is_some()
        }
        LIGATURE => would_apply_ligature(ctx, data),
        CONTEXT => would_apply_context(ctx, data),
        CHAIN_CONTEXT => would_apply_chain_context(ctx, data),
        _ => false,
    }
}

fn would_apply_ligature(ctx: &WouldApplyContext, data: &[u8]) -> bool {
    if read_u16_be(data, 0) != Some(1) {
        return false;
    }
    let Some(cov) = offset_coverage(data, 2) else { return false };
    let Some(set) = coverage_indexed_offset(&cov, data, ctx.glyphs[0]) else { return false };
    let Some(count) = read_u16_be(set, 0) else { return false };

    (0..count).any(|i| {
        let Some(off) = read_u16_be(set, 2 + i as usize * 2) else { return false };
        let Some(lig) = set.get(off as usize..) else { return false };
        let Some(component_count) = read_u16_be(lig, 2) else { return false };
        let Some(input_len) = component_count.checked_sub(1) else { return false };
        if ctx.glyphs.len() != input_len as usize + 1 {
            return false;
        }
        let Some(rest) = lig.get(4..) else { return false };
        let components = U16Array { data: rest, len: input_len };
        (0..input_len).all(|i| components.get(i) == Some(ctx.glyphs[i as usize + 1]))
    })
}

fn rule_input_matches(ctx: &WouldApplyContext, rule: &[u8], value_of: impl Fn(u16) -> u16) -> bool {
    let Some(glyph_count) = read_u16_be(rule, 0) else { return false };
    let Some(input_len) = glyph_count.checked_sub(1) else { return false };
    if ctx.glyphs.len() != input_len as usize + 1 {
        return false;
    }
    let Some(rest) = rule.get(4..) else { return false };
    let input = U16Array { data: rest, len: input_len };
    (0..input_len).all(|i| input.get(i) == Some(value_of(ctx.glyphs[i as usize + 1])))
}

fn any_rule_in_set(set: &[u8], mut matches: impl FnMut(&[u8]) -> bool) -> bool {
    let Some(count) = read_u16_be(set, 0) else { return false };
    (0..count).any(|i| {
        read_u16_be(set, 2 + i as usize * 2)
            .and_then(|off| set.get(off as usize..))
            .is_some_and(&mut matches)
    })
}

fn would_apply_context(ctx: &WouldApplyContext, data: &[u8]) -> bool {
    let first = ctx.glyphs[0];
    match read_u16_be(data, 0) {
        Some(1) => offset_coverage(data, 2).and_then(|c| coverage_indexed_offset(&c, data, first))
            .is_some_and(|set| any_rule_in_set(set, |rule| rule_input_matches(ctx, rule, |g| g))),
        Some(2) => {
            let Some(classes) = offset_class_def(data, 4) else { return false };
            if offset_coverage(data, 2).and_then(|c| c.index_of(first)).is_none() {
                return false;
            }
            let set_index = classes.class_of(first) as usize;
            let Some(count) = read_u16_be(data, 6) else { return false };
            if set_index >= count as usize {
                return false;
            }
            let Some(off) = read_u16_be(data, 8 + set_index * 2) else { return false };
            if off == 0 {
                return false;
            }
            let Some(set) = data.get(off as usize..) else { return false };
            any_rule_in_set(set, |rule| rule_input_matches(ctx, rule, |g| classes.class_of(g)))
        }
        Some(3) => {
            let Some(glyph_count) = read_u16_be(data, 2) else { return false };
            if glyph_count == 0 || ctx.glyphs.len() != glyph_count as usize {
                return false;
            }
            (0..glyph_count).all(|i| {
                offset_coverage(data, 6 + i as usize * 2)
                    .and_then(|c| c.index_of(ctx.glyphs[i as usize]))
                    .is_some()
            })
        }
        _ => false,
    }
}

fn would_apply_chain_context(ctx: &WouldApplyContext, data: &[u8]) -> bool {
    let first = ctx.glyphs[0];
    match read_u16_be(data, 0) {
        Some(1) => offset_coverage(data, 2).and_then(|c| coverage_indexed_offset(&c, data, first)).is_some_and(|set| {
            any_rule_in_set(set, |rule| chain_rule_matches(ctx, rule, |g| g))
        }),
        Some(2) => {
            let Some(input_classes) = offset_class_def(data, 6) else { return false };
            if offset_coverage(data, 2).and_then(|c| c.index_of(first)).is_none() {
                return false;
            }
            let set_index = input_classes.class_of(first) as usize;
            let Some(count) = read_u16_be(data, 10) else { return false };
            if set_index >= count as usize {
                return false;
            }
            let Some(off) = read_u16_be(data, 12 + set_index * 2) else { return false };
            if off == 0 {
                return false;
            }
            let Some(set) = data.get(off as usize..) else { return false };
            any_rule_in_set(set, |rule| {
                chain_rule_matches(ctx, rule, |g| input_classes.class_of(g))
            })
        }
        Some(3) => {
            let Some(backtrack_count) = read_u16_be(data, 2) else { return false };
            let input_at = 4 + backtrack_count as usize * 2;
            let Some(input_count) = read_u16_be(data, input_at) else { return false };
            let lookahead_at = input_at + 2 + input_count as usize * 2;
            let Some(lookahead_count) = read_u16_be(data, lookahead_at) else { return false };

            if ctx.zero_context && (backtrack_count != 0 || lookahead_count != 0) {
                return false;
            }
            if input_count == 0 || ctx.glyphs.len() != input_count as usize {
                return false;
            }
            (0..input_count).all(|i| {
                offset_coverage(data, input_at + 2 + i as usize * 2)
                    .and_then(|c| c.index_of(ctx.glyphs[i as usize]))
                    .is_some()
            })
        }
        _ => false,
    }
}

fn chain_rule_matches(
    ctx: &WouldApplyContext,
    rule: &[u8],
    value_of: impl Fn(u16) -> u16,
) -> bool {
    let Some(backtrack_count) = read_u16_be(rule, 0) else { return false };
    let input_at = 2 + backtrack_count as usize * 2;
    let Some(glyph_count) = read_u16_be(rule, input_at) else { return false };
    let Some(input_len) = glyph_count.checked_sub(1) else { return false };
    let lookahead_at = input_at + 2 + input_len as usize * 2;
    let Some(lookahead_count) = read_u16_be(rule, lookahead_at) else { return false };

    if ctx.zero_context && (backtrack_count != 0 || lookahead_count != 0) {
        return false;
    }
    if ctx.glyphs.len() != input_len as usize + 1 {
        return false;
    }

    let Some(rest) = rule.get(input_at + 2..) else { return false };
    let input = U16Array { data: rest, len: input_len };
    (0..input_len).all(|i| input.get(i) == Some(value_of(ctx.glyphs[i as usize + 1])))
}
