use crate::daecore::daetype::format::aat::{class, state, Entry, Lookup, StateTable};
use crate::daecore::daeshaper::buffer::{Buffer, Direction};
use crate::daecore::daetype::decoder::{read_u16_be, read_u32_be, window};

// A deleted component is not removed on the spot but set to this, which is a *class* the following
// subtables can still match on, and swept away once the whole table has run. Deletion is deferred
// because the format needs it to be.
pub(crate) const DELETED_GLYPH: u32 = 0xFFFF;

mod coverage {
    pub(super) const VERTICAL: u32 = 0x8000_0000;
    pub(super) const DESCENDING: u32 = 0x4000_0000;
    pub(super) const ALL_DIRECTIONS: u32 = 0x2000_0000;
    pub(super) const LOGICAL: u32 = 0x1000_0000;
    pub(super) const TYPE: u32 = 0x0000_00FF;
}

mod entry_flags {
    pub(super) const REARRANGE_VERB: u16 = 0x000F;
    pub(super) const MARK_LAST: u16 = 0x2000;
    pub(super) const MARK_FIRST: u16 = 0x8000;

    pub(super) const SET_MARK: u16 = 0x8000;
    pub(super) const DONT_ADVANCE: u16 = 0x4000;

    pub(super) const SET_COMPONENT: u16 = 0x8000;
    pub(super) const PERFORM_ACTION: u16 = 0x2000;

    pub(super) const CURRENT_INSERT_BEFORE: u16 = 0x0800;
    pub(super) const MARKED_INSERT_BEFORE: u16 = 0x0400;
    pub(super) const CURRENT_INSERT_COUNT: u16 = 0x03E0;
    pub(super) const MARKED_INSERT_COUNT: u16 = 0x001F;
}

mod lig_action {
    pub(super) const LAST: u32 = 0x8000_0000;
    pub(super) const STORE: u32 = 0x4000_0000;
    pub(super) const OFFSET: u32 = 0x3FFF_FFFF;
    pub(super) const SIGN: u32 = 0x2000_0000;
}

pub(crate) fn apply(table: &[u8], buffer: &mut Buffer, num_glyphs: u16) {
    let Some(n_chains) = read_u32_be(table, 4) else { return };

    let mut at = 8usize;
    for _ in 0..n_chains {
        let Some(h) = window::<16>(table, at) else { break };
        let w = |i: usize| u32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
        let (default_flags, length, n_features, n_subtables) = (w(0), w(4), w(8), w(12));
        let Some(chain) = at.checked_add(length as usize).and_then(|e| table.get(at..e)) else {
            break;
        };

        let flags = default_flags;

        let Some(mut sub_at) = (n_features as usize).checked_mul(12).and_then(|n| n.checked_add(16))
        else {
            break;
        };
        for _ in 0..n_subtables {
            let Some(h) = window::<12>(chain, sub_at) else { break };
            let w = |i: usize| u32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
            let (sub_len, cov, sub_flags) = (w(0), w(4), w(8));
            let Some(body) = sub_at
                .checked_add(12)
                .zip(sub_at.checked_add(sub_len as usize))
                .and_then(|(b, e)| chain.get(b..e))
            else {
                break;
            };

            if sub_flags & flags != 0 {
                apply_subtable(body, cov, buffer, num_glyphs);
            }

            let Some(next) = sub_at.checked_add(sub_len as usize) else { break };
            if sub_len == 0 || next > chain.len() {
                break;
            }
            sub_at = next;
        }

        let Some(next) = at.checked_add(length as usize) else { break };
        if length == 0 || next > table.len() {
            break;
        }
        at = next;
    }

}

pub(crate) fn remove_deleted_glyphs(buffer: &mut Buffer) {
    buffer.delete_glyphs_inplace(|info| info.id == DELETED_GLYPH);
}

