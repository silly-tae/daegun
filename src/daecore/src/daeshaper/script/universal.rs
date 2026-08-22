use crate::daecore::daeshaper::buffer::{scratch_flags, Buffer, GlyphInfo};
use crate::daecore::daeshaper::face::Face;
use crate::daecore::daeshaper::ot::map::{feature_flags as ff, MapBuilder};
use crate::daecore::daeshaper::normalize;
use crate::daecore::daeshaper::plan::ShapePlan;
use super::{PauseFn, Shaper, ZeroWidthMarks};
use super::syllabic;
use super::syllable::{self, Segment};
use crate::daecore::daeshaper::generated::syllable_tables::{
    lana_accept, use_accept, UseSyllable, LANA_TRANSITIONS, USE_TRANSITIONS,
};
use crate::daecore::daeshaper::ot::tag::Tag;
use crate::daecore::daeshaper::unicode::{self, Script};
use crate::daecore::daeshaper::generated::use_tables;

pub(crate) const SHAPER: Shaper = Shaper {
    name: "universal",
    collect_features: Some(collect_features),
    pauses: PAUSES,
    override_features: None,
    preprocess_text: Some(preprocess_text),
    postprocess_glyphs: None,
    normalization_preference: normalize::Mode::ComposedDiacritics,
    decompose: None,
    compose: Some(compose),
    setup_masks: Some(setup_masks),
    gpos_tag: None,
    reorder_marks: None,
    zero_width_marks: ZeroWidthMarks::ByGdefEarly,
    fallback_position: false,
};

const PAUSES: &[PauseFn] = &[
    setup_syllables,
    clear_substitution_flags,
    record_rphf,
    record_pref,
    reorder,
    clear_syllables,
];

const PAUSE_SETUP_SYLLABLES: usize = 0;
const PAUSE_CLEAR_SUBSTITUTION: usize = 1;
const PAUSE_RECORD_RPHF: usize = 2;
const PAUSE_RECORD_PREF: usize = 3;
const PAUSE_REORDER: usize = 4;
const PAUSE_CLEAR_SYLLABLES: usize = 5;

mod category {
    pub(crate) const B: u8 = 1;
    pub(crate) const H: u8 = 12;
    pub(crate) const R: u8 = 18;
    pub(crate) const V_PRE: u8 = 22;
    pub(crate) const VM_PRE: u8 = 23;
    pub(crate) const F_ABV: u8 = 24;
    pub(crate) const F_BLW: u8 = 25;
    pub(crate) const F_PST: u8 = 26;
    pub(crate) const IS: u8 = 44;
    pub(crate) const FM_ABV: u8 = 45;
    pub(crate) const FM_BLW: u8 = 46;
    pub(crate) const FM_PST: u8 = 47;
    pub(crate) const ZWNJ: u8 = 14;
    pub(crate) const CGJ: u8 = 6;
    pub(crate) const HVM: u8 = 53;
}

const POST_BASE: [u8; 6] = [
    category::F_ABV,
    category::F_BLW,
    category::F_PST,
    category::FM_ABV,
    category::FM_BLW,
    category::FM_PST,
];

const BASIC_FEATURES: [&[u8; 4]; 7] =
    [b"rkrf", b"abvf", b"blwf", b"half", b"pstf", b"vatu", b"cjct"];

const TOPOGRAPHICAL_FEATURES: [&[u8; 4]; 4] = [b"isol", b"init", b"medi", b"fina"];

