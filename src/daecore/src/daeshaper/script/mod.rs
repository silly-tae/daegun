pub(crate) mod arabic;
pub(crate) mod hangul;
pub(crate) mod hebrew;
pub(crate) mod thai;
pub(crate) mod indic;
pub(crate) mod indic_category;
pub(crate) mod khmer;
pub(crate) mod myanmar;
pub(crate) mod universal;
pub(crate) mod syllable;
pub(crate) mod syllabic;

use super::buffer::{Buffer, Direction};
use super::face::Face;
use super::ot::map::MapBuilder;
use super::normalize;
use super::plan::ShapePlan;
use super::ot::tag::Tag;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ZeroWidthMarks {
    Never,
    ByGdefEarly,
    ByGdefLate,
}

pub(crate) type PauseFn = fn(&ShapePlan, &Face, &mut Buffer) -> bool;

pub(crate) struct Shaper {
    pub(crate) name: &'static str,
    pub(crate) collect_features: Option<fn(&mut MapBuilder, Option<Tag>)>,
    pub(crate) pauses: &'static [PauseFn],
    pub(crate) override_features: Option<fn(&mut MapBuilder)>,
    pub(crate) preprocess_text: Option<fn(&ShapePlan, &Face, &mut Buffer)>,
    pub(crate) postprocess_glyphs: Option<fn(&ShapePlan, &Face, &mut Buffer)>,
    pub(crate) normalization_preference: normalize::Mode,
    pub(crate) decompose: Option<normalize::DecomposeFn>,
    pub(crate) compose: Option<normalize::ComposeFn>,
    pub(crate) setup_masks: Option<fn(&ShapePlan, &Face, &mut Buffer)>,
    pub(crate) gpos_tag: Option<Tag>,
    pub(crate) reorder_marks: Option<fn(&mut Buffer, usize, usize)>,
    pub(crate) zero_width_marks: ZeroWidthMarks,
    pub(crate) fallback_position: bool,
}

pub(crate) const DEFAULT: Shaper = Shaper {
    name: "default",
    collect_features: None,
    pauses: &[],
    override_features: None,
    preprocess_text: None,
    postprocess_glyphs: None,
    normalization_preference: normalize::Mode::Auto,
    decompose: None,
    compose: None,
    setup_masks: None,
    gpos_tag: None,
    reorder_marks: None,
    zero_width_marks: ZeroWidthMarks::ByGdefLate,
    fallback_position: true,
};

const INDIC: &[&str] = &[
    "Bengali", "Devanagari", "Gujarati", "Gurmukhi", "Kannada", "Malayalam", "Oriya", "Tamil",
    "Telugu",
];

const UNIVERSAL: &[&str] = &[
    "Adlam", "Ahom", "Balinese", "Batak", "Bhaiksuki", "Brahmi", "Buginese", "Buhid", "Chakma",
    "Cham", "Chorasmian", "Cypro_Minoan", "Dives_Akuru", "Dogra", "Duployan",
    "Egyptian_Hieroglyphs", "Elymaic", "Garay", "Grantha", "Gunjala_Gondi", "Gurung_Khema",
    "Hanifi_Rohingya", "Hanunoo", "Javanese", "Kaithi", "Kawi", "Kayah_Li", "Kharoshthi",
    "Khitan_Small_Script", "Khojki", "Khudawadi", "Kirat_Rai", "Lepcha", "Limbu", "Mahajani",
    "Makasar", "Mandaic", "Manichaean", "Marchen", "Masaram_Gondi", "Medefaidrin", "Meetei_Mayek",
    "Miao", "Modi", "Mongolian", "Multani", "Nag_Mundari", "Nandinagari", "Newa", "Nko",
    "Nyiakeng_Puachue_Hmong", "Ol_Onal", "Old_Sogdian", "Old_Uyghur", "Pahawh_Hmong", "Phags_Pa",
    "Psalter_Pahlavi", "Rejang", "Saurashtra", "Sharada", "Siddham", "Sinhala", "Sogdian",
    "Soyombo", "Sundanese", "Sunuwar", "Syloti_Nagri", "Tagalog", "Tagbanwa", "Tai_Le", "Tai_Tham",
    "Tai_Viet", "Takri", "Tangsa", "Tibetan", "Tifinagh", "Tirhuta", "Todhri", "Toto",
    "Tulu_Tigalari", "Vithkuqi", "Wancho", "Yezidi", "Zanabazar_Square",
];

pub(crate) fn select(
    script: Option<super::unicode::Script>,
    direction: Direction,
    gsub_script: Option<Tag>,
    requested: Option<Tag>,
) -> &'static Shaper {
    let filed_under = |s: &[u8; 4]| gsub_script == Some(Tag::from_bytes(s));
    let no_script = filed_under(b"DFLT");
    let opted_out = no_script || filed_under(b"latn");

    if requested.is_some_and(|t| t.to_bytes().eq_ignore_ascii_case(b"qaag")) {
        return &myanmar::ZAWGYI_SHAPER;
    }

    match script.map(|s| s.name()) {
        Some("Hangul") => &hangul::SHAPER,
        Some("Khmer") => &khmer::SHAPER,
        Some("Thai") | Some("Lao") => &thai::SHAPER,
        Some("Hebrew") => &hebrew::SHAPER,
        Some(name @ ("Arabic" | "Syriac")) => {
            if direction.is_horizontal() && (!no_script || name == "Arabic") {
                &arabic::SHAPER
            } else {
                &DEFAULT
            }
        }
        Some("Myanmar") => {
            if opted_out || filed_under(b"mymr") {
                &DEFAULT
            } else {
                &myanmar::SHAPER
            }
        }
        Some(name) if INDIC.contains(&name) => {
            if opted_out {
                &DEFAULT
            } else if gsub_script.is_some_and(|t| t.to_bytes()[3] == b'3') {
                &universal::SHAPER
            } else {
                &indic::SHAPER
            }
        }
        Some(name) if UNIVERSAL.contains(&name) && !opted_out => &universal::SHAPER,
        _ => &DEFAULT,
    }
}
