use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use super::io::read_u16_be;
use crate::daecore::daetype::TableBytes;

pub fn read_font_family_name(table_map: &BTreeMap<String, TableBytes>) -> Option<String> {
    read_name_string(table_map, 16).or_else(|| read_name_string(table_map, 1))
}

pub fn read_name_string(table_map: &BTreeMap<String, TableBytes>, name_id: u16) -> Option<String> {
    let data = table_map.get("name")?;
    if data.len() < 6 { return None; }

    let count          = read_u16_be(data, 2)? as usize;
    let storage_offset = read_u16_be(data, 4)? as usize;

    if data.len() < 6 + count * 12 { return None; }

    let mut best_score:    i32   = -1;
    let mut best_offset:   usize = 0;
    let mut best_length:   usize = 0;
    let mut best_platform: u16   = 0;

    for i in 0..count {
        let rec = 6 + i * 12;
        let (Some(platform_id), Some(encoding_id), Some(language_id), Some(rec_name_id), Some(length), Some(offset)) = (
            read_u16_be(data, rec),
            read_u16_be(data, rec + 2),
            read_u16_be(data, rec + 4),
            read_u16_be(data, rec + 6),
            read_u16_be(data, rec + 8),
            read_u16_be(data, rec + 10),
        ) else { continue };
        if rec_name_id != name_id { continue; }
        let length = length as usize;
        let offset = offset as usize;

        let score: i32 = match (platform_id, encoding_id, language_id) {
            (3, 1, 0x0409) => 2,
            (1, _, _)      => 1,
            _ => continue,
        };

        if score > best_score {
            best_score    = score;
            best_offset   = offset;
            best_length   = length;
            best_platform = platform_id;
        }
    }

    if best_score < 0 { return None; }

    let abs = storage_offset + best_offset;
    if abs + best_length > data.len() { return None; }
    let raw = &data[abs..abs + best_length];

    let mut name = String::new();
    decode_name_into(raw, best_platform, &mut name);

    let trimmed = name.trim_matches('\0').trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

fn decode_name_into(raw: &[u8], platform: u16, out: &mut String) {
    out.clear();
    if platform == 3 {
        out.extend(char::decode_utf16(raw.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])))
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)));
    } else {
        out.extend(raw.iter().map(|&b| mac_roman_char(b)));
    }
}

pub fn parse_all_name_strings(table_map: &BTreeMap<String, TableBytes>) -> BTreeMap<u16, String> {
    let mut out = BTreeMap::new();
    let Some(data) = table_map.get("name") else { return out };
    if data.len() < 6 { return out; }
    let (Some(count), Some(storage_offset)) = (read_u16_be(data, 2), read_u16_be(data, 4)) else { return out };
    let count = count as usize;
    let storage_offset = storage_offset as usize;
    if data.len() < 6 + count * 12 { return out; }

    let mut best: BTreeMap<u16, (i32, u16, usize, usize)> = BTreeMap::new();
    for i in 0..count {
        let rec = 6 + i * 12;
        let (Some(platform_id), Some(encoding_id), Some(language_id), Some(rec_name_id), Some(length), Some(offset)) = (
            read_u16_be(data, rec),
            read_u16_be(data, rec + 2),
            read_u16_be(data, rec + 4),
            read_u16_be(data, rec + 6),
            read_u16_be(data, rec + 8),
            read_u16_be(data, rec + 10),
        ) else { continue };

        let score: i32 = match (platform_id, encoding_id, language_id) {
            (3, 1, 0x0409) => 2,
            (1, _, _)      => 1,
            _ => continue,
        };

        let entry = best.entry(rec_name_id).or_insert((-1, 0, 0, 0));
        if score > entry.0 {
            *entry = (score, platform_id, offset as usize, length as usize);
        }
    }

    let mut scratch = String::new();
    for (name_id, (_score, platform, offset, length)) in best {
        let abs = storage_offset + offset;
        let Some(raw) = data.get(abs..abs + length) else { continue };

        decode_name_into(raw, platform, &mut scratch);
        let trimmed = scratch.trim_matches('\0').trim();
        if !trimmed.is_empty() {
            out.insert(name_id, trimmed.to_string());
        }
    }

    out
}

