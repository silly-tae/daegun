use super::apply::{ApplyContext, SkippingIterator};
use super::gpos::attach_type;
use super::kern::{apply_pair, chain_for_cross_stream};
use super::lookup_flags;
use super::map::TableIndex;
use super::morx::DELETED_GLYPH;
use crate::daecore::daeshaper::buffer::{scratch_flags, Buffer, Mask};
use crate::daecore::daeshaper::face::Face;
use crate::daecore::daetype::decoder::{read_i16_be, read_u16_be, read_u32_be, window};
use crate::daecore::daetype::format::aat::{class, state, Lookup, StateTable};
use crate::daecore::daetype::format::ankr::{control_point, Ankr};

const HEADER_LEN: usize = 12;

const KERN_STACK: usize = 8;

const NO_ACTION: u16 = 0xFFFF;

mod entry_flags {
    pub(super) const PUSH: u16 = 0x8000;
    pub(super) const MARK: u16 = 0x8000;
    pub(super) const DONT_ADVANCE: u16 = 0x4000;
    pub(super) const RESET: u16 = 0x2000;
}

mod coverage {
    pub(super) const VERTICAL: u32 = 0x8000_0000;
    pub(super) const CROSS_STREAM: u32 = 0x4000_0000;
    pub(super) const VARIABLE: u32 = 0x2000_0000;
    pub(super) const BACKWARDS: u32 = 0x1000_0000;
}

#[derive(Clone, Copy)]
pub(super) struct Subtable<'a> {
    pub(super) data: &'a [u8],
    pub(super) format: u8,
    pub(super) vertical: bool,
    pub(super) cross_stream: bool,
    variable: bool,
    pub(super) backwards: bool,
    tuple_count: u32,
}

fn subtables(table: &[u8]) -> Option<(usize, u32)> {
    let version = read_u16_be(table, 0)?;
    if version > 3 {
        return None;
    }
    Some((8, read_u32_be(table, 4)?))
}

fn parse_subtable(table: &[u8], at: usize) -> Option<(Subtable<'_>, usize)> {
    let h = window::<12>(table, at)?;
    let length = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as usize;
    let cov = u32::from_be_bytes([h[4], h[5], h[6], h[7]]);
    let tuple_count = u32::from_be_bytes([h[8], h[9], h[10], h[11]]);

    if length < HEADER_LEN {
        return None;
    }
    let end = at.checked_add(length)?;
    let sub = Subtable {
        data: table.get(at..end.min(table.len()))?,
        format: (cov & 0xFF) as u8,
        vertical: cov & coverage::VERTICAL != 0,
        cross_stream: cov & coverage::CROSS_STREAM != 0,
        variable: cov & coverage::VARIABLE != 0,
        backwards: cov & coverage::BACKWARDS != 0,
        tuple_count,
    };
    Some((sub, end))
}

