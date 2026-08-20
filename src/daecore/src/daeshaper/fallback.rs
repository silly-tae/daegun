use super::buffer::{Buffer, Direction, GlyphPosition};
use super::face::{Face, GlyphExtents};
use super::plan::ShapePlan;
use super::unicode::{self, GeneralCategory, SpaceFallback};

mod ccc {
    pub(super) const ATTACHED_BELOW_LEFT: u8 = 200;
    pub(super) const ATTACHED_BELOW: u8 = 202;
    pub(super) const ATTACHED_ABOVE: u8 = 214;
    pub(super) const ATTACHED_ABOVE_RIGHT: u8 = 216;
    pub(super) const BELOW_LEFT: u8 = 218;
    pub(super) const BELOW: u8 = 220;
    pub(super) const BELOW_RIGHT: u8 = 222;
    pub(super) const ABOVE_LEFT: u8 = 228;
    pub(super) const ABOVE: u8 = 230;
    pub(super) const ABOVE_RIGHT: u8 = 232;
    pub(super) const DOUBLE_BELOW: u8 = 233;
    pub(super) const DOUBLE_ABOVE: u8 = 234;
}

fn recategorize_combining_class(u: u32, mut class: u8) -> u8 {
    if class >= 200 {
        return class;
    }

    if u & !0xFF == 0x0E00 {
        if class == 0 {
            match u {
                0x0E31 | 0x0E34..=0x0E37 | 0x0E47 | 0x0E4C..=0x0E4E => class = ccc::ABOVE_RIGHT,
                0x0EB1 | 0x0EB4..=0x0EB7 | 0x0EBB | 0x0ECC | 0x0ECD => class = ccc::ABOVE,
                0x0EBC => class = ccc::BELOW,
                _ => {}
            }
        } else if u == 0x0E3A {
            class = ccc::BELOW_RIGHT;
        }
    }

    match class {
        15..=25 => ccc::BELOW,
        13 => ccc::ATTACHED_ABOVE,
        10 => ccc::ABOVE_RIGHT,
        11 | 14 => ccc::ABOVE_LEFT,
        26 => ccc::ABOVE,
        12 => class,

        27..=29 | 31 | 32 | 34..=36 => ccc::ABOVE,
        30 | 33 => ccc::BELOW,

        3 => ccc::BELOW_RIGHT,
        107 => ccc::ABOVE_RIGHT,

        118 => ccc::BELOW,
        122 => ccc::ABOVE,

        129 => ccc::BELOW,
        132 => ccc::ABOVE,
        131 => ccc::BELOW,

        _ => class,
    }
}

pub(crate) fn recategorize_marks(buffer: &mut Buffer) {
    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        if GeneralCategory::from_stored(info.general_category()) == GeneralCategory::NonspacingMark {
            let class = recategorize_combining_class(info.id, info.modified_combining_class());
            info.set_modified_combining_class(class);
        }
    }
}

fn zero_mark_advances(buffer: &mut Buffer, start: usize, end: usize, adjust_offsets: bool) {
    for i in start..end {
        let is_mark = GeneralCategory::from_stored(buffer.info[i].general_category())
            == GeneralCategory::NonspacingMark;
        if !is_mark {
            continue;
        }
        if adjust_offsets {
            buffer.pos[i].x_offset = buffer.pos[i].x_offset.saturating_sub(buffer.pos[i].x_advance);
            buffer.pos[i].y_offset = buffer.pos[i].y_offset.saturating_sub(buffer.pos[i].y_advance);
        }
        buffer.pos[i].x_advance = 0;
        buffer.pos[i].y_advance = 0;
    }
}

