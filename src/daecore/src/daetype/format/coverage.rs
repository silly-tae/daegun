use super::super::decoder::{read_u16_be, search_records, window};

const LINEAR_MAX: usize = 64;

pub fn coverage_index(data: &[u8], glyph: u16) -> Option<u16> {
    match read_u16_be(data, 0)? {
        1 => {
            let count = read_u16_be(data, 2)? as usize;
            if count <= LINEAR_MAX {
                let avail = data.get(4..).map_or(&[][..], |r| &r[..(count * 2).min(r.len())]);
                let (recs, _) = avail.as_chunks::<2>();
                for (i, r) in recs.iter().enumerate() {
                    let g = u16::from_be_bytes(*r);
                    if g == glyph {
                        return u16::try_from(i).ok();
                    }
                    if g > glyph {
                        return None;
                    }
                }
                return None;
            }
            let hit = match data.get(4..).and_then(|r| r.get(..count * 2)) {
                Some(whole) => {
                    let (recs, _) = whole.as_chunks::<2>();
                    search_records(count, glyph as u32, |i| {
                        recs.get(i).map(|r| u32::from(u16::from_be_bytes(*r)))
                    })?
                }
                None => search_records(count, glyph as u32, |i| {
                    read_u16_be(data, 4 + i * 2).map(u32::from)
                })?,
            }
            .ok()?;
            u16::try_from(hit).ok()
        }
        2 => {
            let count = read_u16_be(data, 2)? as usize;
            if count <= LINEAR_MAX {
                let avail = data.get(4..).map_or(&[][..], |r| &r[..(count * 6).min(r.len())]);
                let (recs, _) = avail.as_chunks::<6>();
                for r in recs {
                    let start = u16::from_be_bytes([r[0], r[1]]);
                    if glyph < start {
                        return None;
                    }
                    if glyph <= u16::from_be_bytes([r[2], r[3]]) {
                        return u16::from_be_bytes([r[4], r[5]]).checked_add(glyph - start);
                    }
                }
                return None;
            }
            let records = data.get(4..).and_then(|r| r.get(..count * 6));
            let cand = match match records {
                Some(whole) => {
                    let (recs, _) = whole.as_chunks::<6>();
                    search_records(count, glyph as u32, |i| {
                        recs.get(i).map(|r| u32::from(u16::from_be_bytes([r[0], r[1]])))
                    })?
                }
                None => search_records(count, glyph as u32, |i| {
                    read_u16_be(data, 4 + i * 6).map(u32::from)
                })?,
            } {
                Ok(i) => i,
                Err(0) => return None,
                Err(i) => i - 1,
            };
            let rec = 4 + cand * 6;
            let h = window::<4>(data, rec)?;
            let start = u16::from_be_bytes([h[0], h[1]]);
            let end = u16::from_be_bytes([h[2], h[3]]);
            if glyph < start || glyph > end { return None; }
            read_u16_be(data, rec + 4)?.checked_add(glyph - start)
        }
        _ => None,
    }
}