impl Subtable<'_> {
    fn readable(&self) -> bool {
        !self.variable && self.tuple_count == 0 && matches!(self.format, 0 | 1 | 2 | 4 | 6)
    }

    fn kerning(&self, left: u16, right: u16, num_glyphs: u16) -> i32 {
        match self.format {
            0 => self.format0(left, right),
            2 => self.format2(left, right, num_glyphs),
            6 => self.format6(left, right, num_glyphs),
            _ => 0,
        }
    }

    fn format0(&self, left: u16, right: u16) -> i32 {
        let body = HEADER_LEN;
        let Some(count) = read_u32_be(self.data, body) else { return 0 };
        let pairs = body + 16;
        let fits = self.data.len().saturating_sub(pairs) / 6;
        let count = (count as usize).min(fits);

        let want = (u32::from(left) << 16) | u32::from(right);
        let (mut lo, mut hi) = (0usize, count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let at = pairs + mid * 6;
            let Some(pair) = read_u32_be(self.data, at) else { return 0 };
            match pair.cmp(&want) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => {
                    return i32::from(read_i16_be(self.data, at + 4).unwrap_or(0))
                }
            }
        }
        0
    }

    fn format2(&self, left: u16, right: u16, num_glyphs: u16) -> i32 {
        let body = HEADER_LEN;
        let left_at = read_u32_be(self.data, body + 4).unwrap_or(0) as usize;
        let right_at = read_u32_be(self.data, body + 8).unwrap_or(0) as usize;
        let array_at = read_u32_be(self.data, body + 12).unwrap_or(0) as usize;

        let class = |off: usize, glyph: u16| -> usize {
            self.data
                .get(off..)
                .and_then(|d| Lookup::parse(d, num_glyphs))
                .and_then(|l| l.value(glyph))
                .map_or(0, usize::from)
        };

        // An element index, not a byte offset – which is where `kerx` and `kern` differ while looking
        // identical on the wire. `kern` format 2 stores byte offsets into its array; here the sum is
        // scaled by the element width. Copying the working `kern` reader would be wrong in a way no
        // malformed-input test could catch.
        let index = class(left_at, left) + class(right_at, right);
        let Some(at) = index.checked_mul(2).and_then(|o| array_at.checked_add(o)) else {
            return 0;
        };
        i32::from(read_i16_be(self.data, at).unwrap_or(0))
    }

    fn format6(&self, left: u16, right: u16, num_glyphs: u16) -> i32 {
        let body = HEADER_LEN;
        let flags = read_u32_be(self.data, body).unwrap_or(0);
        if flags & 1 != 0 {
            return 0;
        }
        let row_at = read_u32_be(self.data, body + 8).unwrap_or(0) as usize;
        let col_at = read_u32_be(self.data, body + 12).unwrap_or(0) as usize;
        let array_at = read_u32_be(self.data, body + 16).unwrap_or(0) as usize;

        let index = |off: usize, glyph: u16| -> usize {
            self.data
                .get(off..)
                .and_then(|d| Lookup::parse(d, num_glyphs))
                .and_then(|l| l.value(glyph))
                .map_or(0, usize::from)
        };

        let i = index(row_at, left) + index(col_at, right);
        let Some(at) = i.checked_mul(2).and_then(|o| array_at.checked_add(o)) else { return 0 };
        i32::from(read_i16_be(self.data, at).unwrap_or(0))
    }
}

pub(crate) fn is_usable(face: &Face) -> bool {
    let Some(table) = face.table("kerx") else { return false };
    let Some((mut at, count)) = subtables(table) else { return false };
    for _ in 0..count.min(MAX_SUBTABLES) {
        let Some((sub, next)) = parse_subtable(table, at) else { return false };
        if sub.readable() {
            return true;
        }
        at = next;
    }
    false
}

const MAX_SUBTABLES: u32 = 4096;

pub(crate) fn apply(face: &Face, buffer: &mut Buffer, kern_mask: Mask, requested: bool) {
    let Some(table) = face.table("kerx") else { return };
    let Some((mut at, count)) = subtables(table) else { return };
    let num_glyphs = face.num_glyphs();
    let ankr = face.table("ankr").and_then(|d| Ankr::parse(d, num_glyphs));

    let mut seen_cross_stream = false;
    let mut reversed = false;

    for _ in 0..count.min(MAX_SUBTABLES) {
        let Some((sub, next)) = parse_subtable(table, at) else { break };
        at = next;

        if !sub.readable() || buffer.direction.is_horizontal() == sub.vertical {
            continue;
        }

        let machine = matches!(sub.format, 1 | 4);
        if !requested && !machine && !sub.cross_stream {
            continue;
        }
        if !machine && sub.backwards {
            continue;
        }

        if !seen_cross_stream && sub.cross_stream {
            seen_cross_stream = true;
            chain_for_cross_stream(buffer);
        }

        let want_reversed = sub.backwards != buffer.direction.is_backward();
        if want_reversed != reversed {
            buffer.reverse();
            reversed = want_reversed;
        }

        match sub.format {
            1 => machine_kern(buffer, kern_mask, &sub, num_glyphs),
            4 => machine_attach(buffer, &sub, num_glyphs, ankr.as_ref(), reversed),
            _ => kern_pairs(face, buffer, kern_mask, &sub, num_glyphs),
        }
    }

    if reversed {
        buffer.reverse();
    }
}

