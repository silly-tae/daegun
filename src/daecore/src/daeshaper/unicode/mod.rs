pub mod bidi;
pub mod linebreak;
pub mod segment;
pub mod itemize;

use super::buffer::Direction;
use super::generated::unicode_tables as t;

#[inline]
pub(crate) fn props(c: char) -> &'static t::Props {
    const LEAF_BITS: u32 = t::PROPS_LEAF_BITS;
    const MID_BITS: u32 = t::PROPS_MID_BITS;
    const LEAF_MASK: u32 = (1 << LEAF_BITS) - 1;
    const MID_MASK: u32 = (1 << MID_BITS) - 1;

    let cp = c as u32;
    let mid = t::PROPS_TOP[(cp >> (LEAF_BITS + MID_BITS)) as usize] as usize;
    let leaf = t::PROPS_MID[(mid << MID_BITS) | ((cp >> LEAF_BITS) & MID_MASK) as usize] as usize;
    &t::PROPS[t::PROPS_LEAF[(leaf << LEAF_BITS) | (cp & LEAF_MASK) as usize] as usize]
}

fn lookup_pair(table: &[(u32, u32)], key: u32) -> Option<u32> {
    table
        .binary_search_by_key(&key, |&(k, _)| k)
        .ok()
        .map(|i| table[i].1)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum GeneralCategory {
    Unassigned = 0,
    Control,
    Format,
    PrivateUse,
    Surrogate,
    LowercaseLetter,
    ModifierLetter,
    OtherLetter,
    TitlecaseLetter,
    UppercaseLetter,
    SpacingMark,
    EnclosingMark,
    NonspacingMark,
    DecimalNumber,
    LetterNumber,
    OtherNumber,
    ConnectPunctuation,
    DashPunctuation,
    ClosePunctuation,
    FinalPunctuation,
    InitialPunctuation,
    OtherPunctuation,
    OpenPunctuation,
    CurrencySymbol,
    ModifierSymbol,
    MathSymbol,
    OtherSymbol,
    LineSeparator,
    ParagraphSeparator,
    SpaceSeparator,
}

impl GeneralCategory {
    pub(crate) fn from_stored(v: u16) -> GeneralCategory {
        GeneralCategory::from_raw(v as u8)
    }

    fn from_raw(v: u8) -> GeneralCategory {
        match v {
            1 => GeneralCategory::Control,
            2 => GeneralCategory::Format,
            3 => GeneralCategory::PrivateUse,
            4 => GeneralCategory::Surrogate,
            5 => GeneralCategory::LowercaseLetter,
            6 => GeneralCategory::ModifierLetter,
            7 => GeneralCategory::OtherLetter,
            8 => GeneralCategory::TitlecaseLetter,
            9 => GeneralCategory::UppercaseLetter,
            10 => GeneralCategory::SpacingMark,
            11 => GeneralCategory::EnclosingMark,
            12 => GeneralCategory::NonspacingMark,
            13 => GeneralCategory::DecimalNumber,
            14 => GeneralCategory::LetterNumber,
            15 => GeneralCategory::OtherNumber,
            16 => GeneralCategory::ConnectPunctuation,
            17 => GeneralCategory::DashPunctuation,
            18 => GeneralCategory::ClosePunctuation,
            19 => GeneralCategory::FinalPunctuation,
            20 => GeneralCategory::InitialPunctuation,
            21 => GeneralCategory::OtherPunctuation,
            22 => GeneralCategory::OpenPunctuation,
            23 => GeneralCategory::CurrencySymbol,
            24 => GeneralCategory::ModifierSymbol,
            25 => GeneralCategory::MathSymbol,
            26 => GeneralCategory::OtherSymbol,
            27 => GeneralCategory::LineSeparator,
            28 => GeneralCategory::ParagraphSeparator,
            29 => GeneralCategory::SpaceSeparator,
            _ => GeneralCategory::Unassigned,
        }
    }

    pub(crate) fn is_mark(self) -> bool {
        matches!(
            self,
            GeneralCategory::SpacingMark
                | GeneralCategory::EnclosingMark
                | GeneralCategory::NonspacingMark
        )
    }

    pub(crate) fn is_letter(self) -> bool {
        matches!(
            self,
            GeneralCategory::LowercaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::UppercaseLetter
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Script(pub u16);

impl Script {
    pub fn name(self) -> &'static str {
        t::SCRIPT_NAMES.get(self.0 as usize).copied().unwrap_or("Unknown")
    }

    pub fn is_rtl(self) -> Option<bool> {
        horizontal_direction(self).map(|d| d == super::buffer::Direction::RightToLeft)
    }

    pub fn opentype_tags(self) -> alloc::vec::Vec<alloc::string::String> {
        use alloc::string::ToString;
        super::ot::tag::script_tags(self)
            .as_slice()
            .iter()
            .filter_map(|t| core::str::from_utf8(&t.to_bytes()).ok().map(ToString::to_string))
            .collect()
    }

    pub fn is_context_dependent(self) -> bool {
        self.0 == t::SCRIPT_COMMON
            || self.0 == t::SCRIPT_INHERITED
            || self.0 as usize >= t::SCRIPT_NAMES.len()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub(crate) enum JoiningType {
    NonJoining = 0,
    LeftJoining = 1,
    RightJoining = 2,
    DualJoining = 3,
    Alaph = 4,
    DalathRish = 5,
    Transparent = 7,
}

impl JoiningType {
    fn from_raw(v: u8) -> JoiningType {
        match v {
            1 => JoiningType::LeftJoining,
            2 => JoiningType::RightJoining,
            3 => JoiningType::DualJoining,
            4 => JoiningType::Alaph,
            5 => JoiningType::DalathRish,
            7 => JoiningType::Transparent,
            _ => JoiningType::NonJoining,
        }
    }
}

pub fn general_category(c: char) -> GeneralCategory {
    GeneralCategory::from_raw(props(c).general_category)
}

pub(crate) fn combining_class(c: char) -> u8 {
    props(c).combining_class
}

pub(crate) fn modified_combining_class(c: char) -> u8 {
    match c {
        '\u{1A60}' => return 254,
        '\u{0FC6}' => return 254,
        '\u{0F39}' => return 127,
        _ => {}
    }
    remap_combining_class(combining_class(c))
}

pub(crate) fn mirrored(c: char) -> Option<char> {
    let key = u16::try_from(c as u32).ok()?;
    t::MIRRORING
        .binary_search_by_key(&key, |&(k, _)| k)
        .ok()
        .and_then(|i| char::from_u32(u32::from(t::MIRRORING[i].1)))
}

pub fn vertical_form(c: char) -> Option<char> {
    Some(match c {
        '\u{2013}' => '\u{FE32}',
        '\u{2014}' => '\u{FE31}',
        '\u{2025}' => '\u{FE30}',
        '\u{2026}' => '\u{FE19}',
        '\u{3001}' => '\u{FE11}',
        '\u{3002}' => '\u{FE12}',
        '\u{3008}' => '\u{FE3F}',
        '\u{3009}' => '\u{FE40}',
        '\u{300A}' => '\u{FE3D}',
        '\u{300B}' => '\u{FE3E}',
        '\u{300C}' => '\u{FE41}',
        '\u{300D}' => '\u{FE42}',
        '\u{300E}' => '\u{FE43}',
        '\u{300F}' => '\u{FE44}',
        '\u{3010}' => '\u{FE3B}',
        '\u{3011}' => '\u{FE3C}',
        '\u{3014}' => '\u{FE39}',
        '\u{3015}' => '\u{FE3A}',
        '\u{3016}' => '\u{FE17}',
        '\u{3017}' => '\u{FE18}',
        '\u{FE4F}' => '\u{FE34}',
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum VerticalOrientation {
    Rotated = 0,
    TransformedRotated,
    TransformedUpright,
    Upright,
}

pub(crate) fn vertical_orientation(c: char) -> VerticalOrientation {
    match props(c).vertical_orientation {
        1 => VerticalOrientation::TransformedRotated,
        2 => VerticalOrientation::TransformedUpright,
        3 => VerticalOrientation::Upright,
        _ => VerticalOrientation::Rotated,
    }
}

pub fn is_upright(c: char, has_vertical_form: bool) -> bool {
    match vertical_orientation(c) {
        VerticalOrientation::Upright | VerticalOrientation::TransformedUpright => true,
        VerticalOrientation::TransformedRotated => has_vertical_form,
        VerticalOrientation::Rotated => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SpaceFallback {
    Space,
    EmDiv(u8),
    Figure,
    Punctuation,
    Narrow,
    Em4Of18,
}

impl SpaceFallback {
    pub(crate) fn to_byte(self) -> u8 {
        match self {
            SpaceFallback::Space => 1,
            SpaceFallback::Figure => 2,
            SpaceFallback::Punctuation => 3,
            SpaceFallback::Narrow => 4,
            SpaceFallback::Em4Of18 => 5,
            SpaceFallback::EmDiv(n) => 16u8.saturating_add(n),
        }
    }

    pub(crate) fn from_byte(b: u8) -> Option<SpaceFallback> {
        Some(match b {
            1 => SpaceFallback::Space,
            2 => SpaceFallback::Figure,
            3 => SpaceFallback::Punctuation,
            4 => SpaceFallback::Narrow,
            5 => SpaceFallback::Em4Of18,
            17.. => SpaceFallback::EmDiv(b - 16),
            _ => return None,
        })
    }
}

pub(crate) fn is_variation_selector(c: char) -> bool {
    matches!(
        c,
        '\u{180B}'..='\u{180D}' | '\u{180F}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}'
    )
}

pub(crate) fn space_fallback(c: char) -> Option<SpaceFallback> {
    use SpaceFallback::*;
    Some(match c {
        '\u{0020}' => Space,
        '\u{00A0}' => Space,
        '\u{2000}' => EmDiv(2),
        '\u{2001}' => EmDiv(1),
        '\u{2002}' => EmDiv(2),
        '\u{2003}' => EmDiv(1),
        '\u{2004}' => EmDiv(3),
        '\u{2005}' => EmDiv(4),
        '\u{2006}' => EmDiv(6),
        '\u{2007}' => Figure,
        '\u{2008}' => Punctuation,
        '\u{2009}' => EmDiv(5),
        '\u{200A}' => EmDiv(16),
        '\u{202F}' => Narrow,
        '\u{205F}' => Em4Of18,
        '\u{3000}' => EmDiv(1),
        _ => return None,
    })
}

fn remap_combining_class(ccc: u8) -> u8 {
    match ccc {
        10 => 22,
        11 => 15,
        12 => 16,
        13 => 17,
        14 => 23,
        15 => 18,
        16 => 19,
        17 => 20,
        18 => 21,
        19 => 14,
        20 => 24,
        21 => 12,
        22 => 25,
        23 => 13,
        24 => 10,
        25 => 11,
        27 => 28,
        28 => 29,
        29 => 30,
        30 => 31,
        31 => 32,
        32 => 33,
        33 => 27,
        84 | 91 => 0,
        103 => 3,
        130 => 132,
        132 => 131,
        other => other,
    }
}

pub(crate) fn script(c: char) -> Script {
    Script(props(c).script)
}

pub(crate) fn is_extended_pictographic(c: char) -> bool {
    props(c).extended_pictographic != 0
}

const RTL_SCRIPTS: &[&str] = &[
    "Adlam", "Arabic", "Avestan", "Chorasmian", "Cypriot", "Elymaic", "Hanifi_Rohingya", "Hatran",
    "Hebrew", "Imperial_Aramaic", "Inscriptional_Pahlavi", "Inscriptional_Parthian", "Kharoshthi",
    "Lydian", "Mandaic", "Manichaean", "Mende_Kikakui", "Meroitic_Cursive", "Meroitic_Hieroglyphs",
    "Nabataean", "Nko", "Old_North_Arabian", "Old_Sogdian", "Old_South_Arabian", "Old_Turkic",
    "Old_Uyghur", "Palmyrene", "Phoenician", "Psalter_Pahlavi", "Samaritan", "Sogdian", "Syriac",
    "Thaana", "Yezidi",
];

const UNSETTLED_DIRECTION: &[&str] = &["Old_Hungarian", "Old_Italic", "Runic", "Tifinagh"];

pub(crate) fn horizontal_direction(s: Script) -> Option<Direction> {
    let name = s.name();
    if RTL_SCRIPTS.contains(&name) {
        Some(Direction::RightToLeft)
    } else if UNSETTLED_DIRECTION.contains(&name) {
        None
    } else {
        Some(Direction::LeftToRight)
    }
}

pub(crate) fn is_default_ignorable(c: char) -> bool {
    props(c).default_ignorable != 0
}

pub(crate) fn joining_type(c: char) -> JoiningType {
    let raw = props(c).joining_type;
    if raw != u8::MAX {
        return JoiningType::from_raw(raw);
    }
    match general_category(c) {
        GeneralCategory::NonspacingMark | GeneralCategory::EnclosingMark | GeneralCategory::Format => {
            JoiningType::Transparent
        }
        _ => JoiningType::NonJoining,
    }
}

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

fn decompose_hangul(ab: char) -> Option<(char, Option<char>)> {
    let si = (ab as u32).checked_sub(S_BASE)?;
    if si >= S_COUNT {
        return None;
    }
    if si % T_COUNT != 0 {
        let lv = char::from_u32(S_BASE + (si / T_COUNT) * T_COUNT)?;
        let t = char::from_u32(T_BASE + si % T_COUNT)?;
        Some((lv, Some(t)))
    } else {
        let l = char::from_u32(L_BASE + si / N_COUNT)?;
        let v = char::from_u32(V_BASE + (si % N_COUNT) / T_COUNT)?;
        Some((l, Some(v)))
    }
}

fn compose_hangul(a: char, b: char) -> Option<char> {
    let (a, b) = (a as u32, b as u32);
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        return char::from_u32(S_BASE + ((a - L_BASE) * V_COUNT + (b - V_BASE)) * T_COUNT);
    }
    if (S_BASE..S_BASE + S_COUNT).contains(&a)
        && (a - S_BASE).is_multiple_of(T_COUNT)
        && (T_BASE + 1..T_BASE + T_COUNT).contains(&b)
    {
        return char::from_u32(a + (b - T_BASE));
    }
    None
}

pub(crate) fn decompose(ab: char) -> Option<(char, Option<char>)> {
    if let Some(h) = decompose_hangul(ab) {
        return Some(h);
    }
    let key = ab as u32;
    if let Some((a, b)) = decompose_pair_of(key) {
        return Some((char::from_u32(a)?, char::from_u32(b)));
    }
    if let Some(&a) = key
        .checked_sub(t::DECOMPOSE_SINGLE_CJK_BASE)
        .and_then(|i| t::DECOMPOSE_SINGLE_CJK.get(i as usize))
    {
        return char::from_u32(a).map(|a| (a, None));
    }
    lookup_pair(t::DECOMPOSE_SINGLE, key).and_then(char::from_u32).map(|a| (a, None))
}

fn decompose_pair_of(key: u32) -> Option<(u32, u32)> {
    if let Ok(k) = u16::try_from(key) {
        let i = t::DECOMPOSE_PAIR_BMP.binary_search_by_key(&k, |&(k, _, _)| k).ok()?;
        let (_, a, b) = t::DECOMPOSE_PAIR_BMP[i];
        return Some((u32::from(a), u32::from(b)));
    }
    let i = t::DECOMPOSE_PAIR_SUPP.binary_search_by_key(&key, |&(k, _, _)| k).ok()?;
    let (_, a, b) = t::DECOMPOSE_PAIR_SUPP[i];
    Some((a, b))
}

fn decompose_pair_at(i: usize) -> (u32, u32, u32) {
    match t::DECOMPOSE_PAIR_BMP.get(i) {
        Some(&(k, a, b)) => (u32::from(k), u32::from(a), u32::from(b)),
        None => t::DECOMPOSE_PAIR_SUPP[i - t::DECOMPOSE_PAIR_BMP.len()],
    }
}

pub(crate) fn compose(a: char, b: char) -> Option<char> {
    if let Some(h) = compose_hangul(a, b) {
        return Some(h);
    }
    let (a, b) = (a as u32, b as u32);
    t::COMPOSE_INDEX
        .binary_search_by(|&i| {
            let (_, ka, kb) = decompose_pair_at(i as usize);
            (ka, kb).cmp(&(a, b))
        })
        .ok()
        .and_then(|i| char::from_u32(decompose_pair_at(t::COMPOSE_INDEX[i] as usize).0))
}