fn position_mark(
    face: &Face,
    direction: Direction,
    glyph: u16,
    pos: &mut GlyphPosition,
    base: &mut GlyphExtents,
    class: u8,
) {
    let Some(mark) = face.glyph_extents(glyph) else { return };

    let y_gap = i32::from(face.units_per_em()) / 16;
    pos.x_offset = 0;
    pos.y_offset = 0;

    match class {
        ccc::DOUBLE_BELOW | ccc::DOUBLE_ABOVE if direction.is_horizontal() => {
            let edge = if direction.is_backward() { 0 } else { base.width };
            pos.x_offset += base.x_bearing + edge - mark.width / 2 - mark.x_bearing;
        }
        ccc::ATTACHED_BELOW_LEFT | ccc::BELOW_LEFT | ccc::ABOVE_LEFT => {
            pos.x_offset += base.x_bearing - mark.x_bearing;
        }
        ccc::ATTACHED_ABOVE_RIGHT | ccc::BELOW_RIGHT | ccc::ABOVE_RIGHT => {
            pos.x_offset += base.x_bearing + base.width - mark.width - mark.x_bearing;
        }
        _ => {
            pos.x_offset += base.x_bearing + (base.width - mark.width) / 2 - mark.x_bearing;
        }
    }

    let attached = matches!(
        class,
        ccc::ATTACHED_BELOW_LEFT | ccc::ATTACHED_BELOW | ccc::ATTACHED_ABOVE | ccc::ATTACHED_ABOVE_RIGHT
    );

    match class {
        ccc::DOUBLE_BELOW
        | ccc::BELOW_LEFT
        | ccc::BELOW
        | ccc::BELOW_RIGHT
        | ccc::ATTACHED_BELOW_LEFT
        | ccc::ATTACHED_BELOW => {
            if !attached {
                base.height -= y_gap;
            }
            pos.y_offset = base.y_bearing + base.height - mark.y_bearing;

            if (y_gap > 0) == (pos.y_offset > 0) {
                base.height -= pos.y_offset;
                pos.y_offset = 0;
            }
            base.height += mark.height;
        }

        ccc::DOUBLE_ABOVE
        | ccc::ABOVE_LEFT
        | ccc::ABOVE
        | ccc::ABOVE_RIGHT
        | ccc::ATTACHED_ABOVE
        | ccc::ATTACHED_ABOVE_RIGHT => {
            if !attached {
                base.y_bearing += y_gap;
                base.height -= y_gap;
            }
            pos.y_offset = base.y_bearing - (mark.y_bearing + mark.height);

            if (y_gap > 0) != (pos.y_offset > 0) {
                let correction = -pos.y_offset / 2;
                base.y_bearing += correction;
                base.height -= correction;
                pos.y_offset += correction;
            }
            base.y_bearing -= mark.height;
            base.height += mark.height;
        }

        _ => {}
    }
}

fn position_around_base(
    plan: &ShapePlan,
    face: &Face,
    buffer: &mut Buffer,
    base: usize,
    end: usize,
    adjust_offsets: bool,
) {
    buffer.unsafe_to_break(base, end);

    let base_glyph = buffer.info[base].id as u16;
    let Some(mut base_extents) = face.glyph_extents(base_glyph) else {
        zero_mark_advances(buffer, base + 1, end, adjust_offsets);
        return;
    };

    base_extents.y_bearing += buffer.pos[base].y_offset;
    base_extents.x_bearing = 0;
    base_extents.width = face.glyph_h_advance(base_glyph);

    let lig_id = buffer.info[base].lig_id();
    let num_components = i32::from(buffer.info[base].lig_num_comps());

    let mut x_offset = 0;
    let mut y_offset = 0;
    if !buffer.direction.is_backward() {
        x_offset -= buffer.pos[base].x_advance;
        y_offset -= buffer.pos[base].y_advance;
    }

    let horizontal_dir = if plan.direction.is_horizontal() {
        plan.direction
    } else {
        buffer
            .script
            .and_then(unicode::horizontal_direction)
            .unwrap_or(Direction::LeftToRight)
    };

    let mut last_component: i32 = -1;
    let mut last_class: u8 = 255;
    let mut component_extents = base_extents;
    let mut cluster_extents = base_extents;

    for i in base + 1..end {
        let class = buffer.info[i].modified_combining_class();
        if class == 0 {
            if buffer.direction.is_backward() {
                x_offset += buffer.pos[i].x_advance;
                y_offset += buffer.pos[i].y_advance;
            } else {
                x_offset -= buffer.pos[i].x_advance;
                y_offset -= buffer.pos[i].y_advance;
            }
            continue;
        }

        if num_components > 1 {
            let this_lig_id = buffer.info[i].lig_id();
            let mut this_component = i32::from(buffer.info[i].lig_comp()) - 1;
            if lig_id == 0 || lig_id != this_lig_id || this_component >= num_components {
                this_component = num_components - 1;
            }

            if last_component != this_component {
                last_component = this_component;
                last_class = 255;
                component_extents = base_extents;

                let nth = if horizontal_dir == Direction::LeftToRight {
                    this_component
                } else {
                    num_components - 1 - this_component
                };
                component_extents.x_bearing += (nth * component_extents.width) / num_components;
                component_extents.width /= num_components;
            }
        }

        if last_class != class {
            last_class = class;
            cluster_extents = component_extents;
        }

        let glyph = buffer.info[i].id as u16;
        let direction = buffer.direction;
        position_mark(face, direction, glyph, &mut buffer.pos[i], &mut cluster_extents, class);

        buffer.pos[i].x_advance = 0;
        buffer.pos[i].y_advance = 0;
        buffer.pos[i].x_offset += x_offset;
        buffer.pos[i].y_offset += y_offset;
    }
}