fn machine_kern(
    buffer: &mut Buffer,
    kern_mask: Mask,
    sub: &Subtable,
    num_glyphs: u16,
) {
    let Some(body) = sub.data.get(HEADER_LEN..) else { return };
    let Some(table) = StateTable::parse(body, 1, num_glyphs) else { return };
    let Some(values_at) = read_u32_be(body, 16).map(|v| v as usize) else { return };

    let horizontal = buffer.direction.is_horizontal();
    let cross_stream = sub.cross_stream;
    let use_x = horizontal != cross_stream;

    let mut stack = [0usize; KERN_STACK];
    let mut depth = 0usize;
    let mut state = state::START_OF_TEXT;
    let mut idx = 0usize;

    loop {
        let class = if idx < buffer.len {
            table.class(buffer.info[idx].id as u16)
        } else {
            class::END_OF_TEXT
        };
        let Some(entry) = table.entry(state, class) else { break };

        if entry.flags & entry_flags::RESET != 0 {
            depth = 0;
        }
        if entry.flags & entry_flags::PUSH != 0 {
            if depth < stack.len() {
                stack[depth] = idx;
                depth += 1;
            } else {
                depth = 0;
            }
        }

        if entry.word1 != NO_ACTION && depth != 0 {
            let mut action = entry.word1 as usize;
            let mut last = false;
            while !last && depth != 0 {
                depth -= 1;
                let target = stack[depth];
                let Some(mut v) = read_i16_be(sub.data, values_at + action * 2).map(i32::from)
                else {
                    break;
                };
                action += 1;
                if target >= buffer.len {
                    continue;
                }
                last = v & 1 != 0;
                v &= !1;
                spend_machine_value(buffer, target, v, kern_mask, horizontal, cross_stream, use_x);
            }
        }

        state = entry.new_state;
        if idx >= buffer.len {
            break;
        }
        if entry.flags & entry_flags::DONT_ADVANCE == 0 || buffer.max_ops <= 0 {
            idx += 1;
        }
        buffer.max_ops -= 1;
    }
}

fn spend_machine_value(
    buffer: &mut Buffer,
    at: usize,
    v: i32,
    kern_mask: Mask,
    horizontal: bool,
    cross_stream: bool,
    use_x: bool,
) {
    let glyph_mask = buffer.info[at].mask;
    let pos = &mut buffer.pos[at];

    if cross_stream {
        if v == -0x8000 {
            pos.attach_type = 0;
            pos.attach_chain = 0;
            if use_x {
                pos.x_offset = 0;
            } else {
                pos.y_offset = 0;
            }
            return;
        }
        if pos.attach_type == 0 {
            return;
        }
        if use_x {
            pos.x_offset = pos.x_offset.saturating_add(v);
        } else {
            pos.y_offset = pos.y_offset.saturating_add(v);
        }
        buffer.scratch_flags |= scratch_flags::HAS_GPOS_ATTACHMENT;
        return;
    }

    if glyph_mask & kern_mask == 0 {
        return;
    }
    if horizontal {
        pos.x_advance = pos.x_advance.saturating_add(v);
        pos.x_offset = pos.x_offset.saturating_add(v);
    } else if pos.y_offset == 0 {
        pos.y_advance = pos.y_advance.saturating_add(v);
        pos.y_offset = pos.y_offset.saturating_add(v);
    }
}