const OTHER_FEATURES: [&[u8; 4]; 5] = [b"abvs", b"blws", b"haln", b"pres", b"psts"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum JoiningForm {
    Isolated = 0,
    Initial = 1,
    Medial = 2,
    Terminal = 3,
}

pub(crate) const ARABIC_JOINING: &[&str] = &[
    "Adlam", "Arabic", "Chorasmian", "Hanifi_Rohingya", "Mandaic", "Manichaean", "Mongolian",
    "Nko", "Old_Uyghur", "Phags_Pa", "Psalter_Pahlavi", "Sogdian", "Syriac",
];

fn is_tai_tham(script: Option<Script>) -> bool {
    script.is_some_and(|s| s.name() == "Tai_Tham")
}

fn has_arabic_joining(script: Option<Script>) -> bool {
    script.is_some_and(|s| ARABIC_JOINING.contains(&s.name()))
}

impl GlyphInfo {
    fn use_category(&self) -> u8 {
        self.shaper_category
    }

    fn set_use_category(&mut self, c: u8) {
        self.shaper_category = c;
    }

    fn is_use_halant(&self) -> bool {
        matches!(self.use_category(), category::H | category::HVM | category::IS)
            && !self.ligated()
    }
}

fn lookup(c: u32) -> u8 {
    const LEAF_BITS: u32 = use_tables::USE_CATEGORY_LEAF_BITS;
    const MID_BITS: u32 = use_tables::USE_CATEGORY_MID_BITS;
    if c >= 0x0011_0000 {
        return 0;
    }
    let mid = use_tables::USE_CATEGORY_TOP[(c >> (LEAF_BITS + MID_BITS)) as usize] as usize;
    let leaf = use_tables::USE_CATEGORY_MID
        [(mid << MID_BITS) | ((c >> LEAF_BITS) & ((1 << MID_BITS) - 1)) as usize] as usize;
    use_tables::USE_CATEGORY
        [use_tables::USE_CATEGORY_LEAF[(leaf << LEAF_BITS) | (c & ((1 << LEAF_BITS) - 1)) as usize] as usize]
        .category
}

fn collect_features(b: &mut MapBuilder, _: Option<Tag>) {
    b.add_gsub_pause(Some(PAUSE_SETUP_SYLLABLES));

    b.enable_feature(Tag::from_bytes(b"locl"), ff::PER_SYLLABLE, 1);
    b.enable_feature(Tag::from_bytes(b"ccmp"), ff::PER_SYLLABLE, 1);
    b.enable_feature(Tag::from_bytes(b"nukt"), ff::PER_SYLLABLE, 1);
    b.enable_feature(Tag::from_bytes(b"akhn"), ff::MANUAL_ZWJ | ff::PER_SYLLABLE, 1);

    b.add_gsub_pause(Some(PAUSE_CLEAR_SUBSTITUTION));
    b.add_feature(Tag::from_bytes(b"rphf"), ff::MANUAL_ZWJ | ff::PER_SYLLABLE, 1);
    b.add_gsub_pause(Some(PAUSE_RECORD_RPHF));

    b.add_gsub_pause(Some(PAUSE_CLEAR_SUBSTITUTION));
    b.enable_feature(Tag::from_bytes(b"pref"), ff::MANUAL_ZWJ | ff::PER_SYLLABLE, 1);
    b.add_gsub_pause(Some(PAUSE_RECORD_PREF));

    for tag in BASIC_FEATURES {
        b.enable_feature(Tag::from_bytes(tag), ff::MANUAL_ZWJ | ff::PER_SYLLABLE, 1);
    }

    b.add_gsub_pause(Some(PAUSE_REORDER));
    b.add_gsub_pause(Some(PAUSE_CLEAR_SYLLABLES));

    for tag in TOPOGRAPHICAL_FEATURES {
        b.add_feature(Tag::from_bytes(tag), ff::NONE, 1);
    }
    b.add_gsub_pause(None);

    for tag in OTHER_FEATURES {
        b.enable_feature(Tag::from_bytes(tag), ff::MANUAL_ZWJ, 1);
    }
}

fn preprocess_text(_: &ShapePlan, face: &Face, buffer: &mut Buffer) {
    syllabic::insert_vowel_constraints(face, buffer);
}

fn compose(_: &normalize::Context, a: char, b: char) -> Option<char> {
    if unicode::general_category(a).is_mark() {
        return None;
    }
    unicode::compose(a, b)
}

fn setup_masks(plan: &ShapePlan, face: &Face, buffer: &mut Buffer) {
    if has_arabic_joining(buffer.script) {
        super::arabic::setup_masks(plan, face, buffer);
    }

    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        let c = lookup(info.id);
        info.set_use_category(c);
    }
}

