use crate::daecore::daeshaper::buffer::{scratch_flags, Buffer, GlyphInfo};
use crate::daecore::daeshaper::face::Face;
use super::indic_category::{category, position};
use crate::daecore::daeshaper::ot::map::{feature_flags as ff, MapBuilder};
use crate::daecore::daeshaper::normalize;
use crate::daecore::daeshaper::plan::ShapePlan;
use super::{PauseFn, Shaper, ZeroWidthMarks};
use super::syllabic;
use super::syllable::{self};
use crate::daecore::daeshaper::generated::syllable_tables::{myanmar_accept, MyanmarSyllable, MYANMAR_TRANSITIONS};
use crate::daecore::daeshaper::ot::tag::Tag;

pub(crate) const SHAPER: Shaper = Shaper {
    name: "myanmar",
    collect_features: Some(collect_features),
    pauses: PAUSES,
    override_features: None,
    preprocess_text: None,
    postprocess_glyphs: None,
    normalization_preference: normalize::Mode::ComposedDiacriticsNoShortCircuit,
    decompose: None,
    compose: None,
    setup_masks: Some(setup_masks),
    gpos_tag: None,
    reorder_marks: None,
    zero_width_marks: ZeroWidthMarks::ByGdefEarly,
    fallback_position: false,
};

pub(crate) const ZAWGYI_SHAPER: Shaper = Shaper {
    name: "myanmar_zawgyi",
    collect_features: None,
    pauses: &[],
    override_features: None,
    preprocess_text: None,
    postprocess_glyphs: None,
    normalization_preference: normalize::Mode::None,
    decompose: None,
    compose: None,
    setup_masks: None,
    gpos_tag: None,
    reorder_marks: None,
    zero_width_marks: ZeroWidthMarks::Never,
    fallback_position: false,
};

const PAUSES: &[PauseFn] = &[setup_syllables, reorder, clear_syllables];

const PAUSE_SETUP_SYLLABLES: usize = 0;
const PAUSE_REORDER: usize = 1;
const PAUSE_CLEAR_SYLLABLES: usize = 2;

const BASIC_FEATURES: [&[u8; 4]; 4] = [b"rphf", b"pref", b"blwf", b"pstf"];
const JOINING_FEATURES: [&[u8; 4]; 4] = [b"pres", b"abvs", b"blws", b"psts"];

fn collect_features(b: &mut MapBuilder, _: Option<Tag>) {
    b.add_gsub_pause(Some(PAUSE_SETUP_SYLLABLES));

    b.enable_feature(Tag::from_bytes(b"locl"), ff::PER_SYLLABLE, 1);
    b.enable_feature(Tag::from_bytes(b"ccmp"), ff::PER_SYLLABLE, 1);

    b.add_gsub_pause(Some(PAUSE_REORDER));

    for tag in BASIC_FEATURES {
        b.enable_feature(Tag::from_bytes(tag), ff::MANUAL_ZWJ | ff::PER_SYLLABLE, 1);
        b.add_gsub_pause(None);
    }

    b.add_gsub_pause(Some(PAUSE_CLEAR_SYLLABLES));

    for tag in JOINING_FEATURES {
        b.enable_feature(Tag::from_bytes(tag), ff::MANUAL_ZWJ, 1);
    }
}

impl GlyphInfo {
    fn myanmar_position(&self) -> u8 {
        self.shaper_auxiliary
    }

    fn set_myanmar_position(&mut self, position: u8) {
        self.shaper_auxiliary = position;
    }

    fn is_myanmar_consonant(&self) -> bool {
        matches!(
            self.shaper_category,
            category::C
                | category::CS
                | category::RA
                | category::V
                | category::PLACEHOLDER
                | category::DOTTEDCIRCLE
        )
    }
}

