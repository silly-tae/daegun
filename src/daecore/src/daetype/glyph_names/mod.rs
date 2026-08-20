mod tables;

use alloc::string::String;
use alloc::vec::Vec;

use self::tables::{CFF_STANDARD_STRINGS, MACINTOSH_NAMES};

const N_STD_STRINGS: u16 = 391;

pub fn glyph_name(post: Option<&[u8]>, cff: Option<&[u8]>, gid: u16) -> Option<String> {
    post.and_then(|p| post_glyph_name(p, gid)).or_else(|| cff.and_then(|c| cff_glyph_name(c, gid)))
}

pub(crate) fn post_glyph_name(post: &[u8], gid: u16) -> Option<String> {
    if read_u32(post, 0)? != 0x0002_0000 {
        return None;
    }
    let count = read_u16(post, 32)?;
    if gid >= count {
        return None;
    }
    let index = read_u16(post, 34 + usize::from(gid) * 2)?;
    if let Some(name) = MACINTOSH_NAMES.get(usize::from(index)) {
        return Some(String::from(*name));
    }

    let mut at = 34 + usize::from(count) * 2;
    let mut remaining = usize::from(index) - MACINTOSH_NAMES.len();
    loop {
        let len = usize::from(*post.get(at)?);
        let text = post.get(at + 1..at + 1 + len)?;
        if remaining == 0 {
            return non_empty(text);
        }
        remaining -= 1;
        at += 1 + len;
    }
}

fn post_glyph_names(post: &[u8], n_glyphs: u16) -> Vec<Option<String>> {
    let none = || alloc::vec![None; usize::from(n_glyphs)];
    let Some(count) = read_u16(post, 32) else { return none() };
    let table_end = 34 + usize::from(count) * 2;

    // Only as many strings as some glyph actually names. Walking to the end of the table instead is
    // real amplification from untrusted input: a span costs 8 bytes to describe and a Pascal string
    // can be one byte, so an N-byte `post` could ask for 8N.
    let indices: Vec<u16> = (0..n_glyphs.min(count))
        .filter_map(|g| read_u16(post, 34 + usize::from(g) * 2))
        .collect();
    let wanted = indices
        .iter()
        .filter_map(|&i| usize::from(i).checked_sub(MACINTOSH_NAMES.len()))
        .max();
    let Some(wanted) = wanted else {
        return (0..n_glyphs).map(|g| post_glyph_name(post, g)).collect();
    };

    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut at = table_end;
    for _ in 0..=wanted {
        let Some(&len) = post.get(at) else { break };
        let len = usize::from(len);
        if post.get(at + 1..at + 1 + len).is_none() {
            break;
        }
        spans.push(((at + 1) as u32, (at + 1 + len) as u32));
        at += 1 + len;
    }

    (0..n_glyphs)
        .map(|g| {
            let index = usize::from(*indices.get(usize::from(g))?);
            if let Some(name) = MACINTOSH_NAMES.get(index) {
                return Some(String::from(*name));
            }
            let (s, e) = *spans.get(index - MACINTOSH_NAMES.len())?;
            non_empty(post.get(s as usize..e as usize)?)
        })
        .collect()
}

pub(crate) fn cff_glyph_name(cff: &[u8], gid: u16) -> Option<String> {
    let hdr = usize::from(*cff.get(2)?);
    let (_, after_name) = super::subsetter::parse_cff_index_refs(cff, hdr, false).ok()?;
    let (top_dicts, after_top) = super::subsetter::parse_cff_index_refs(cff, after_name, false).ok()?;
    let top = top_dicts.into_iter().next()?;
    let fields = super::subsetter::parse_top_dict(top).ok()?;
    if fields.ros.is_some() {
        return None;
    }
    let (charstrings, _) = super::subsetter::parse_cff_index_refs(cff, fields.charstrings_off, false).ok()?;
    let n_glyphs = charstrings.len();
    if usize::from(gid) >= n_glyphs {
        return None;
    }

    let sid = match fields.charset_off {
        Some(off) => charset_sid(cff, off, n_glyphs, gid)?,
        None => match fields.charset_predefined {
            1 => *super::subsetter::cff::expert_charsets::EXPERT_CHARSET.get(usize::from(gid))?,
            2 => *super::subsetter::cff::expert_charsets::EXPERT_SUBSET_CHARSET.get(usize::from(gid))?,
            _ => gid,
        },
    };

    if let Some(name) = CFF_STANDARD_STRINGS.get(usize::from(sid)) {
        return Some(String::from(*name));
    }
    let (strings, _) = super::subsetter::parse_cff_index_refs(cff, after_top, false).ok()?;
    non_empty(strings.get(usize::from(sid - N_STD_STRINGS))?)
}

