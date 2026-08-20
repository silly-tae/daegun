use crate::daecore::daeshaper::buffer::{scratch_flags, Buffer, Direction, GlyphInfo, Mask};
use crate::daecore::daeshaper::face::Face;
use crate::daecore::daeshaper::ot::map::{feature_flags as ff, MapBuilder};
use crate::daecore::daeshaper::normalize;
use crate::daecore::daeshaper::plan::ShapePlan;
use super::{Shaper, ZeroWidthMarks};
use crate::daecore::daeshaper::ot::tag::Tag;
use crate::daecore::daeshaper::unicode::{self, GeneralCategory, JoiningType};

pub(crate) const SHAPER: Shaper = Shaper {
    name: "arabic",
    collect_features: Some(collect_features),
    pauses: &[record_stch, no_op],
    override_features: None,
    preprocess_text: None,
    postprocess_glyphs: Some(postprocess_glyphs),
    normalization_preference: normalize::Mode::Auto,
    decompose: None,
    compose: None,
    setup_masks: Some(setup_masks),
    gpos_tag: None,
    reorder_marks: Some(reorder_marks),
    zero_width_marks: ZeroWidthMarks::ByGdefLate,
    fallback_position: true,
};

const FEATURES: [&[u8; 4]; 7] = [b"isol", b"fina", b"fin2", b"fin3", b"medi", b"med2", b"init"];

mod action {
    pub(super) const ISOL: u8 = 0;
    pub(super) const FINA: u8 = 1;
    pub(super) const FIN2: u8 = 2;
    pub(super) const FIN3: u8 = 3;
    pub(super) const MEDI: u8 = 4;
    pub(super) const MED2: u8 = 5;
    pub(super) const INIT: u8 = 6;
    pub(super) const NONE: u8 = 7;

    // The same byte is reused once joining is done, to record what `stch` produced.
    pub(super) const STRETCHING_FIXED: u8 = 8;
    pub(super) const STRETCHING_REPEATING: u8 = 9;

    pub(super) fn is_stretching(n: u8) -> bool {
        matches!(n, STRETCHING_FIXED | STRETCHING_REPEATING)
    }
}

type Entry = (u8, u8, u8);
const STATE_TABLE: [[Entry; 6]; 7] = [

    [
        (action::NONE, action::NONE, 0),
        (action::NONE, action::ISOL, 2),
        (action::NONE, action::ISOL, 1),
        (action::NONE, action::ISOL, 2),
        (action::NONE, action::ISOL, 1),
        (action::NONE, action::ISOL, 6),
    ],
    [
        (action::NONE, action::NONE, 0),
        (action::NONE, action::ISOL, 2),
        (action::NONE, action::ISOL, 1),
        (action::NONE, action::ISOL, 2),
        (action::NONE, action::FIN2, 5),
        (action::NONE, action::ISOL, 6),
    ],
    [
        (action::NONE, action::NONE, 0),
        (action::NONE, action::ISOL, 2),
        (action::INIT, action::FINA, 1),
        (action::INIT, action::FINA, 3),
        (action::INIT, action::FINA, 4),
        (action::INIT, action::FINA, 6),
    ],
    [
        (action::NONE, action::NONE, 0),
        (action::NONE, action::ISOL, 2),
        (action::MEDI, action::FINA, 1),
        (action::MEDI, action::FINA, 3),
        (action::MEDI, action::FINA, 4),
        (action::MEDI, action::FINA, 6),
    ],
    [
        (action::NONE, action::NONE, 0),
        (action::NONE, action::ISOL, 2),
        (action::MED2, action::ISOL, 1),
        (action::MED2, action::ISOL, 2),
        (action::MED2, action::FIN2, 5),
        (action::MED2, action::ISOL, 6),
    ],
    [
        (action::NONE, action::NONE, 0),
        (action::NONE, action::ISOL, 2),
        (action::ISOL, action::ISOL, 1),
        (action::ISOL, action::ISOL, 2),
        (action::ISOL, action::FIN2, 5),
        (action::ISOL, action::ISOL, 6),
    ],
    [
        (action::NONE, action::NONE, 0),
        (action::NONE, action::ISOL, 2),
        (action::NONE, action::ISOL, 1),
        (action::NONE, action::ISOL, 2),
        (action::NONE, action::FIN3, 5),
        (action::NONE, action::ISOL, 6),
    ],
];

