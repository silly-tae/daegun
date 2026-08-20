use alloc::vec::Vec;

const PAGE_BITS: u32 = 8;
const PAGE: usize = 1 << PAGE_BITS;

const NO_PAGE: u32 = u32::MAX;

enum Kind {
    Direct { base: u32, values: Vec<u16> },
    Paged { base_page: u32, page_of: Vec<u32>, values: Vec<u16> },
}

pub struct SparseIndex {
    absent: u16,
    kind: Kind,
}

impl SparseIndex {
    #[inline]
    pub fn lookup(&self, key: u32) -> u16 {
        match &self.kind {
            Kind::Direct { base, values } => key
                .checked_sub(*base)
                .and_then(|i| values.get(i as usize))
                .copied()
                .unwrap_or(self.absent),
            Kind::Paged { base_page, page_of, values } => (key >> PAGE_BITS)
                .checked_sub(*base_page)
                .and_then(|p| page_of.get(p as usize).copied())
                .filter(|&base| base != NO_PAGE)
                .and_then(|base| values.get(base as usize + (key as usize & (PAGE - 1))))
                .copied()
                .unwrap_or(self.absent),
        }
    }

    pub fn bytes(&self) -> usize {
        match &self.kind {
            Kind::Direct { values, .. } => values.len() * 2,
            Kind::Paged { page_of, values, .. } => page_of.len() * 4 + values.len() * 2,
        }
    }

    pub fn build(entries: &[(u32, u16)], absent: u16, budget: usize) -> Option<SparseIndex> {
        let first = entries.first()?.0;
        let last = entries.last()?.0;
        debug_assert!(entries.windows(2).all(|w| w[0].0 < w[1].0), "entries must be sorted, unique");

        let span = (last - first) as usize + 1;
        let direct_bytes = span.saturating_mul(2);

        let first_page = first >> PAGE_BITS;
        let last_page = last >> PAGE_BITS;
        let directory = (last_page - first_page) as usize + 1;
        let populated = {
            let mut n = 0usize;
            let mut seen = u32::MAX;
            for &(key, _) in entries {
                let page = key >> PAGE_BITS;
                if page != seen {
                    seen = page;
                    n += 1;
                }
            }
            n
        };
        let paged_bytes = directory
            .saturating_mul(4)
            .saturating_add(populated.saturating_mul(PAGE * 2));

        if direct_bytes.min(paged_bytes) > budget {
            return None;
        }

        let kind = if direct_bytes <= paged_bytes {
            let mut values = alloc::vec![absent; span];
            for &(key, value) in entries {
                values[(key - first) as usize] = value;
            }
            Kind::Direct { base: first, values }
        } else {
            let mut page_of = alloc::vec![NO_PAGE; directory];
            let mut values: Vec<u16> = Vec::new();
            for &(key, value) in entries {
                let page = ((key >> PAGE_BITS) - first_page) as usize;
                if page_of[page] == NO_PAGE {
                    page_of[page] = values.len() as u32;
                    values.resize(values.len() + PAGE, absent);
                }
                values[page_of[page] as usize + (key as usize & (PAGE - 1))] = value;
            }
            Kind::Paged { base_page: first_page, page_of, values }
        };

        Some(SparseIndex { absent, kind })
    }
}
