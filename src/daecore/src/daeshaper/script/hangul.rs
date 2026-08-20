use crate::daecore::daeshaper::buffer::{Buffer, GlyphInfo};
use crate::daecore::daeshaper::face::Face;
use crate::daecore::daeshaper::ot::map::{feature_flags as ff, MapBuilder};
use crate::daecore::daeshaper::normalize;
use crate::daecore::daeshaper::plan::ShapePlan;
use super::{Shaper, ZeroWidthMarks};
use crate::daecore::daeshaper::ot::tag::Tag;

pub(crate) const SHAPER: Shaper = Shaper {
    name: "hangul",
    collect_features: Some(collect_features),
    pauses: &[],
    override_features: Some(override_features),
    preprocess_text: Some(preprocess_text),
    postprocess_glyphs: None,
    // The composing and decomposing here is font-aware in a way the normalizer is not, so the
    // normalizer is told to keep out of it entirely.
    normalization_preference: normalize::Mode::None,
    decompose: None,
    compose: None,
    setup_masks: Some(setup_masks),
    gpos_tag: None,
    reorder_marks: None,
    zero_width_marks: ZeroWidthMarks::Never,
    fallback_position: false,
};

const LJMO: u8 = 1;
const VJMO: u8 = 2;
const TJMO: u8 = 3;

const FEATURES: [&[u8; 4]; 3] = [b"ljmo", b"vjmo", b"tjmo"];

const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;
const S_BASE: u32 = 0xAC00;

fn is_composing_l(u: u32) -> bool {
    (L_BASE..L_BASE + L_COUNT).contains(&u)
}

fn is_composing_v(u: u32) -> bool {
    (V_BASE..V_BASE + V_COUNT).contains(&u)
}

fn is_composing_t(u: u32) -> bool {
    (T_BASE + 1..T_BASE + T_COUNT).contains(&u)
}

fn is_precomposed(u: u32) -> bool {
    (S_BASE..S_BASE + S_COUNT).contains(&u)
}

fn is_l(u: u32) -> bool {
    (0x1100..=0x115F).contains(&u) || (0xA960..=0xA97C).contains(&u)
}

fn is_v(u: u32) -> bool {
    (0x1160..=0x11A7).contains(&u) || (0xD7B0..=0xD7C6).contains(&u)
}

fn is_t(u: u32) -> bool {
    (0x11A8..=0x11FF).contains(&u) || (0xD7CB..=0xD7FB).contains(&u)
}

fn is_tone(u: u32) -> bool {
    (0x302E..=0x302F).contains(&u)
}

fn collect_features(b: &mut MapBuilder, _: Option<Tag>) {
    for feature in FEATURES {
        b.add_feature(Tag::from_bytes(feature), ff::NONE, 1);
    }
}

fn override_features(b: &mut MapBuilder) {
    b.add_feature(Tag::from_bytes(b"calt"), ff::NONE, 1);
}

impl GlyphInfo {
    fn jamo_feature(&self) -> u8 {
        self.shaper_auxiliary
    }

    fn set_jamo_feature(&mut self, feature: u8) {
        self.shaper_auxiliary = feature;
    }
}

fn is_zero_width(face: &Face, u: u32) -> bool {
    face.glyph_index(u).is_some_and(|g| face.glyph_h_advance(g) == 0)
}

fn preprocess_text(_: &ShapePlan, face: &Face, buffer: &mut Buffer) {
    buffer.clear_output();
    buffer.idx = 0;

    let mut start = 0;
    let mut end = 0;

    while buffer.idx < buffer.len {
        let u = buffer.cur(0).id;

        if is_tone(u) {
            handle_tone(face, buffer, u, start, end);
            start = buffer.out_len;
            end = buffer.out_len;
            continue;
        }

        start = buffer.out_len;

        if is_l(u) && buffer.idx + 1 < buffer.len && is_v(buffer.cur(1).id) {
            end = handle_jamo_sequence(face, buffer, start);
            continue;
        }

        if is_precomposed(u)
            && let Some(new_end) = handle_precomposed(face, buffer, start) {
                end = new_end;
                continue;
            }

        buffer.next_glyph();
    }

    buffer.sync();
}

