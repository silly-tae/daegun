use crate::daecore::daeshaper::buffer::Buffer;
use crate::daecore::daeshaper::face::Face;
use crate::daecore::daeshaper::ot::map::TableIndex;
use crate::daecore::daeshaper::normalize;
use crate::daecore::daeshaper::plan::ShapePlan;
use super::{Shaper, ZeroWidthMarks};
use crate::daecore::daeshaper::unicode::GeneralCategory;

pub(crate) const SHAPER: Shaper = Shaper {
    name: "thai",
    collect_features: None,
    pauses: &[],
    override_features: None,
    preprocess_text: Some(preprocess_text),
    postprocess_glyphs: None,
    normalization_preference: normalize::Mode::Auto,
    decompose: None,
    compose: None,
    setup_masks: None,
    gpos_tag: None,
    reorder_marks: None,
    zero_width_marks: ZeroWidthMarks::ByGdefLate,
    fallback_position: false,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Consonant {
    Normal = 0,
    Ascending = 1,
    RemovableDescender = 2,
    StrictDescender = 3,
    Other = 4,
}

fn consonant_type(u: u32) -> Consonant {
    match u {
        0x0E1B | 0x0E1D | 0x0E1F => Consonant::Ascending,
        0x0E0D | 0x0E10 => Consonant::RemovableDescender,
        0x0E0E | 0x0E0F => Consonant::StrictDescender,
        0x0E01..=0x0E2E => Consonant::Normal,
        _ => Consonant::Other,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mark {
    Above = 0,
    Below = 1,
    Tone = 2,
    Base = 3,
}

fn mark_type(u: u32) -> Mark {
    match u {
        0x0E31 | 0x0E34..=0x0E37 | 0x0E47 | 0x0E4D..=0x0E4E => Mark::Above,
        0x0E38..=0x0E3A => Mark::Below,
        0x0E48..=0x0E4C => Mark::Tone,
        _ => Mark::Base,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    None,
    Down,
    Left,
    DownLeft,
    RemoveDescender,
}

struct Variant {
    of: u32,
    windows: u32,
    mac: u32,
}

const fn v(of: u32, windows: u32, mac: u32) -> Variant {
    Variant { of, windows, mac }
}

const SHIFT_DOWN: &[Variant] = &[
    v(0x0E48, 0xF70A, 0xF88B),
    v(0x0E49, 0xF70B, 0xF88E),
    v(0x0E4A, 0xF70C, 0xF891),
    v(0x0E4B, 0xF70D, 0xF894),
    v(0x0E4C, 0xF70E, 0xF897),
    v(0x0E38, 0xF718, 0xF89B),
    v(0x0E39, 0xF719, 0xF89C),
    v(0x0E3A, 0xF71A, 0xF89D),
];

const SHIFT_DOWN_LEFT: &[Variant] = &[
    v(0x0E48, 0xF705, 0xF88C),
    v(0x0E49, 0xF706, 0xF88F),
    v(0x0E4A, 0xF707, 0xF892),
    v(0x0E4B, 0xF708, 0xF895),
    v(0x0E4C, 0xF709, 0xF898),
];

const SHIFT_LEFT: &[Variant] = &[
    v(0x0E48, 0xF713, 0xF88A),
    v(0x0E49, 0xF714, 0xF88D),
    v(0x0E4A, 0xF715, 0xF890),
    v(0x0E4B, 0xF716, 0xF893),
    v(0x0E4C, 0xF717, 0xF896),
    v(0x0E31, 0xF710, 0xF884),
    v(0x0E34, 0xF701, 0xF885),
    v(0x0E35, 0xF702, 0xF886),
    v(0x0E36, 0xF703, 0xF887),
    v(0x0E37, 0xF704, 0xF888),
    v(0x0E47, 0xF712, 0xF889),
    v(0x0E4D, 0xF711, 0xF899),
];

const DESCENDERLESS: &[Variant] = &[
    v(0x0E0D, 0xF70F, 0xF89A),
    v(0x0E10, 0xF700, 0xF89E),
];

fn variant_for(u: u32, action: Action, face: &Face) -> u32 {
    let table = match action {
        Action::None => return u,
        Action::Down => SHIFT_DOWN,
        Action::Left => SHIFT_LEFT,
        Action::DownLeft => SHIFT_DOWN_LEFT,
        Action::RemoveDescender => DESCENDERLESS,
    };

    let Some(variant) = table.iter().find(|v| v.of == u) else { return u };
    if face.has_glyph(variant.windows) {
        return variant.windows;
    }
    if face.has_glyph(variant.mac) {
        return variant.mac;
    }
    u
}

#[derive(Clone, Copy)]
enum Above {
    Empty = 0,
    Ascender = 1,
    OneMark = 2,
    Full = 3,
}

const ABOVE_START: [Above; 5] = [
    Above::Empty,
    Above::Ascender,
    Above::Empty,
    Above::Empty,
    Above::Full,
];

const ABOVE_MACHINE: [[(Action, Above); 3]; 4] = [
    [
        (Action::None, Above::Full),
        (Action::None, Above::Empty),
        (Action::Down, Above::Full),
    ],
    [
        (Action::Left, Above::OneMark),
        (Action::None, Above::Ascender),
        (Action::DownLeft, Above::OneMark),
    ],
    [
        (Action::None, Above::Full),
        (Action::None, Above::OneMark),
        (Action::Left, Above::Full),
    ],
    [
        (Action::None, Above::Full),
        (Action::None, Above::Full),
        (Action::None, Above::Full),
    ],
];

#[derive(Clone, Copy)]
enum Below {
    Clear = 0,
    Removable = 1,
    Occupied = 2,
}

const BELOW_START: [Below; 5] = [
    Below::Clear,
    Below::Clear,
    Below::Removable,
    Below::Occupied,
    Below::Occupied,
];

const BELOW_MACHINE: [[(Action, Below); 3]; 3] = [
    [
        (Action::None, Below::Clear),
        (Action::None, Below::Occupied),
        (Action::None, Below::Clear),
    ],
    [
        (Action::None, Below::Removable),
        (Action::RemoveDescender, Below::Occupied),
        (Action::None, Below::Removable),
    ],
    [
        (Action::None, Below::Occupied),
        (Action::Down, Below::Occupied),
        (Action::None, Below::Occupied),
    ],
];

fn pua_shaping(face: &Face, buffer: &mut Buffer) {
    let mut above = ABOVE_START[Consonant::Other as usize];
    let mut below = BELOW_START[Consonant::Other as usize];
    let mut base = 0;

    for i in 0..buffer.len {
        let mark = mark_type(buffer.info[i].id);
        if mark == Mark::Base {
            let consonant = consonant_type(buffer.info[i].id);
            above = ABOVE_START[consonant as usize];
            below = BELOW_START[consonant as usize];
            base = i;
            continue;
        }

        let (above_action, next_above) = ABOVE_MACHINE[above as usize][mark as usize];
        let (below_action, next_below) = BELOW_MACHINE[below as usize][mark as usize];
        above = next_above;
        below = next_below;

        let action = if above_action != Action::None { above_action } else { below_action };

        buffer.unsafe_to_break(base, i);
        if action == Action::RemoveDescender {
            buffer.info[base].id = variant_for(buffer.info[base].id, action, face);
        } else {
            buffer.info[i].id = variant_for(buffer.info[i].id, action, face);
        }
    }
}

fn is_sara_am(u: u32) -> bool {
    (u & !0x0080) == 0x0E33
}

fn nikhahit_of(u: u32) -> u32 {
    u - 0x0E33 + 0x0E4D
}

fn sara_aa_of(u: u32) -> u32 {
    u - 1
}

fn is_above_base_mark(u: u32) -> bool {
    matches!(u & !0x0080, 0x0E31 | 0x0E34..=0x0E37 | 0x0E3B | 0x0E47..=0x0E4E)
}

fn preprocess_text(plan: &ShapePlan, face: &Face, buffer: &mut Buffer) {
    decompose_sara_am(buffer);

    let is_thai = buffer.script.map(|s| s.name()) == Some("Thai");
    // Only where the font has no Thai GSUB: one that has it has already been told how to arrange
    // these marks and does not want a private-use stand-in. The reorder above is not in Microsoft's
    // Thai spec either – it is what Uniscribe does, and fonts are built against that.
    if is_thai && !plan.map.found_script[TableIndex::Gsub.idx()] {
        pua_shaping(face, buffer);
    }
}

fn decompose_sara_am(buffer: &mut Buffer) {
    buffer.clear_output();
    buffer.idx = 0;

    while buffer.idx < buffer.len {
        let u = buffer.cur(0).id;
        if !is_sara_am(u) {
            buffer.next_glyph();
            continue;
        }

        buffer.output_glyph(nikhahit_of(u));
        let at = buffer.out_len - 1;
        buffer.out_info_mut()[at].set_continuation();
        buffer.replace_glyph(sara_aa_of(u));

        let end = buffer.out_len;
        buffer.out_info_mut()[end - 2].set_general_category(GeneralCategory::NonspacingMark as u16);

        let mut start = end - 2;
        while start > 0 && is_above_base_mark(buffer.out_info()[start - 1].id) {
            start -= 1;
        }

        if start + 2 < end {
            buffer.merge_out_clusters(start, end);
            let nikhahit = buffer.out_info()[end - 2];
            let out = buffer.out_info_mut();
            for i in (0..end - start - 2).rev() {
                out[i + start + 1] = out[i + start];
            }
            out[start] = nikhahit;
        }

        if start != 0 {
            buffer.merge_out_grapheme_clusters(start - 1, end);
        }
    }

    buffer.sync();
}
