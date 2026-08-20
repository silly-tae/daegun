pub(crate) struct PaintLayout {
    pub(crate) inline_len: usize,
    pub(crate) children: &'static [usize],
    pub(crate) glyph_ids: &'static [usize],
}

const NONE: &[usize] = &[];
const CHILD_AT_1: &[usize] = &[1];

pub(crate) fn paint_layout(format: u8) -> Option<PaintLayout> {
    let (inline_len, children, glyph_ids) = match format {
        1 => (6, NONE, NONE),
        2 => (5, NONE, NONE),
        3 => (9, NONE, NONE),
        4 => (16, NONE, NONE),
        5 => (20, NONE, NONE),
        6 => (16, NONE, NONE),
        7 => (20, NONE, NONE),
        8 => (12, NONE, NONE),
        9 => (16, NONE, NONE),
        10 => (6, CHILD_AT_1, &[4usize][..]),
        11 => (3, NONE, &[1usize][..]),
        12 => (7, CHILD_AT_1, NONE),
        13 => (7, CHILD_AT_1, NONE),
        14 => (8, CHILD_AT_1, NONE),
        15 => (12, CHILD_AT_1, NONE),
        16 => (8, CHILD_AT_1, NONE),
        17 => (12, CHILD_AT_1, NONE),
        18 => (12, CHILD_AT_1, NONE),
        19 => (16, CHILD_AT_1, NONE),
        20 => (6, CHILD_AT_1, NONE),
        21 => (10, CHILD_AT_1, NONE),
        22 => (10, CHILD_AT_1, NONE),
        23 => (14, CHILD_AT_1, NONE),
        24 => (6, CHILD_AT_1, NONE),
        25 => (10, CHILD_AT_1, NONE),
        26 => (10, CHILD_AT_1, NONE),
        27 => (14, CHILD_AT_1, NONE),
        28 => (8, CHILD_AT_1, NONE),
        29 => (12, CHILD_AT_1, NONE),
        30 => (12, CHILD_AT_1, NONE),
        31 => (16, CHILD_AT_1, NONE),
        32 => (8, &[1usize, 5][..], NONE),
        _ => return None,
    };
    Some(PaintLayout { inline_len, children, glyph_ids })
}
