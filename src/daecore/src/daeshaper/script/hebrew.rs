use crate::daecore::daeshaper::buffer::Buffer;
use crate::daecore::daeshaper::normalize::{self, Context};
use super::{Shaper, ZeroWidthMarks};
use crate::daecore::daeshaper::ot::tag::Tag;
use crate::daecore::daeshaper::unicode;

pub(crate) const SHAPER: Shaper = Shaper {
    name: "hebrew",
    collect_features: None,
    pauses: &[],
    override_features: None,
    preprocess_text: None,
    postprocess_glyphs: None,
    normalization_preference: normalize::Mode::Auto,
    decompose: None,
    compose: Some(compose),
    setup_masks: None,
    gpos_tag: Some(Tag(u32::from_be_bytes(*b"hebr"))),
    reorder_marks: Some(reorder_marks),
    zero_width_marks: ZeroWidthMarks::ByGdefLate,
    fallback_position: true,
};

fn reorder_marks(buffer: &mut Buffer, start: usize, end: usize) {
    const PATAH: u8 = 20;
    const QAMATS: u8 = 21;
    const SHEVA: u8 = 22;
    const HIRIQ: u8 = 23;
    const METEG: u8 = 25;
    const BELOW: u8 = 220;

    for i in start + 2..end {
        let c0 = buffer.info[i - 2].modified_combining_class();
        let c1 = buffer.info[i - 1].modified_combining_class();
        let c2 = buffer.info[i].modified_combining_class();

        if matches!(c0, PATAH | QAMATS)
            && matches!(c1, SHEVA | HIRIQ)
            && matches!(c2, METEG | BELOW)
        {
            buffer.merge_clusters(i - 1, i + 1);
            buffer.info.swap(i - 1, i);
            break;
        }
    }
}

const DAGESH_FORMS: [char; 27] = [
    '\u{FB30}',
    '\u{FB31}',
    '\u{FB32}',
    '\u{FB33}',
    '\u{FB34}',
    '\u{FB35}',
    '\u{FB36}',
    '\0',
    '\u{FB38}',
    '\u{FB39}',
    '\u{FB3A}',
    '\u{FB3B}',
    '\u{FB3C}',
    '\0',
    '\u{FB3E}',
    '\0',
    '\u{FB40}',
    '\u{FB41}',
    '\0',
    '\u{FB43}',
    '\u{FB44}',
    '\0',
    '\u{FB46}',
    '\u{FB47}',
    '\u{FB48}',
    '\u{FB49}',
    '\u{FB4A}',
];

fn compose(ctx: &Context, a: char, b: char) -> Option<char> {
    if let Some(c) = unicode::compose(a, b) {
        return Some(c);
    }
    if ctx.has_gpos_mark {
        return None;
    }

    let base = a as u32;
    match b as u32 {
        0x05B4 => (base == 0x05D9).then_some('\u{FB1D}'),
        0x05B7 => match base {
            0x05D9 => Some('\u{FB1F}'),
            0x05D0 => Some('\u{FB2E}'),
            _ => None,
        },
        0x05B8 => (base == 0x05D0).then_some('\u{FB2F}'),
        0x05B9 => (base == 0x05D5).then_some('\u{FB4B}'),
        0x05BC => match base {
            0x05D0..=0x05EA => match DAGESH_FORMS[(base - 0x05D0) as usize] {
                '\0' => None,
                c => Some(c),
            },
            0xFB2A => Some('\u{FB2C}'),
            0xFB2B => Some('\u{FB2D}'),
            _ => None,
        },
        0x05BF => match base {
            0x05D1 => Some('\u{FB4C}'),
            0x05DB => Some('\u{FB4D}'),
            0x05E4 => Some('\u{FB4E}'),
            _ => None,
        },
        0x05C1 => match base {
            0x05E9 => Some('\u{FB2A}'),
            0xFB49 => Some('\u{FB2C}'),
            _ => None,
        },
        0x05C2 => match base {
            0x05E9 => Some('\u{FB2B}'),
            0xFB49 => Some('\u{FB2D}'),
            _ => None,
        },
        _ => None,
    }
}
