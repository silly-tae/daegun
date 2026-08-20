use alloc::collections::BTreeMap;
use alloc::vec::Vec;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct GlyphKey {
    pub gid: u16,
    pub px_bits: u32,
    pub layout: u64,
    pub gamma_bits: Option<u32>,
    pub transform_bits: Option<[u32; 6]>,
    pub hinting: u8,
    pub stroke: Option<(u32, u8, u32, u8)>,
    pub embolden_bits: Option<u32>,
    pub oblique_bits: Option<u32>,
    pub axes: crate::sync::Shared<crate::daecore::cache::AxisKey>,
}

#[derive(Clone)]
pub(crate) struct CachedGlyph {
    pub bitmap: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
    pub bounds: crate::daerizer::daecpu::math::OutlineBounds,
}

impl CachedGlyph {
    fn cost(&self) -> usize {
        self.bitmap.len() + 64
    }
}

pub(crate) struct ByteLru<K: Ord + Clone, V> {
    entries: BTreeMap<K, (V, u64)>,
    by_age: BTreeMap<u64, K>,
    clock: u64,
    bytes: usize,
    budget: usize,
    cost: fn(&V) -> usize,
}

impl<K: Ord + Clone, V> ByteLru<K, V> {
    pub(crate) fn new(budget: usize, cost: fn(&V) -> usize) -> ByteLru<K, V> {
        ByteLru {
            entries: BTreeMap::new(),
            by_age: BTreeMap::new(),
            clock: 0,
            bytes: 0,
            budget,
            cost,
        }
    }

    pub(crate) fn len(&self) -> usize { self.entries.len() }
    pub(crate) fn bytes(&self) -> usize { self.bytes }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        let slot = self.entries.get_mut(key)?;
        let old_tick = slot.1;
        self.clock += 1;
        let tick = self.clock;
        slot.1 = tick;

        match self.by_age.remove(&old_tick) {
            Some(owned) => self.by_age.insert(tick, owned),
            // Unreachable while the two maps agree, and `is_consistent` does not prove they do – it
            // compares lengths only, which is what this arm keeps true.
            None => self.by_age.insert(tick, key.clone()),
        };
        Some(&slot.0)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        let cost = (self.cost)(&value);
        if cost > self.budget { return; }
        if let Some((old, old_tick)) = self.entries.remove(&key) {
            self.bytes -= (self.cost)(&old);
            self.by_age.remove(&old_tick);
        }
        self.clock += 1;
        let tick = self.clock;
        self.bytes += cost;
        self.by_age.insert(tick, key.clone());
        self.entries.insert(key, (value, tick));
        self.evict_to_fit();
    }

    fn evict_to_fit(&mut self) {
        while self.bytes > self.budget {
            let Some((&tick, _)) = self.by_age.iter().next() else { break };
            let Some(victim) = self.by_age.remove(&tick) else { break };
            if let Some((v, _)) = self.entries.remove(&victim) {
                self.bytes -= (self.cost)(&v);
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.by_age.clear();
        self.bytes = 0;
    }
}

pub(crate) type GlyphCache = ByteLru<GlyphKey, CachedGlyph>;

pub(crate) fn glyph_cache(budget: usize) -> GlyphCache {
    ByteLru::new(budget, CachedGlyph::cost)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub struct ShelfPacker {
    width: usize,
    height: usize,
    shelves: Vec<(usize, usize, usize)>,
    used_height: usize,
}

impl ShelfPacker {
    pub fn new(width: usize, height: usize) -> ShelfPacker {
        ShelfPacker { width, height, shelves: Vec::new(), used_height: 0 }
    }

    pub fn insert(&mut self, w: usize, h: usize) -> Option<Rect> {
        if w == 0 || h == 0 || w > self.width || h > self.height { return None; }

        let mut best: Option<usize> = None;
        for (i, &(_, sh, used)) in self.shelves.iter().enumerate() {
            if sh >= h && used.checked_add(w).is_some_and(|end| end <= self.width) {
                let better = best.is_none_or(|b| sh < self.shelves[b].1);
                if better { best = Some(i); }
            }
        }
        if let Some(i) = best {
            let (y, _, used) = self.shelves[i];
            self.shelves[i].2 = used + w;
            return Some(Rect { x: used, y, w, h });
        }

        if self.used_height.checked_add(h).is_none_or(|end| end > self.height) { return None; }
        let y = self.used_height;
        self.used_height += h;
        self.shelves.push((y, h, w));
        Some(Rect { x: 0, y, w, h })
    }

    pub fn reset(&mut self) {
        self.shelves.clear();
        self.used_height = 0;
    }
}