fn machine_attach(
    buffer: &mut Buffer,
    sub: &Subtable,
    num_glyphs: u16,
    ankr: Option<&Ankr>,
    reversed: bool,
) {
    let Some(body) = sub.data.get(HEADER_LEN..) else { return };
    let Some(table) = StateTable::parse(body, 1, num_glyphs) else { return };
    let Some(flags) = read_u32_be(body, 16) else { return };

    let action_kind = (flags & 0xC000_0000) >> 30;
    let actions_at = (flags & 0x00FF_FFFF) as usize;

    let mut state = state::START_OF_TEXT;
    let mut idx = 0usize;
    let mut marked: Option<usize> = None;

    loop {
        let class = if idx < buffer.len {
            table.class(buffer.info[idx].id as u16)
        } else {
            class::END_OF_TEXT
        };
        let Some(entry) = table.entry(state, class) else { break };

        if let Some(mark) = marked
            && entry.word1 != NO_ACTION
            && idx < buffer.len
        {
            let action = entry.word1 as usize;
            let offsets = match action_kind {
                1 => ankr.and_then(|ankr| {
                    let mark_idx = read_u16_be(body, actions_at + action * 4)?;
                    let curr_idx = read_u16_be(body, actions_at + action * 4 + 2)?;
                    let m = ankr.anchor_point(buffer.info[mark].id as u16, mark_idx)?;
                    let c = ankr.anchor_point(buffer.info[idx].id as u16, curr_idx)?;
                    Some((i32::from(m.0) - i32::from(c.0), i32::from(m.1) - i32::from(c.1)))
                }),
                2 => {
                    let base = actions_at + action * 8;
                    control_point(body, base).zip(control_point(body, base + 4)).map(|(m, c)| {
                        (i32::from(m.0) - i32::from(c.0), i32::from(m.1) - i32::from(c.1))
                    })
                }
                _ => None,
            };

            if let Some((dx, dy)) = offsets {
                buffer.pos[idx].x_offset = dx;
                buffer.pos[idx].y_offset = dy;
            }

            buffer.pos[idx].attach_type = attach_type::MARK;
            let chain = mark as isize - idx as isize;
            let chain = if reversed { -chain } else { chain };
            buffer.pos[idx].attach_chain = chain.clamp(i16::MIN as isize, i16::MAX as isize) as i16;
            buffer.scratch_flags |= scratch_flags::HAS_GPOS_ATTACHMENT;
        }

        if entry.flags & entry_flags::MARK != 0 {
            marked = Some(idx);
        }

        state = entry.new_state;
        if idx >= buffer.len {
            break;
        }
        if entry.flags & entry_flags::DONT_ADVANCE == 0 || buffer.max_ops <= 0 {
            idx += 1;
        }
        buffer.max_ops -= 1;
    }
}

fn kern_pairs(
    face: &Face,
    buffer: &mut Buffer,
    kern_mask: Mask,
    sub: &Subtable,
    num_glyphs: u16,
) {
    let horizontal = buffer.direction.is_horizontal();
    let cross_stream = sub.cross_stream;

    let mut ctx = ApplyContext::new(TableIndex::Gpos, face, buffer, |_, _| false);
    ctx.set_lookup_mask(kern_mask);
    ctx.lookup_props = u32::from(lookup_flags::IGNORE_MARKS);

    let mut i = 0;
    while i < ctx.buffer.len {
        if ctx.buffer.info[i].mask & kern_mask == 0 {
            i += 1;
            continue;
        }

        let j = {
            let mut iter = SkippingIterator::new(&ctx, i, false);
            if !iter.next(None) {
                i += 1;
                continue;
            }
            iter.index()
        };

        let (left, right) = (ctx.buffer.info[i].id as u16, ctx.buffer.info[j].id as u16);
        let kern = if u32::from(left) == DELETED_GLYPH || u32::from(right) == DELETED_GLYPH {
            0
        } else {
            sub.kerning(left, right, num_glyphs)
        };
        if kern != 0 {
            apply_pair(ctx.buffer, i, j, kern, horizontal, cross_stream);
            ctx.buffer.unsafe_to_break(i, j + 1);
        }

        i = j;
    }
}