fn position_cluster(
    plan: &ShapePlan,
    face: &Face,
    buffer: &mut Buffer,
    start: usize,
    end: usize,
    adjust_offsets: bool,
) {
    if end - start < 2 {
        return;
    }

    let mut i = start;
    while i < end {
        if !is_unicode_mark(buffer, i) {
            let mut j = i + 1;
            while j < end && is_unicode_mark(buffer, j) {
                j += 1;
            }
            position_around_base(plan, face, buffer, i, j, adjust_offsets);
            i = j - 1;
        }
        i += 1;
    }
}

fn is_unicode_mark(buffer: &Buffer, i: usize) -> bool {
    GeneralCategory::from_stored(buffer.info[i].general_category()).is_mark()
}

pub(crate) fn position_marks(
    plan: &ShapePlan,
    face: &Face,
    buffer: &mut Buffer,
    adjust_offsets: bool,
) {
    let len = buffer.len;
    let mut start = 0;
    for i in 1..len {
        if !is_unicode_mark(buffer, i) {
            position_cluster(plan, face, buffer, start, i, adjust_offsets);
            start = i;
        }
    }
    position_cluster(plan, face, buffer, start, len, adjust_offsets);
}

pub(crate) fn adjust_spaces(face: &Face, buffer: &mut Buffer) {
    let len = buffer.len;
    let horizontal = buffer.direction.is_horizontal();
    let upm = i32::from(face.units_per_em());

    for i in 0..len {
        if buffer.info[i].ligated() {
            continue;
        }
        let Some(kind) = buffer.info[i].space_fallback() else { continue };

        let length = match kind {
            SpaceFallback::Space => continue,
            SpaceFallback::EmDiv(n) => {
                let n = i32::from(n).max(1);
                (upm + n / 2) / n
            }
            SpaceFallback::Em4Of18 => upm * 4 / 18,
            SpaceFallback::Figure => {
                let Some(g) = ('0'..='9').find_map(|c| face.glyph_index(c as u32)) else { continue };
                advance_of(face, g, horizontal)
            }
            SpaceFallback::Punctuation => {
                let Some(g) = face.glyph_index('.' as u32).or_else(|| face.glyph_index(',' as u32))
                else {
                    continue;
                };
                advance_of(face, g, horizontal)
            }
            SpaceFallback::Narrow => {
                if horizontal {
                    buffer.pos[i].x_advance /= 2;
                } else {
                    buffer.pos[i].y_advance /= 2;
                }
                continue;
            }
        };

        if horizontal {
            buffer.pos[i].x_advance = length;
        } else {
            buffer.pos[i].y_advance = -length;
        }
    }
}

fn advance_of(face: &Face, glyph: u16, horizontal: bool) -> i32 {
    if horizontal {
        face.glyph_h_advance(glyph)
    } else {
        face.glyph_v_advance(glyph)
    }
}
