const SHIFTS: [u32; 3] = [4, 0, 9];

const MASK_WORDS: usize = 4;

const MASK_BITS: usize = MASK_WORDS * 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Digest {
    masks: [[u64; MASK_WORDS]; 3],
}

impl Digest {
    pub fn new() -> Self {
        Digest { masks: [[0; MASK_WORDS]; 3] }
    }

    pub(crate) fn full() -> Self {
        Digest { masks: [[u64::MAX; MASK_WORDS]; 3] }
    }

    fn bit(glyph: u16, shift: u32) -> (usize, u64) {
        let at = ((glyph as usize) >> shift) & (MASK_BITS - 1);
        (at / 64, 1u64 << (at % 64))
    }

    pub(crate) fn add(&mut self, glyph: u16) {
        for (i, shift) in SHIFTS.iter().enumerate() {
            let (word, bit) = Self::bit(glyph, *shift);
            self.masks[i][word] |= bit;
        }
    }

    pub(crate) fn add_range(&mut self, first: u16, last: u16) {
        if first > last {
            return;
        }
        for (i, shift) in SHIFTS.iter().enumerate() {
            let lo = (first as usize) >> shift;
            let hi = (last as usize) >> shift;

            if hi - lo >= MASK_BITS - 1 {
                self.masks[i] = [u64::MAX; MASK_WORDS];
                continue;
            }

            let (a, b) = (lo & (MASK_BITS - 1), hi & (MASK_BITS - 1));
            if a <= b {
                set_span(&mut self.masks[i], a, b);
            } else {
                set_span(&mut self.masks[i], a, MASK_BITS - 1);
                set_span(&mut self.masks[i], 0, b);
            }
        }
    }

    pub(crate) fn union(&mut self, other: &Digest) {
        for i in 0..3 {
            for w in 0..MASK_WORDS {
                self.masks[i][w] |= other.masks[i][w];
            }
        }
    }

    pub(crate) fn may_intersect(&self, other: &Digest) -> bool {
        (0..3).all(|i| (0..MASK_WORDS).any(|w| self.masks[i][w] & other.masks[i][w] != 0))
    }

    pub(crate) fn may_have(&self, glyph: u16) -> bool {
        SHIFTS.iter().enumerate().all(|(i, shift)| {
            let (word, bit) = Self::bit(glyph, *shift);
            self.masks[i][word] & bit != 0
        })
    }
}

fn set_span(mask: &mut [u64; MASK_WORDS], from: usize, to: usize) {
    let (first, last) = (from / 64, to / 64);
    if first == last {
        mask[first] |= word_span(from % 64, to % 64);
        return;
    }
    mask[first] |= word_span(from % 64, 63);
    for word in &mut mask[first + 1..last] {
        *word = u64::MAX;
    }
    mask[last] |= word_span(0, to % 64);
}

fn word_span(from: usize, to: usize) -> u64 {
    let width = to - from + 1;
    if width == 64 { u64::MAX } else { ((1u64 << width) - 1) << from }
}

impl Default for Digest {
    fn default() -> Self {
        Digest::new()
    }
}
