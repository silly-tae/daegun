use crate::daecore::daeshaper::generated::indic_tables as t;

#[allow(dead_code, reason = "generated category space; the machines match a subset")]
pub(crate) mod category {
    pub(crate) const X: u8 = 0;
    pub(crate) const C: u8 = 1;
    pub(crate) const V: u8 = 2;
    pub(crate) const N: u8 = 3;
    pub(crate) const H: u8 = 4;
    pub(crate) const ZWNJ: u8 = 5;
    pub(crate) const ZWJ: u8 = 6;
    pub(crate) const M: u8 = 7;
    pub(crate) const SM: u8 = 8;
    pub(crate) const A: u8 = 9;
    pub(crate) const PLACEHOLDER: u8 = 10;
    pub(crate) const DOTTEDCIRCLE: u8 = 11;
    pub(crate) const RS: u8 = 12;
    pub(crate) const MPST: u8 = 13;
    pub(crate) const REPHA: u8 = 14;
    pub(crate) const RA: u8 = 15;
    pub(crate) const CM: u8 = 16;
    pub(crate) const SYMBOL: u8 = 17;
    pub(crate) const CS: u8 = 18;
    pub(crate) const V_ABV: u8 = 19;
    pub(crate) const V_BLW: u8 = 20;
    pub(crate) const V_PRE: u8 = 21;
    pub(crate) const V_PST: u8 = 22;
    pub(crate) const VS: u8 = 23;
    pub(crate) const MW: u8 = 24;
    pub(crate) const MY: u8 = 25;
    pub(crate) const MR: u8 = 26;
    pub(crate) const MH: u8 = 27;
    pub(crate) const ML: u8 = 28;
    pub(crate) const PT: u8 = 29;
    pub(crate) const AS: u8 = 30;
    pub(crate) const ROBATIC: u8 = 31;
    pub(crate) const XGROUP: u8 = 32;
    pub(crate) const YGROUP: u8 = 33;
    pub(crate) const SMPST: u8 = 34;
}

#[allow(dead_code, reason = "the ordering is the specification; the pass names a subset")]
pub(crate) mod position {
    pub(crate) const START: u8 = 0;
    pub(crate) const RA_TO_BECOME_REPH: u8 = 1;
    pub(crate) const PRE_M: u8 = 2;
    pub(crate) const PRE_C: u8 = 3;
    pub(crate) const BASE_C: u8 = 4;
    pub(crate) const AFTER_MAIN: u8 = 5;
    pub(crate) const ABOVE_C: u8 = 6;
    pub(crate) const BEFORE_SUB: u8 = 7;
    pub(crate) const BELOW_C: u8 = 8;
    pub(crate) const AFTER_SUB: u8 = 9;
    pub(crate) const BEFORE_POST: u8 = 10;
    pub(crate) const POST_C: u8 = 11;
    pub(crate) const AFTER_POST: u8 = 12;
    pub(crate) const SMVD: u8 = 13;
    pub(crate) const END: u8 = 14;
}

pub(crate) fn lookup(c: u32) -> (u8, u8) {
    const LEAF_BITS: u32 = t::INDIC_CATEGORY_LEAF_BITS;
    const MID_BITS: u32 = t::INDIC_CATEGORY_MID_BITS;
    if c >= 0x0011_0000 {
        return (category::X, position::END);
    }
    let mid = t::INDIC_CATEGORY_TOP[(c >> (LEAF_BITS + MID_BITS)) as usize] as usize;
    let leaf = t::INDIC_CATEGORY_MID
        [(mid << MID_BITS) | ((c >> LEAF_BITS) & ((1 << MID_BITS) - 1)) as usize] as usize;
    let rec = &t::INDIC_CATEGORY
        [t::INDIC_CATEGORY_LEAF[(leaf << LEAF_BITS) | (c & ((1 << LEAF_BITS) - 1)) as usize] as usize];
    (rec.category, rec.position)
}