fn no_op(_: &ShapePlan, _: &Face, _: &mut Buffer) -> bool {
    false
}

fn is_syriac_only(tag: &[u8; 4]) -> bool {
    matches!(tag[3], b'2' | b'3')
}

fn collect_features(b: &mut MapBuilder, script: Option<Tag>) {
    let tag = |s: &[u8; 4]| Tag::from_bytes(s);
    let is_arabic = script == Some(tag(b"arab"));

    b.enable_feature(tag(b"stch"), ff::NONE, 1);
    b.add_gsub_pause(Some(PAUSE_RECORD_STCH));

    b.enable_feature(tag(b"ccmp"), ff::MANUAL_JOINERS, 1);
    b.enable_feature(tag(b"locl"), ff::MANUAL_JOINERS, 1);
    b.add_gsub_pause(Some(PAUSE_NONE));

    for feature in FEATURES {
        let fallback = if is_arabic && !is_syriac_only(feature) { ff::HAS_FALLBACK } else { 0 };
        b.add_feature(tag(feature), ff::MANUAL_JOINERS | fallback, 1);
        b.add_gsub_pause(Some(PAUSE_NONE));
    }

    b.enable_feature(tag(b"rlig"), ff::MANUAL_JOINERS | ff::HAS_FALLBACK, 1);
    if is_arabic {
        b.add_gsub_pause(Some(PAUSE_NONE));
    }

    b.enable_feature(tag(b"calt"), ff::MANUAL_JOINERS, 1);
    b.add_gsub_pause(Some(PAUSE_NONE));

    b.enable_feature(tag(b"liga"), ff::MANUAL_JOINERS, 1);
    b.enable_feature(tag(b"clig"), ff::MANUAL_JOINERS, 1);
    b.enable_feature(tag(b"mset"), ff::MANUAL_JOINERS, 1);
}

const PAUSE_RECORD_STCH: usize = 0;
const PAUSE_NONE: usize = 1;

impl GlyphInfo {
    fn joining_action(&self) -> u8 {
        self.shaper_auxiliary
    }

    fn set_joining_action(&mut self, action: u8) {
        self.shaper_auxiliary = action;
    }
}

fn joining_type_of_codepoint(id: u32) -> JoiningType {
    let Some(c) = char::from_u32(id) else { return JoiningType::NonJoining };
    unicode::joining_type(c)
}

fn joining_type_of(info: &GlyphInfo) -> JoiningType {
    joining_type_of_codepoint(info.id)
}

fn arabic_joining(buffer: &mut Buffer) {
    let mut prev: Option<usize> = None;
    let mut state = 0usize;

    for i in 0..buffer.context_len[0] {
        let this_type = joining_type_of_codepoint(buffer.context[0][i]);
        if this_type == JoiningType::Transparent {
            continue;
        }
        state = STATE_TABLE[state][this_type as usize].2 as usize;
        break;
    }

    for i in 0..buffer.len {
        let this_type = joining_type_of(&buffer.info[i]);

        if this_type == JoiningType::Transparent {
            buffer.info[i].set_joining_action(action::NONE);
            continue;
        }

        let entry = STATE_TABLE[state][this_type as usize];

        if entry.0 != action::NONE {
            if let Some(p) = prev {
                buffer.info[p].set_joining_action(entry.0);
                buffer.safe_to_insert_tatweel(p, i + 1);
            }
        } else if let Some(p) = prev {
            if this_type >= JoiningType::RightJoining || (2..=5).contains(&state) {
                buffer.unsafe_to_concat(p, i + 1);
            }
        } else if this_type >= JoiningType::RightJoining {
            buffer.unsafe_to_concat_from_outbuffer(0, i + 1);
        }

        buffer.info[i].set_joining_action(entry.1);
        prev = Some(i);
        state = entry.2 as usize;
    }

    for i in 0..buffer.context_len[1] {
        let this_type = joining_type_of_codepoint(buffer.context[1][i]);
        if this_type == JoiningType::Transparent {
            continue;
        }
        let entry = STATE_TABLE[state][this_type as usize];
        if entry.0 != action::NONE
            && let Some(p) = prev {
                buffer.info[p].set_joining_action(entry.0);
            }
        break;
    }
}