fn apply_subtable(body: &[u8], cov: u32, buffer: &mut Buffer, num_glyphs: u16) {
    let vertical = matches!(buffer.direction, Direction::TopToBottom | Direction::BottomToTop);

    if cov & coverage::ALL_DIRECTIONS == 0 && vertical != (cov & coverage::VERTICAL != 0) {
        return;
    }

    let backward = matches!(buffer.direction, Direction::RightToLeft | Direction::BottomToTop);
    let descending = cov & coverage::DESCENDING != 0;
    let reverse =
        if cov & coverage::LOGICAL != 0 { descending } else { descending != backward };

    if reverse {
        buffer.reverse();
    }
    match cov & coverage::TYPE {
        0 => rearrangement(body, buffer, num_glyphs),
        1 => contextual(body, buffer, num_glyphs),
        2 => ligature(body, buffer, num_glyphs),
        4 => non_contextual(body, buffer, num_glyphs),
        5 => insertion(body, buffer, num_glyphs),
        _ => {}
    }
    if reverse {
        buffer.reverse();
    }
}

mod actionable {
    use super::{entry_flags, Entry};

    pub(super) fn rearrangement(e: &Entry) -> bool {
        e.flags & entry_flags::REARRANGE_VERB != 0
    }
    pub(super) fn contextual(e: &Entry) -> bool {
        e.word1 != 0xFFFF || e.word2 != 0xFFFF
    }
    pub(super) fn ligature(e: &Entry) -> bool {
        e.flags & entry_flags::PERFORM_ACTION != 0
    }
    pub(super) fn insertion(e: &Entry) -> bool {
        e.flags & (entry_flags::CURRENT_INSERT_COUNT | entry_flags::MARKED_INSERT_COUNT) != 0
            && (e.word1 != 0xFFFF || e.word2 != 0xFFFF)
    }
}

fn can_advance(e: &Entry) -> bool {
    e.flags & entry_flags::DONT_ADVANCE == 0
}

const INITIATES_ACTION: u16 = 0x8000;

fn machine_can_act(table: &StateTable, buffer: &Buffer, actionable: fn(&Entry) -> bool) -> bool {
    let interesting = |klass: u16| {
        table.entry(state::START_OF_TEXT, klass).is_none_or(|e| {
            e.new_state != state::START_OF_TEXT
                || e.flags & INITIATES_ACTION != 0
                || actionable(&e)
        })
    };
    buffer.info[..buffer.len].iter().any(|info| {
        let klass = if info.id == DELETED_GLYPH {
            class::DELETED_GLYPH
        } else {
            table.class(info.id as u16)
        };
        interesting(klass)
    })
}

fn collect_start_end_safe(table: &StateTable, actionable: fn(&Entry) -> bool) -> u64 {
    let mut mask = 0u64;
    for s in 0..64u16 {
        if table.entry(s, class::END_OF_TEXT).is_none_or(|e| !actionable(&e)) {
            mask |= 1u64 << s;
        }
    }
    mask
}

fn safe_to_break(
    table: &StateTable,
    state: u16,
    klass: u16,
    entry: &Entry,
    start_end_safe: u64,
    actionable: fn(&Entry) -> bool,
) -> bool {
    if actionable(entry) {
        return false;
    }
    let restartable = state == state::START_OF_TEXT
        || (!can_advance(entry) && entry.new_state == state::START_OF_TEXT)
        || table.entry(state::START_OF_TEXT, klass).is_some_and(|w| {
            !actionable(&w)
                && entry.new_state == w.new_state
                && can_advance(entry) == can_advance(&w)
        });
    if !restartable {
        return false;
    }
    if state < 64 {
        start_end_safe & (1u64 << state) != 0
    } else {
        table.entry(state, class::END_OF_TEXT).is_some_and(|e| !actionable(&e))
    }
}

