use crate::daecore::daeshaper::buffer::Buffer;
use crate::daecore::daeshaper::generated::syllable_tables::DEAD;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Segment<T> {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) kind: T,
}

pub(crate) struct Segments {
    inline: [Segment<u8>; INLINE_SEGMENTS],
    len: usize,
    spill: alloc::vec::Vec<Segment<u8>>,
}

const INLINE_SEGMENTS: usize = 32;

impl Segments {
    pub(crate) fn new() -> Self {
        Segments {
            inline: [Segment { start: 0, end: 0, kind: 0 }; INLINE_SEGMENTS],
            len: 0,
            spill: alloc::vec::Vec::new(),
        }
    }

    pub(crate) fn push<T: Into<u8>>(&mut self, s: Segment<T>) {
        let s = Segment { start: s.start, end: s.end, kind: s.kind.into() };
        if !self.spill.is_empty() {
            self.spill.push(s);
        } else if self.len < INLINE_SEGMENTS {
            self.inline[self.len] = s;
            self.len += 1;
        } else {
            self.spill.reserve(INLINE_SEGMENTS * 2);
            self.spill.extend_from_slice(&self.inline);
            self.spill.push(s);
        }
    }

    pub(crate) fn as_slice(&self) -> &[Segment<u8>] {
        if self.spill.is_empty() { &self.inline[..self.len] } else { &self.spill }
    }
}

pub fn segment<const N: usize, T: Copy>(
    len: usize,
    transitions: &[[u16; N]],
    category_of: impl Fn(usize) -> u8,
    accept: impl Fn(u16) -> Option<T>,
    mut emit: impl FnMut(Segment<T>),
) {
    let mut at = 0;

    while at < len {
        let mut state = 0u16;
        let mut last: Option<(usize, T)> = None;

        let mut i = at;
        while i < len {
            let category = category_of(i) as usize;
            let Some(row) = transitions.get(state as usize) else { break };
            let Some(&next) = row.get(category) else { break };
            if next == DEAD {
                break;
            }
            state = next;
            i += 1;
            if let Some(kind) = accept(state) {
                last = Some((i, kind));
            }
        }

        match last {
            Some((end, kind)) => {
                emit(Segment { start: at, end, kind });
                at = end;
            }
            None => at += 1,
        }
    }
}

pub(crate) fn set_syllables<T: Copy + Into<u8>>(
    buffer: &mut Buffer,
    segments: &[Segment<T>],
) {
    for (index, segment) in segments.iter().enumerate() {
        let serial = ((index as u8 % 15) + 1) << 4;
        let value = serial | segment.kind.into();
        for info in &mut buffer.info[segment.start..segment.end] {
            info.syllable = value;
        }
    }
}

#[cfg(test)]
mod segments {
    use super::{INLINE_SEGMENTS, Segment, Segments};

    #[test]
    fn spilling_keeps_everything_that_came_before_it() {
        for n in [0, 1, INLINE_SEGMENTS - 1, INLINE_SEGMENTS, INLINE_SEGMENTS + 1, INLINE_SEGMENTS * 3 + 7] {
            let mut segs = Segments::new();
            for i in 0..n {
                segs.push(Segment { start: i, end: i + 1, kind: (i % 251) as u8 });
            }
            let out = segs.as_slice();
            assert_eq!(out.len(), n, "{n} pushed, {} came back", out.len());
            for (i, s) in out.iter().enumerate() {
                assert_eq!(
                    (s.start, s.end, s.kind), (i, i + 1, (i % 251) as u8),
                    "entry {i} of {n} is not what was pushed",
                );
            }
        }
    }

    #[test]
    fn the_slice_is_one_run_on_both_sides_of_the_boundary() {
        for n in [INLINE_SEGMENTS - 1, INLINE_SEGMENTS, INLINE_SEGMENTS + 1] {
            let mut segs = Segments::new();
            for i in 0..n {
                segs.push(Segment { start: i * 2, end: i * 2 + 1, kind: 0u8 });
            }
            let out = segs.as_slice();
            for w in out.windows(2) {
                assert_eq!(w[1].start, w[0].start + 2, "a gap at the inline/spill boundary, n = {n}");
            }
        }
    }
}