fn mongolian_variation_selectors(buffer: &mut Buffer) {
    for i in 1..buffer.len {
        let id = buffer.info[i].id;
        if (0x180B..=0x180D).contains(&id) || id == 0x180F {
            let action = buffer.info[i - 1].joining_action();
            buffer.info[i].set_joining_action(action);
        }
    }
}

pub(crate) fn setup_masks(plan: &ShapePlan, _: &Face, buffer: &mut Buffer) {
    arabic_joining(buffer);

    if buffer.script.map(|s| s.name()) == Some("Mongolian") {
        mongolian_variation_selectors(buffer);
    }

    let mut masks = [0 as Mask; FEATURES.len() + 1];
    for (i, feature) in FEATURES.iter().enumerate() {
        masks[i] = plan.map.one_mask(Tag::from_bytes(feature));
    }

    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        masks
            .get(info.joining_action() as usize)
            .inspect(|&&m| info.mask |= m);
    }
}

fn record_stch(plan: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    if plan.map.one_mask(Tag::from_bytes(b"stch")) == 0 {
        return false;
    }

    let len = buffer.len;
    let mut found = false;
    for info in &mut buffer.info[..len] {
        if let Some(action) = stretch_action(info) {
            info.set_joining_action(action);
            found = true;
        }
    }

    if found {
        buffer.scratch_flags |= scratch_flags::ARABIC_HAS_STCH;
    }
    false
}

fn stretch_action(info: &GlyphInfo) -> Option<u8> {
    if !info.multiplied() {
        return None;
    }
    Some(if !info.lig_comp().is_multiple_of(2) {
        action::STRETCHING_REPEATING
    } else {
        action::STRETCHING_FIXED
    })
}

fn postprocess_glyphs(_: &ShapePlan, face: &Face, buffer: &mut Buffer) {
    apply_stch(face, buffer);
}

fn apply_stch(face: &Face, buffer: &mut Buffer) {
    if buffer.scratch_flags & scratch_flags::ARABIC_HAS_STCH == 0 {
        return;
    }

    let rtl = buffer.direction == Direction::RightToLeft;
    if !rtl {
        buffer.reverse();
    }

    const MEASURE: usize = 0;
    const CUT: usize = 1;
    let mut extra_needed = 0usize;

    for step in [MEASURE, CUT] {
        let new_len = buffer.len + extra_needed;
        let mut i = buffer.len;
        let mut j = new_len;

        while i != 0 {
            if !action::is_stretching(buffer.info[i - 1].joining_action()) {
                if step == CUT {
                    j -= 1;
                    buffer.info[j] = buffer.info[i - 1];
                    buffer.pos[j] = buffer.pos[i - 1];
                }
                i -= 1;
                continue;
            }

            let end = i;
            let mut w_fixed = 0;
            let mut w_repeating = 0;
            let mut n_repeating = 0i32;

            while i != 0 && action::is_stretching(buffer.info[i - 1].joining_action()) {
                i -= 1;
                let width = face.glyph_h_advance(buffer.info[i].id as u16);
                if buffer.info[i].joining_action() == action::STRETCHING_FIXED {
                    w_fixed += width;
                } else {
                    w_repeating += width;
                    n_repeating += 1;
                }
            }

            let start = i;

            let mut context = i;
            let mut w_total = 0;
            while context != 0
                && !action::is_stretching(buffer.info[context - 1].joining_action())
                && (buffer.info[context - 1].is_default_ignorable()
                    || is_word_category(&buffer.info[context - 1]))
            {
                context -= 1;
                w_total += buffer.pos[context].x_advance;
            }

            let mut n_copies = 0i32;
            let mut w_remaining = w_total - w_fixed;
            if w_remaining > w_repeating && w_repeating > 0 {
                n_copies = w_remaining / w_repeating - 1;
            }

            let mut overlap = 0;
            let shortfall = w_remaining.saturating_sub(w_repeating.saturating_mul(n_copies.saturating_add(1)));
            if shortfall > 0 && n_repeating > 0 {
                n_copies += 1;
                let excess = n_copies
                    .saturating_add(1)
                    .saturating_mul(w_repeating)
                    .saturating_sub(w_remaining);
                if excess > 0 {
                    overlap = excess / n_copies.saturating_mul(n_repeating).max(1);
                    w_remaining = 0;
                }
            }

            if step == MEASURE {
                extra_needed += n_copies.saturating_mul(n_repeating).max(0) as usize;
            } else {
                buffer.unsafe_to_break(context, end);
                let mut x_offset = w_remaining / 2;

                for k in (start + 1..=end).rev() {
                    let width = face.glyph_h_advance(buffer.info[k - 1].id as u16);
                    let mut repeat = 1;
                    if buffer.info[k - 1].joining_action() == action::STRETCHING_REPEATING {
                        repeat += n_copies;
                    }

                    buffer.pos[k - 1].x_advance = 0;
                    for n in 0..repeat {
                        if rtl {
                            x_offset -= width;
                            if n > 0 {
                                x_offset += overlap;
                            }
                        }
                        buffer.pos[k - 1].x_offset = x_offset;

                        j -= 1;
                        buffer.info[j] = buffer.info[k - 1];
                        buffer.pos[j] = buffer.pos[k - 1];

                        if !rtl {
                            x_offset += width;
                            if n > 0 {
                                x_offset -= overlap;
                            }
                        }
                    }
                }
            }

        }

        if step == MEASURE {
            if !buffer.ensure(buffer.len + extra_needed) {
                break;
            }
        } else {
            debug_assert_eq!(j, 0);
            buffer.len = new_len;
        }
    }

    if !rtl {
        buffer.reverse();
    }
}