fn drive<F>(table: &StateTable, buffer: &mut Buffer, actionable: fn(&Entry) -> bool, mut step: F)
where
    F: FnMut(&mut Buffer, usize, &Entry),
{
    let start_end_safe = collect_start_end_safe(table, actionable);
    let mut current = state::START_OF_TEXT;
    let mut i = 0usize;
    let mut memo_key = u32::MAX;
    let mut memo_entry = Entry::default();

    loop {
        let klass = if i < buffer.len {
            let glyph = buffer.info[i].id;
            if glyph == DELETED_GLYPH {
                class::DELETED_GLYPH
            } else {
                table.class(glyph as u16)
            }
        } else {
            class::END_OF_TEXT
        };

        let key = u32::from(current) << 16 | u32::from(klass);
        if key != memo_key {
            let Some(e) = table.entry(current, klass) else { return };
            memo_key = key;
            memo_entry = e;
        }
        let entry = memo_entry;
        if i > 0
            && i < buffer.len
            && !safe_to_break(table, current, klass, &entry, start_end_safe, actionable)
        {
            buffer.unsafe_to_break_from_outbuffer(i - 1, i + 1);
        }
        step(buffer, i, &entry);
        current = entry.new_state;

        if i >= buffer.len {
            return;
        }
        if entry.flags & entry_flags::DONT_ADVANCE == 0 {
            i += 1;
        } else {
            if buffer.max_ops <= 0 {
                i += 1;
            }
            buffer.max_ops -= 1;
        }
    }
}

fn rearrangement(body: &[u8], buffer: &mut Buffer, num_glyphs: u16) {
    let Some(table) = StateTable::parse(body, 0, num_glyphs) else { return };
    if !machine_can_act(&table, buffer, actionable::rearrangement) {
        return;
    }

    const VERBS: [(usize, usize, bool, bool); 16] = [
        (0, 0, false, false),
        (1, 0, false, false),
        (0, 1, false, false),
        (1, 1, false, false),
        (2, 0, false, false),
        (2, 0, true, false),
        (0, 2, false, false),
        (0, 2, false, true),
        (1, 2, false, false),
        (1, 2, false, true),
        (2, 1, false, false),
        (2, 1, true, false),
        (2, 2, false, false),
        (2, 2, true, false),
        (2, 2, false, true),
        (2, 2, true, true),
    ];

    let mut span_start = 0usize;
    let mut span_end = 0usize;

    drive(&table, buffer, actionable::rearrangement, |buffer, i, entry| {
        if entry.flags & entry_flags::MARK_FIRST != 0 {
            span_start = i;
        }
        if entry.flags & entry_flags::MARK_LAST != 0 {
            span_end = (i + 1).min(buffer.len);
        }

        let verb = usize::from(entry.flags & entry_flags::REARRANGE_VERB);
        if verb == 0 || span_start >= span_end {
            return;
        }
        let (l, r, rev_l, rev_r) = VERBS[verb];
        if span_end - span_start < l + r {
            return;
        }

        buffer.merge_clusters(span_start, (i + 1).min(buffer.len));

        let span: alloc::vec::Vec<_> = buffer.info[span_start..span_end].to_vec();
        let mut front = span[..l].to_vec();
        let mut back = span[span.len() - r..].to_vec();
        if rev_l {
            front.reverse();
        }
        if rev_r {
            back.reverse();
        }

        let mut out = back;
        out.extend_from_slice(&span[l..span.len() - r]);
        out.extend_from_slice(&front);
        buffer.info[span_start..span_end].copy_from_slice(&out);
    });
}