fn non_empty(text: &[u8]) -> Option<String> {
    let name = core::str::from_utf8(text).ok()?;
    (!name.is_empty()).then(|| String::from(name))
}

fn charset_sid(cff: &[u8], off: usize, n_glyphs: usize, gid: u16) -> Option<u16> {
    if gid == 0 {
        return Some(0);
    }
    let mut found = None;
    super::subsetter::walk_charset(cff, off, n_glyphs, |g, sid| {
        if g == gid {
            found = Some(sid);
            super::subsetter::CharsetFlow::Stop
        } else {
            super::subsetter::CharsetFlow::Continue
        }
    })
    .ok()?;
    found
}

fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    super::decoder::read_u16_be(data, off)
}

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    super::decoder::read_u32_be(data, off)
}

pub fn glyph_names(post: Option<&[u8]>, cff: Option<&[u8]>, n_glyphs: u16) -> Vec<Option<String>> {
    if let Some(p) = post
        && read_u32(p, 0) == Some(0x0002_0000) {
            return post_glyph_names(p, n_glyphs);
        }
    cff.and_then(|c| cff_glyph_names(c, n_glyphs))
        .unwrap_or_else(|| alloc::vec![None; usize::from(n_glyphs)])
}

fn cff_glyph_names(cff: &[u8], n_glyphs: u16) -> Option<Vec<Option<String>>> {
    let hdr = usize::from(*cff.get(2)?);
    let (_, after_name) = super::subsetter::parse_cff_index_refs(cff, hdr, false).ok()?;
    let (top_dicts, after_top) = super::subsetter::parse_cff_index_refs(cff, after_name, false).ok()?;
    let fields = super::subsetter::parse_top_dict(top_dicts.into_iter().next()?).ok()?;
    if fields.ros.is_some() {
        return None;
    }
    let (charstrings, _) =
        super::subsetter::parse_cff_index_refs(cff, fields.charstrings_off, false).ok()?;
    let covered = charstrings.len();

    let mut sids = alloc::vec![0u16; usize::from(n_glyphs)];
    match fields.charset_off {
        Some(off) => {
            super::subsetter::walk_charset(cff, off, covered, |g, sid| {
                if let Some(slot) = sids.get_mut(usize::from(g)) {
                    *slot = sid;
                }
                super::subsetter::CharsetFlow::Continue
            })
            .ok()?;
        }
        None => {
            let predefined: Option<&[u16]> = match fields.charset_predefined {
                1 => Some(&super::subsetter::cff::expert_charsets::EXPERT_CHARSET),
                2 => Some(&super::subsetter::cff::expert_charsets::EXPERT_SUBSET_CHARSET),
                _ => None,
            };
            for (g, slot) in sids.iter_mut().enumerate() {
                *slot = match predefined {
                    Some(table) => *table.get(g)?,
                    None => u16::try_from(g).ok()?,
                };
            }
        }
    }

    let strings = super::subsetter::parse_cff_index_refs(cff, after_top, false).ok().map(|(v, _)| v);
    Some(
        sids.iter()
            .enumerate()
            .map(|(g, &sid)| {
                if g >= covered {
                    return None;
                }
                if let Some(name) = CFF_STANDARD_STRINGS.get(usize::from(sid)) {
                    return Some(String::from(*name));
                }
                non_empty(strings.as_ref()?.get(usize::from(sid - N_STD_STRINGS))?)
            })
            .collect(),
    )
}