fn is_word_category(info: &GlyphInfo) -> bool {
    use GeneralCategory as G;
    matches!(
        GeneralCategory::from_stored(info.general_category()),
        G::Unassigned
            | G::PrivateUse
            | G::ModifierLetter
            | G::OtherLetter
            | G::SpacingMark
            | G::EnclosingMark
            | G::NonspacingMark
            | G::DecimalNumber
            | G::LetterNumber
            | G::OtherNumber
            | G::CurrencySymbol
            | G::ModifierSymbol
            | G::MathSymbol
            | G::OtherSymbol
    )
}

const MODIFIER_MARKS: &[u32] = &[
    0x0654,
    0x0655,
    0x0658,
    0x06DC,
    0x06E3,
    0x06E7,
    0x06E8,
    0x08CA,
    0x08CB,
    0x08CD,
    0x08CE,
    0x08CF,
    0x08D3,
    0x08F3,
];

fn reorder_marks(buffer: &mut Buffer, mut start: usize, end: usize) {
    let mut i = start;

    for cc in [220u8, 230] {
        while i < end && buffer.info[i].modified_combining_class() < cc {
            i += 1;
        }
        if i == end {
            break;
        }
        if buffer.info[i].modified_combining_class() > cc {
            continue;
        }

        let mut j = i;
        while j < end
            && buffer.info[j].modified_combining_class() == cc
            && MODIFIER_MARKS.contains(&buffer.info[j].id)
        {
            j += 1;
        }
        if i == j {
            continue;
        }

        let count = j - i;
        debug_assert!(count <= normalize::MAX_COMBINING_MARKS);
        buffer.merge_clusters(start, j);

        let mut moved = [GlyphInfo::default(); normalize::MAX_COMBINING_MARKS];
        moved[..count].copy_from_slice(&buffer.info[i..j]);
        for k in (0..i - start).rev() {
            buffer.info[k + start + count] = buffer.info[k + start];
        }
        buffer.info[start..start + count].copy_from_slice(&moved[..count]);

        let new_start = start + count;
        let new_cc = if cc == 220 { 22 } else { 26 };
        while start < new_start {
            buffer.info[start].set_modified_combining_class(new_cc);
            start += 1;
        }

        i = j;
    }
}
