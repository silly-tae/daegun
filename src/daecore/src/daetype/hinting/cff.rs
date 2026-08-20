use alloc::vec::Vec;

use super::f26dot6;
use super::auto::AutoPoints;
use crate::daecore::daetype::outline::cff_pen::CffHints;

#[derive(Clone, Copy, Debug)]
struct Mapped {
    min: f32,
    max: f32,
    min_fit: i32,
    max_fit: i32,
}

fn active(mask: &[u8], stem: usize) -> bool {
    mask.get(stem / 8).is_some_and(|b| b & (0x80 >> (stem % 8)) != 0)
}

pub fn apply(pts: &AutoPoints, hints: &CffHints, ppem: u16, upm: u16) -> Option<Vec<i32>> {
    if upm == 0 || hints.stems.iter().all(|s| s.vertical) {
        return None;
    }
    let scale = |v: f32| f26dot6::scale(v as i32, ppem, upm);

    let mapped: Vec<Option<Mapped>> = hints
        .stems
        .iter()
        .map(|s| {
            if s.vertical {
                return None;
            }
            let (min, max) = (s.min.min(s.max), s.min.max(s.max));
            let min_fit = f26dot6::round_to_grid(scale(min));
            let width = f26dot6::round_to_grid(scale(max) - scale(min)).max(f26dot6::ONE);
            Some(Mapped { min, max, min_fit, max_fit: min_fit + width })
        })
        .collect();

    if mapped.iter().all(Option::is_none) {
        return None;
    }

    let mut out: Vec<i32> = pts.y.iter().map(|&v| scale(v)).collect();
    let all_on = alloc::vec![0xFFu8; hints.stems.len().div_ceil(8).max(1)];
    let mut mask: &[u8] = &all_on;
    let mut next = 0usize;

    for (i, o) in out.iter_mut().enumerate() {
        while next < hints.masks.len() && hints.masks[next].0 <= i {
            mask = &hints.masks[next].1;
            next += 1;
        }
        *o = map_one(pts.y[i], &mapped, mask, &scale);
    }
    Some(out)
}

fn map_one(y: f32, mapped: &[Option<Mapped>], mask: &[u8], scale: &impl Fn(f32) -> i32) -> i32 {
    let scaled = scale(y);
    let mut nearest: Option<(f32, i32)> = None;

    for (i, m) in mapped.iter().enumerate() {
        let Some(m) = m else { continue };
        if !active(mask, i) {
            continue;
        }
        if y >= m.min && y <= m.max {
            let span = m.max - m.min;
            return if span > 0.0 {
                let t = (y - m.min) / span;
                m.min_fit + ((m.max_fit - m.min_fit) as f32 * t) as i32
            } else {
                m.min_fit
            };
        }
        for (edge, fitted) in [(m.min, m.min_fit), (m.max, m.max_fit)] {
            let d = (y - edge).abs();
            if nearest.is_none_or(|(bd, _)| d < bd) {
                nearest = Some((d, fitted - scale(edge)));
            }
        }
    }
    match nearest {
        Some((_, shift)) => scaled + shift,
        None => scaled,
    }
}