fn handle_tone(face: &Face, buffer: &mut Buffer, u: u32, start: usize, end: usize) {
    if start < end && end == buffer.out_len {
        buffer.unsafe_to_break_from_outbuffer(start, buffer.idx);
        buffer.next_glyph();

        if !is_zero_width(face, u) {
            buffer.merge_out_grapheme_clusters(start, end + 1);
            let out = buffer.out_info_mut();
            let tone = out[end];
            for i in (0..end - start).rev() {
                out[i + start + 1] = out[i + start];
            }
            out[start] = tone;
        }
        return;
    }

    if buffer.insert_dotted_circle && face.has_glyph(0x25CC) {
        let pair = if is_zero_width(face, u) { [0x25CC, u] } else { [u, 0x25CC] };
        buffer.replace_glyphs(1, &pair);
    } else {
        buffer.next_glyph();
    }
}

fn handle_jamo_sequence(face: &Face, buffer: &mut Buffer, start: usize) -> usize {
    let l = buffer.cur(0).id;
    let v = buffer.cur(1).id;

    let t = if buffer.idx + 2 < buffer.len {
        let candidate = buffer.cur(2).id;
        if is_t(candidate) { candidate } else { 0 }
    } else {
        0
    };

    let count = if t != 0 { 3 } else { 2 };
    let idx = buffer.idx;
    buffer.unsafe_to_break(idx, idx + count);

    if is_composing_l(l) && is_composing_v(v) && (t == 0 || is_composing_t(t)) {
        let tindex = if t != 0 { t - T_BASE } else { 0 };
        let s = S_BASE + (l - L_BASE) * N_COUNT + (v - V_BASE) * T_COUNT + tindex;

        if face.has_glyph(s) {
            buffer.replace_glyphs(count, &[s]);
            return start + 1;
        }
    }

    buffer.cur_mut(0).set_jamo_feature(LJMO);
    buffer.next_glyph();
    buffer.cur_mut(0).set_jamo_feature(VJMO);
    buffer.next_glyph();

    let end = if t != 0 {
        buffer.cur_mut(0).set_jamo_feature(TJMO);
        buffer.next_glyph();
        start + 3
    } else {
        start + 2
    };

    buffer.merge_out_grapheme_clusters(start, end);
    end
}

fn handle_precomposed(face: &Face, buffer: &mut Buffer, start: usize) -> Option<usize> {
    let s = buffer.cur(0).id;
    let has_glyph = face.has_glyph(s);

    let lindex = (s - S_BASE) / N_COUNT;
    let nindex = (s - S_BASE) % N_COUNT;
    let vindex = nindex / T_COUNT;
    let tindex = nindex % T_COUNT;

    if tindex == 0 && buffer.idx + 1 < buffer.len && is_composing_t(buffer.cur(1).id) {
        let combined = s + (buffer.cur(1).id - T_BASE);
        if face.has_glyph(combined) {
            buffer.replace_glyphs(2, &[combined]);
            return Some(start + 1);
        }
        let idx = buffer.idx;
        buffer.unsafe_to_break(idx, idx + 2);
    }

    let trailing_t = tindex == 0 && buffer.idx + 1 < buffer.len && is_t(buffer.cur(1).id);
    if !has_glyph || trailing_t {
        let pieces = [L_BASE + lindex, V_BASE + vindex, T_BASE + tindex];
        let drawable = face.has_glyph(pieces[0])
            && face.has_glyph(pieces[1])
            && (tindex == 0 || face.has_glyph(pieces[2]));

        if drawable {
            let mut count = if tindex != 0 { 3 } else { 2 };
            buffer.replace_glyphs(1, &pieces[..count]);

            if has_glyph && tindex == 0 {
                buffer.next_glyph();
                count += 1;
            }

            let end = start + count;
            let out = buffer.out_info_mut();
            out[start].set_jamo_feature(LJMO);
            out[start + 1].set_jamo_feature(VJMO);
            if start + 2 < end {
                out[start + 2].set_jamo_feature(TJMO);
            }

            buffer.merge_out_grapheme_clusters(start, end);
            return Some(end);
        }
    }

    if has_glyph {
        buffer.next_glyph();
        return Some(start + 1);
    }

    None
}

fn setup_masks(plan: &ShapePlan, _: &Face, buffer: &mut Buffer) {
    let masks = [
        0,
        plan.map.one_mask(Tag::from_bytes(b"ljmo")),
        plan.map.one_mask(Tag::from_bytes(b"vjmo")),
        plan.map.one_mask(Tag::from_bytes(b"tjmo")),
    ];

    let calt = plan.map.one_mask(Tag::from_bytes(b"calt"));

    let len = buffer.len;
    for info in &mut buffer.info[..len] {
        if let Some(&mask) = masks.get(info.jamo_feature() as usize) {
            info.mask |= mask;
        }
        let u = info.id;
        if is_l(u) || is_v(u) || is_t(u) {
            info.mask &= !calt;
        }
    }
}