fn setup_masks(_: &ShapePlan, _: &Face, buffer: &mut Buffer) {
    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        let (cat, pos) = super::indic_category::lookup(info.id);
        info.shaper_category = cat;
        info.set_myanmar_position(pos);
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
        &MYANMAR_TRANSITIONS,
        |i| buffer.info[i].shaper_category,
        myanmar_accept,
        |s| segments.push(s),
    );

    let segments = segments.as_slice();
    if segments.iter().any(|s| s.kind == u8::from(MyanmarSyllable::BrokenCluster)) {
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

fn reorder(_: &ShapePlan, face: &Face, buffer: &mut Buffer) -> bool {
    let inserted = syllabic::insert_dotted_circles(
        face,
        buffer,
        MyanmarSyllable::BrokenCluster.into(),
        category::DOTTEDCIRCLE,
        None,
        None,
    );

    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        let kind = buffer.info[start].syllable & 0x0F;
        if kind == MyanmarSyllable::ConsonantSyllable.into()
            || kind == MyanmarSyllable::BrokenCluster.into()
        {
            reorder_consonant_syllable(start, end, buffer);
        }
        start = end;
    }

    inserted
}

fn reorder_consonant_syllable(start: usize, end: usize, buffer: &mut Buffer) {
    let has_kinzi = start + 3 <= end
        && buffer.info[start].shaper_category == category::RA
        && buffer.info[start + 1].shaper_category == category::AS
        && buffer.info[start + 2].shaper_category == category::H;

    let limit = if has_kinzi { start + 3 } else { start };
    let base = (limit..end)
        .find(|&i| buffer.info[i].is_myanmar_consonant())
        .unwrap_or(start);

    let mut i = start;

    while i < start + if has_kinzi { 3 } else { 0 } {
        buffer.info[i].set_myanmar_position(position::AFTER_MAIN);
        i += 1;
    }

    while i < base {
        buffer.info[i].set_myanmar_position(position::PRE_C);
        i += 1;
    }

    if i < end {
        buffer.info[i].set_myanmar_position(position::BASE_C);
        i += 1;
    }

    let mut pos = position::AFTER_MAIN;
    while i < end {
        let category = buffer.info[i].shaper_category;

        if category == category::MR {
            buffer.info[i].set_myanmar_position(position::PRE_C);
        } else if category == category::V_PRE {
            buffer.info[i].set_myanmar_position(position::PRE_M);
        } else if category == category::VS {
            let previous = buffer.info[i - 1].myanmar_position();
            buffer.info[i].set_myanmar_position(previous);
        } else if pos == position::AFTER_MAIN && category == category::V_BLW {
            pos = position::BELOW_C;
            buffer.info[i].set_myanmar_position(pos);
        } else if pos == position::BELOW_C && category == category::A {
            buffer.info[i].set_myanmar_position(position::BEFORE_SUB);
        } else if pos == position::BELOW_C && category == category::V_BLW {
            buffer.info[i].set_myanmar_position(pos);
        } else if pos == position::BELOW_C {
            pos = position::AFTER_SUB;
            buffer.info[i].set_myanmar_position(pos);
        } else {
            buffer.info[i].set_myanmar_position(pos);
        }

        i += 1;
    }

    buffer.sort(start, end, |a, b| a.myanmar_position() > b.myanmar_position());

    flip_pre_base_vowel_run(start, end, buffer);
}

fn flip_pre_base_vowel_run(start: usize, end: usize, buffer: &mut Buffer) {
    let mut first = end;
    let mut last = end;

    for i in start..end {
        if buffer.info[i].myanmar_position() == position::PRE_M {
            if first == end {
                first = i;
            }
            last = i;
        }
    }

    if first >= last {
        return;
    }

    buffer.reverse_range(first, last + 1);

    let mut group_start = first;
    for j in first..=last {
        if buffer.info[j].shaper_category == category::V_PRE {
            buffer.reverse_range(group_start, j + 1);
            group_start = j + 1;
        }
    }
}

fn clear_syllables(_: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        info.syllable = 0;
    }
    false
}