fn included(buffer: &Buffer, i: usize) -> bool {
    let info = &buffer.info[i];
    if info.use_category() == category::CGJ {
        return false;
    }
    if info.use_category() == category::ZWNJ {
        for next in &buffer.info[i + 1..buffer.len] {
            if next.use_category() != category::CGJ {
                return !unicode::GeneralCategory::from_stored(next.general_category()).is_mark();
            }
        }
    }
    true
}

fn setup_syllables(plan: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    segment_into_syllables(buffer);

    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        buffer.unsafe_to_break(start, end);
        start = end;
    }

    setup_rphf_mask(plan, buffer);
    setup_topographical_masks(plan, buffer);
    false
}

fn segment_into_syllables(buffer: &mut Buffer) {
    let included: alloc::vec::Vec<usize> =
        (0..buffer.len).filter(|&i| included(buffer, i)).collect();
    let categories: alloc::vec::Vec<u8> =
        included.iter().map(|&i| buffer.info[i].use_category()).collect();

    let mut segments = syllable::Segments::new();
    // Tai Tham writes a cluster's vowels in orders the specification's pattern forbids, so it gets
    // a grammar of its own. Every other script keeps the pattern exactly.
    if is_tai_tham(buffer.script) {
        syllable::segment(
            included.len(),
            &LANA_TRANSITIONS,
            |i| categories[i],
            lana_accept,
            |s| segments.push(s),
        );
    } else {
        syllable::segment(
            included.len(),
            &USE_TRANSITIONS,
            |i| categories[i],
            use_accept,
            |s| segments.push(s),
        );
    }
    let segments = segments.as_slice();

    let mut spans = syllable::Segments::new();
    for (n, s) in segments.iter().enumerate() {
        let start = included[s.start];
        let end = if n + 1 < segments.len() {
            included[segments[n + 1].start]
        } else {
            buffer.len
        };
        spans.push(Segment { start, end, kind: s.kind });
    }
    let spans = spans.as_slice();

    if spans.iter().any(|s| s.kind == u8::from(UseSyllable::BrokenCluster)) {
        buffer.scratch_flags |= scratch_flags::HAS_BROKEN_SYLLABLE;
    }
    syllable::set_syllables(buffer, spans);
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

fn setup_rphf_mask(plan: &ShapePlan, buffer: &mut Buffer) {
    let mask = plan.map.one_mask(Tag::from_bytes(b"rphf"));
    if mask == 0 {
        return;
    }

    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        let limit = if buffer.info[start].use_category() == category::R {
            1
        } else {
            3.min(end - start)
        };
        for info in &mut buffer.info[start..start + limit] {
            info.mask |= mask;
        }
        start = end;
    }
}

fn setup_topographical_masks(plan: &ShapePlan, buffer: &mut Buffer) {
    if has_arabic_joining(buffer.script) {
        return;
    }

    let mut masks = [0u32; 4];
    let mut all = 0;
    for (i, tag) in TOPOGRAPHICAL_FEATURES.iter().enumerate() {
        masks[i] = plan.map.one_mask(Tag::from_bytes(tag));
        if masks[i] == plan.map.global_mask {
            masks[i] = 0;
        }
        all |= masks[i];
    }
    if all == 0 {
        return;
    }
    let others = !all;

    let mut last_start = 0;
    let mut last_form: Option<JoiningForm> = None;
    let mut start = 0;

    while start < buffer.len {
        let end = next_syllable(buffer, start);
        let kind = buffer.info[start].syllable & 0x0F;

        if kind == UseSyllable::HieroglyphCluster.into() || kind == UseSyllable::NonCluster.into() {
            last_form = None;
        } else {
            let joins =
                matches!(last_form, Some(JoiningForm::Terminal) | Some(JoiningForm::Isolated));

            if joins {
                let corrected = if last_form == Some(JoiningForm::Terminal) {
                    JoiningForm::Medial
                } else {
                    JoiningForm::Initial
                };
                for info in &mut buffer.info[last_start..start] {
                    info.mask = (info.mask & others) | masks[corrected as usize];
                }
            }

            let form = if joins { JoiningForm::Terminal } else { JoiningForm::Isolated };
            last_form = Some(form);
            for info in &mut buffer.info[start..end] {
                info.mask = (info.mask & others) | masks[form as usize];
            }
        }

        last_start = start;
        start = end;
    }
}