fn contextual(body: &[u8], buffer: &mut Buffer, num_glyphs: u16) {
    let Some(table) = StateTable::parse(body, 2, num_glyphs) else { return };
    if !machine_can_act(&table, buffer, actionable::contextual) {
        return;
    }
    let Some(subst_off) = read_u32_be(body, 16) else { return };
    let Some(substitutions) = body.get(subst_off as usize..) else { return };

    let mut memo: [Option<(u16, Lookup<'_>)>; 2] = [None, None];

    let mut mark = 0usize;
    let mut mark_set = false;

    drive(&table, buffer, actionable::contextual, |buffer, i, entry| {
        if i >= buffer.len && !mark_set {
            return;
        }

        if entry.word1 != 0xFFFF && mark < buffer.len {
            let Some(l) = resolve(substitutions, num_glyphs, &mut memo, entry.word1) else { return };
            if let Some(g) = l.value(buffer.info[mark].id as u16) {
                buffer.info[mark].id = u32::from(g);
            }
        }
        if entry.word2 != 0xFFFF && buffer.len > 0 {
            let at = i.min(buffer.len - 1);
            let Some(l) = resolve(substitutions, num_glyphs, &mut memo, entry.word2) else { return };
            if let Some(g) = l.value(buffer.info[at].id as u16) {
                buffer.info[at].id = u32::from(g);
            }
        }
        if entry.flags & entry_flags::SET_MARK != 0 {
            mark = i;
            mark_set = true;
        }
    });
}

fn resolve<'a>(
    substitutions: &'a [u8],
    num_glyphs: u16,
    memo: &mut [Option<(u16, Lookup<'a>)>; 2],
    index: u16,
) -> Option<Lookup<'a>> {
    if let Some((at, l)) = memo[0]
        && at == index {
            return Some(l);
        }
    if let Some((at, l)) = memo[1]
        && at == index {
            memo.swap(0, 1);
            return Some(l);
        }
    let off = read_u32_be(substitutions, 4 * usize::from(index))? as usize;
    let l = Lookup::parse(substitutions.get(off..)?, num_glyphs)?;
    memo[1] = memo[0];
    memo[0] = Some((index, l));
    Some(l)
}

fn ligature(body: &[u8], buffer: &mut Buffer, num_glyphs: u16) {
    let Some(table) = StateTable::parse(body, 1, num_glyphs) else { return };
    if !machine_can_act(&table, buffer, actionable::ligature) {
        return;
    }
    let Some(h) = window::<12>(body, 16) else { return };
    let w = |i: usize| u32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    let (action_off, component_off, ligature_off) = (w(0), w(4), w(8));

    let actions = body.get(action_off as usize..).unwrap_or_default();
    let components = body.get(component_off as usize..).unwrap_or_default();
    let ligatures = body.get(ligature_off as usize..).unwrap_or_default();

    const MAX_COMPONENTS: usize = 64;
    let mut marks: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    let mut staged: alloc::vec::Vec<(usize, u32)> = alloc::vec::Vec::new();

    drive(&table, buffer, actionable::ligature, |buffer, i, entry| {
        if entry.flags & entry_flags::SET_COMPONENT != 0 {
            if marks.last() == Some(&i) {
                marks.pop();
            }
            if marks.len() >= MAX_COMPONENTS {
                marks.clear();
            }
            marks.push(i);
        }

        if entry.flags & entry_flags::PERFORM_ACTION == 0 || marks.is_empty() || i >= buffer.len {
            return;
        }

        let mut action_idx = usize::from(entry.word1);
        let mut ligature_idx = 0u32;
        staged.clear();
        let mut completed = false;

        while let Some(pos) = marks.pop() {
            let Some(action) = read_u32_be(actions, 4 * action_idx) else { break };

            let mut offset = action & lig_action::OFFSET;
            if offset & lig_action::SIGN != 0 {
                offset |= !lig_action::OFFSET;
            }
            let component_idx = buffer.info[pos].id.wrapping_add(offset);
            let Some(at) = usize::try_from(component_idx).ok().and_then(|c| c.checked_mul(2)) else {
                break;
            };
            let Some(contribution) = read_u16_be(components, at) else { break };
            ligature_idx = ligature_idx.wrapping_add(u32::from(contribution));

            if action & (lig_action::LAST | lig_action::STORE) != 0 {
                let Some(at) = usize::try_from(ligature_idx).ok().and_then(|l| l.checked_mul(2))
                else {
                    break;
                };
                let Some(lig) = read_u16_be(ligatures, at) else { break };
                staged.push((pos, u32::from(lig)));
                ligature_idx = 0;
            } else {
                staged.push((pos, DELETED_GLYPH));
            }

            action_idx += 1;
            if action & lig_action::LAST != 0 {
                completed = true;
                break;
            }
        }

        if completed {
            let first = staged.iter().map(|(p, _)| *p).min().unwrap_or(i);
            buffer.merge_clusters(first, (i + 1).min(buffer.len));
            for &(pos, glyph) in &staged {
                buffer.info[pos].id = glyph;
            }
        }
        marks.clear();
    });
}

fn non_contextual(body: &[u8], buffer: &mut Buffer, num_glyphs: u16) {
    let Some(lookup) = Lookup::parse(body, num_glyphs) else { return };
    for i in 0..buffer.len {
        let glyph = buffer.info[i].id;
        if glyph == DELETED_GLYPH {
            continue;
        }
        if let Some(g) = lookup.value(glyph as u16) {
            buffer.info[i].id = u32::from(g);
        }
    }
}

fn insertion(body: &[u8], buffer: &mut Buffer, num_glyphs: u16) {
    let Some(table) = StateTable::parse(body, 2, num_glyphs) else { return };
    if !machine_can_act(&table, buffer, actionable::insertion) {
        return;
    }
    let Some(action_off) = read_u32_be(body, 16) else { return };
    let actions = body.get(action_off as usize..).unwrap_or_default();

    let start_end_safe = collect_start_end_safe(&table, actionable::insertion);

    buffer.clear_output();
    let mut mark = 0usize;
    let mut current = state::START_OF_TEXT;

    loop {
        let klass = if buffer.idx < buffer.len {
            let glyph = buffer.info[buffer.idx].id;
            if glyph == DELETED_GLYPH { class::DELETED_GLYPH } else { table.class(glyph as u16) }
        } else {
            class::END_OF_TEXT
        };
        let Some(entry) = table.entry(current, klass) else { break };

        if buffer.out_len > 0
            && buffer.idx < buffer.len
            && !safe_to_break(&table, current, klass, &entry, start_end_safe, actionable::insertion)
        {
            buffer.unsafe_to_break_from_outbuffer(buffer.out_len - 1, buffer.idx + 1);
        }

        let mark_loc = buffer.out_len;

        if entry.word2 != 0xFFFF {
            let count = usize::from(entry.flags & entry_flags::MARKED_INSERT_COUNT);
            let before = entry.flags & entry_flags::MARKED_INSERT_BEFORE != 0;
            let end = buffer.out_len;
            if !buffer.move_to(mark) {
                break;
            }
            if buffer.idx < buffer.len && !before {
                let info = buffer.info[buffer.idx];
                buffer.output_info(info);
            }
            let mut inserted = 0usize;
            for k in 0..count {
                let Some(g) = read_u16_be(actions, 2 * (usize::from(entry.word2) + k)) else { break };
                buffer.output_glyph(u32::from(g));
                inserted += 1;
            }
            if buffer.idx < buffer.len && !before {
                buffer.skip_glyph();
            }
            if !buffer.move_to(end + inserted) {
                break;
            }
        }

        if entry.flags & entry_flags::SET_MARK != 0 {
            mark = mark_loc;
        }

        if entry.word1 != 0xFFFF {
            let count = usize::from((entry.flags & entry_flags::CURRENT_INSERT_COUNT) >> 5);
            let before = entry.flags & entry_flags::CURRENT_INSERT_BEFORE != 0;
            let end = buffer.out_len;
            if buffer.idx < buffer.len && !before {
                let info = buffer.info[buffer.idx];
                buffer.output_info(info);
            }
            let mut inserted = 0usize;
            for k in 0..count {
                let Some(g) = read_u16_be(actions, 2 * (usize::from(entry.word1) + k)) else { break };
                buffer.output_glyph(u32::from(g));
                inserted += 1;
            }
            if buffer.idx < buffer.len && !before {
                buffer.skip_glyph();
            }
            let to = if entry.flags & entry_flags::DONT_ADVANCE != 0 { end } else { end + inserted };
            if !buffer.move_to(to) {
                break;
            }
        }

        current = entry.new_state;
        if buffer.idx >= buffer.len {
            break;
        }
        if entry.flags & entry_flags::DONT_ADVANCE == 0 {
            buffer.next_glyph();
        } else {
            if buffer.max_ops <= 0 {
                buffer.next_glyph();
            }
            buffer.max_ops -= 1;
        }
    }

    while buffer.idx < buffer.len {
        buffer.next_glyph();
    }
    buffer.sync();
}
