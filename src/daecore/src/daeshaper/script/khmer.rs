use crate::daecore::daeshaper::buffer::{scratch_flags, Buffer};
use crate::daecore::daeshaper::face::Face;
use super::indic_category::category;
use crate::daecore::daeshaper::ot::map::{feature_flags as ff, MapBuilder};
use crate::daecore::daeshaper::normalize;
use crate::daecore::daeshaper::plan::ShapePlan;
use super::{PauseFn, Shaper, ZeroWidthMarks};
use super::syllabic;
use super::syllable::{self};
use crate::daecore::daeshaper::generated::syllable_tables::{khmer_accept, KhmerSyllable, KHMER_TRANSITIONS};
use crate::daecore::daeshaper::ot::tag::Tag;
use crate::daecore::daeshaper::unicode;

pub(crate) const SHAPER: Shaper = Shaper {
    name: "khmer",
    collect_features: Some(collect_features),
    pauses: PAUSES,
    override_features: Some(override_features),
    preprocess_text: None,
    postprocess_glyphs: None,
    normalization_preference: normalize::Mode::ComposedDiacriticsNoShortCircuit,
    decompose: Some(decompose),
    compose: Some(compose),
    setup_masks: Some(setup_masks),
    gpos_tag: None,
    reorder_marks: None,
    zero_width_marks: ZeroWidthMarks::Never,
    fallback_position: false,
};

const PAUSES: &[PauseFn] = &[setup_syllables, reorder, clear_syllables];

const PAUSE_SETUP_SYLLABLES: usize = 0;
const PAUSE_REORDER: usize = 1;
const PAUSE_CLEAR_SYLLABLES: usize = 2;

const FEATURES: [(&[u8; 4], u32); 9] = [
    (b"pref", ff::MANUAL_JOINERS | ff::PER_SYLLABLE),
    (b"blwf", ff::MANUAL_JOINERS | ff::PER_SYLLABLE),
    (b"abvf", ff::MANUAL_JOINERS | ff::PER_SYLLABLE),
    (b"pstf", ff::MANUAL_JOINERS | ff::PER_SYLLABLE),
    (b"cfar", ff::MANUAL_JOINERS | ff::PER_SYLLABLE),
    (b"pres", ff::GLOBAL_MANUAL_JOINERS),
    (b"abvs", ff::GLOBAL_MANUAL_JOINERS),
    (b"blws", ff::GLOBAL_MANUAL_JOINERS),
    (b"psts", ff::GLOBAL_MANUAL_JOINERS),
];

const PREF: usize = 0;
const BLWF: usize = 1;
const ABVF: usize = 2;
const PSTF: usize = 3;
const CFAR: usize = 4;
const BASIC: usize = 5;

fn collect_features(b: &mut MapBuilder, _: Option<Tag>) {
    b.add_gsub_pause(Some(PAUSE_SETUP_SYLLABLES));
    b.add_gsub_pause(Some(PAUSE_REORDER));

    b.enable_feature(Tag::from_bytes(b"locl"), ff::PER_SYLLABLE, 1);
    b.enable_feature(Tag::from_bytes(b"ccmp"), ff::PER_SYLLABLE, 1);

    for (tag, flags) in FEATURES.iter().take(BASIC) {
        b.add_feature(Tag::from_bytes(tag), *flags, 1);
    }

    b.add_gsub_pause(Some(PAUSE_CLEAR_SYLLABLES));

    for (tag, flags) in FEATURES.iter().skip(BASIC) {
        b.add_feature(Tag::from_bytes(tag), *flags, 1);
    }
}

fn override_features(b: &mut MapBuilder) {
    b.enable_feature(Tag::from_bytes(b"clig"), ff::NONE, 1);
    b.disable_feature(Tag::from_bytes(b"liga"));
}

fn decompose(_: &normalize::Context, ab: char) -> Option<(char, Option<char>)> {
    match ab {
        '\u{17BE}' | '\u{17BF}' | '\u{17C0}' | '\u{17C4}' | '\u{17C5}' => {
            Some(('\u{17C1}', Some(ab)))
        }
        _ => unicode::decompose(ab),
    }
}