fn clear_substitution_flags(_: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        info.clear_substituted();
    }
    false
}

fn record_rphf(plan: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    let mask = plan.map.one_mask(Tag::from_bytes(b"rphf"));
    if mask == 0 {
        return false;
    }

    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        for i in start..end {
            if buffer.info[i].mask & mask == 0 {
                break;
            }
            if buffer.info[i].substituted() {
                buffer.info[i].set_use_category(category::R);
                break;
            }
        }
        start = end;
    }
    false
}

fn record_pref(_: &ShapePlan, _: &Face, buffer: &mut Buffer) -> bool {
    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        for i in start..end {
            if buffer.info[i].substituted() {
                buffer.info[i].set_use_category(category::V_PRE);
                break;
            }
        }
        start = end;
    }
    false
}

fn reorder(_: &ShapePlan, face: &Face, buffer: &mut Buffer) -> bool {
    let inserted = syllabic::insert_dotted_circles(
        face,
        buffer,
        UseSyllable::BrokenCluster.into(),
        category::B,
        Some(category::R),
        None,
    );

    let mut start = 0;
    while start < buffer.len {
        let end = next_syllable(buffer, start);
        reorder_syllable(start, end, buffer);
        start = end;
    }

    inserted
}

fn reorder_syllable(start: usize, end: usize, buffer: &mut Buffer) {
    let kind = buffer.info[start].syllable & 0x0F;
    let reorders = kind == UseSyllable::ViramaTerminatedCluster.into()
        || kind == UseSyllable::SakotTerminatedCluster.into()
        || kind == UseSyllable::StandardCluster.into()
        || kind == UseSyllable::BrokenCluster.into();
    if !reorders {
        return;
    }

    move_repha_forward(start, end, buffer);
    move_pre_base_vowels_back(start, end, buffer);
}

fn move_repha_forward(start: usize, end: usize, buffer: &mut Buffer) {
    if buffer.info[start].use_category() != category::R || end - start <= 1 {
        return;
    }

    for i in start + 1..end {
        let post_base = POST_BASE.contains(&buffer.info[i].use_category())
            || buffer.info[i].is_use_halant();

        if !post_base && i != end - 1 {
            continue;
        }

        let target = if post_base { i - 1 } else { i };
        buffer.merge_clusters(start, target + 1);

        let repha = buffer.info[start];
        for k in 0..target - start {
            buffer.info[k + start] = buffer.info[k + start + 1];
        }
        buffer.info[target] = repha;
        return;
    }
}

fn move_pre_base_vowels_back(start: usize, end: usize, buffer: &mut Buffer) {
    let mut target = start;

    for i in start..end {
        if buffer.info[i].is_use_halant() {
            target = i + 1;
            continue;
        }

        let is_pre = matches!(buffer.info[i].use_category(), category::V_PRE | category::VM_PRE);
        if is_pre && buffer.info[i].lig_comp() == 0 && target < i {
            buffer.merge_clusters(target, i + 1);
            let vowel = buffer.info[i];
            for k in (0..i - target).rev() {
                buffer.info[k + target + 1] = buffer.info[k + target];
            }
            buffer.info[target] = vowel;
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