pub(crate) const MAC_ROMAN_HIGH: [char; 128] = [
    'Ä','Å','Ç','É','Ñ','Ö','Ü','á','à','â','ä','ã','å','ç','é','è',
    'ê','ë','í','ì','î','ï','ñ','ó','ò','ô','ö','õ','ú','ù','û','ü',
    '†','°','¢','£','§','•','¶','ß','®','©','™','´','¨','≠','Æ','Ø',
    '∞','±','≤','≥','¥','µ','∂','∑','∏','π','∫','ª','º','Ω','æ','ø',
    '¿','¡','¬','√','ƒ','≈','∆','«','»','…','\u{A0}','À','Ã','Õ','Œ','œ',
    '–','—','“','”','‘','’','÷','◊','ÿ','Ÿ','⁄','€','‹','›','ﬁ','ﬂ',
    '‡','·','‚','„','‰','Â','Ê','Á','Ë','È','Í','Î','Ï','Ì','Ó','Ô',
    '\u{F8FF}','Ò','Ú','Û','Ù','ı','ˆ','˜','¯','˘','˙','˚','¸','˝','˛','ˇ',
];

pub(crate) fn mac_roman_char(b: u8) -> char {
    if b < 0x80 {
        return b as char;
    }
    MAC_ROMAN_HIGH[usize::from(b) - 0x80]
}

// MacOS Turkish is MacRoman with seven positions reassigned, per Apple's TURKISH.TXT. A cmap
// subtable on platform 1 encoding 0 names it with language 18.
const MAC_TURKISH_OVERRIDES: [(u8, char); 7] = [
    (0xDA, 'Ğ'), (0xDB, 'ğ'), (0xDC, 'İ'), (0xDD, 'ı'),
    (0xDE, 'Ş'), (0xDF, 'ş'), (0xF5, '\u{F8A0}'),
];

pub(crate) fn mac_turkish_char(b: u8) -> char {
    match MAC_TURKISH_OVERRIDES.iter().find(|&&(k, _)| k == b) {
        Some(&(_, c)) => c,
        None => mac_roman_char(b),
    }
}

pub(crate) fn mac_turkish_byte(codepoint: u32) -> Option<u8> {
    let wanted = char::from_u32(codepoint)?;
    if let Some(&(b, _)) = MAC_TURKISH_OVERRIDES.iter().find(|&&(_, c)| c == wanted) {
        return Some(b);
    }
    let byte = mac_roman_byte(codepoint)?;
    // A byte Turkish reassigned does not stand for whatever MacRoman put there.
    (!MAC_TURKISH_OVERRIDES.iter().any(|&(k, _)| k == byte)).then_some(byte)
}

pub(crate) fn mac_roman_byte(codepoint: u32) -> Option<u8> {
    if codepoint < 0x80 {
        return Some(codepoint as u8);
    }
    let wanted = char::from_u32(codepoint)?;
    let index = MAC_ROMAN_HIGH.iter().position(|c| *c == wanted)?;
    Some(0x80 + index as u8)
}

pub fn read_font_style(table_map: &BTreeMap<String, TableBytes>) -> &'static str {
    let italic_os2 = super::os2::parse_os2(table_map)
        .and_then(|o| o.fs_selection)
        .is_some_and(|v| v & 0x0001 != 0);

    let italic_head = table_map.get("head")
        .filter(|d| d.len() >= 46)
        .and_then(|d| read_u16_be(d, 44))
        .is_some_and(|v| v & 0x0002 != 0);

    if italic_os2 || italic_head { "italic" } else { "normal" }
}