fn compose(_: &normalize::Context, a: char, b: char) -> Option<char> {
    if unicode::general_category(a).is_mark() {
        return None;
    }
    unicode::compose(a, b)
}

fn setup_masks(_: &ShapePlan, _: &Face, buffer: &mut Buffer) {
    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        info.shaper_category = super::indic_category::lookup(info.id).0;
    }
}

fn setup_syllables(_: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    segment_into_syllables(buffer);

    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        buffer.unsafe_to_break(start, end);
        start = end;
    }

    false
}

fn segment_into_syllables(buffer: &mut Buffer) {
    let mut segments = syllable::Segments::new();

    let len = buffer.len;
    syllable::segment(
        len,
        &KHMER_TRANSITIONS,
        |i| buffer.info[i].shaper_category,
        khmer_accept,
        |s| segments.push(s),
    );

    let segments = segments.as_slice();
    if segments.iter().any(|s| s.kind == u8::from(KhmerSyllable::BrokenCluster)) {
        buffer.scratch_flags |= scratch_flags::HAS_BROKEN_SYLLABLE;
    }
    syllable::set_syllables(buffer, segments);
}

fn next_syllable(buffer: &Buffer, start: usize) -> usize {
    if start >= buffer.len {
        return start;
    }
    let syllable = buffer.info[start].syllable;
    let mut end = start + 1;
    while end < buffer.len && buffer.info[end].syllable == syllable {
        end += 1;
    }
    end
}

fn reorder(plan: &ShapePlan, face: &Face, buffer: &mut Buffer) -> bool {
    let inserted = syllabic::insert_dotted_circles(
        face,
        buffer,
        KhmerSyllable::BrokenCluster.into(),
        category::DOTTEDCIRCLE,
        Some(category::REPHA),
        None,
    );

    let masks = mask_array(plan);

    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        if buffer.info[start].syllable & 0x0F != KhmerSyllable::NonKhmerCluster.into() {
            reorder_consonant_syllable(&masks, start, end, buffer);
        }
        start = end;
    }

    inserted
}

fn mask_array(plan: &ShapePlan) -> [u32; FEATURES.len()] {
    let mut masks = [0; FEATURES.len()];
    for (i, (tag, flags)) in FEATURES.iter().enumerate() {
        if flags & ff::GLOBAL == 0 {
            masks[i] = plan.map.one_mask(Tag::from_bytes(tag));
        }
    }
    masks
}

fn reorder_consonant_syllable(
    masks: &[u32; FEATURES.len()],
    start: usize,
    end: usize,
    buffer: &mut Buffer,
) {
    let post_base = masks[BLWF] | masks[ABVF] | masks[PSTF];
    for info in &mut buffer.info[start + 1..end] {
        info.mask |= post_base;
    }

    let mut coengs = 0;
    let mut i = start + 1;

    while i < end {
        if buffer.info[i].shaper_category == category::H && coengs <= 2 && i + 1 < end {
            coengs += 1;

            if buffer.info[i + 1].shaper_category == category::RA {
                buffer.info[i].mask |= masks[PREF];
                buffer.info[i + 1].mask |= masks[PREF];

                buffer.merge_clusters(start, i + 2);
                let coeng = buffer.info[i];
                let ro = buffer.info[i + 1];
                for k in (0..i - start).rev() {
                    buffer.info[k + start + 2] = buffer.info[k + start];
                }
                buffer.info[start] = coeng;
                buffer.info[start + 1] = ro;

                if masks[CFAR] != 0 {
                    for info in &mut buffer.info[i + 2..end] {
                        info.mask |= masks[CFAR];
                    }
                }

                coengs = 2;
            }
        } else if buffer.info[i].shaper_category == category::V_PRE {
            buffer.merge_clusters(start, i + 1);
            let matra = buffer.info[i];
            for k in (0..i - start).rev() {
                buffer.info[k + start + 1] = buffer.info[k + start];
            }
            buffer.info[start] = matra;
        }

        i += 1;
    }
}

fn clear_syllables(_: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        info.syllable = 0;
    }
    false
}
