use crate::daecore::daeshaper::unicode::Script;
use crate::daecore::daeshaper::generated::unicode_tables as t;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Tag(pub(crate) u32);

impl Tag {
    pub(crate) const fn from_bytes(b: &[u8; 4]) -> Tag {
        Tag(((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | b[3] as u32)
    }

    pub(crate) fn to_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    pub(crate) const DEFAULT_SCRIPT: Tag = Tag::from_bytes(b"DFLT");
}

fn v2_tag(iso: &str) -> Option<Tag> {
    Some(match iso {
        "Beng" => Tag::from_bytes(b"bng2"),
        "Deva" => Tag::from_bytes(b"dev2"),
        "Gujr" => Tag::from_bytes(b"gjr2"),
        "Guru" => Tag::from_bytes(b"gur2"),
        "Knda" => Tag::from_bytes(b"knd2"),
        "Mlym" => Tag::from_bytes(b"mlm2"),
        "Orya" => Tag::from_bytes(b"ory2"),
        "Taml" => Tag::from_bytes(b"tml2"),
        "Telu" => Tag::from_bytes(b"tel2"),
        "Mymr" => Tag::from_bytes(b"mym2"),
        _ => return None,
    })
}

fn v1_tag(iso: &str) -> Tag {
    match iso {
        "Laoo" => Tag::from_bytes(b"lao "),
        "Yiii" => Tag::from_bytes(b"yi  "),
        "Nkoo" => Tag::from_bytes(b"nko "),
        "Vaii" => Tag::from_bytes(b"vai "),
        "Hira" => Tag::from_bytes(b"kana"),
        _ => {
            let b = iso.as_bytes();
            if b.len() != 4 {
                return Tag::DEFAULT_SCRIPT;
            }
            Tag::from_bytes(&[b[0].to_ascii_lowercase(), b[1], b[2], b[3]])
        }
    }
}

pub fn script_tags(script: Script) -> heapless3::Tags {
    let mut tags = heapless3::Tags::new();
    if script.is_context_dependent() {
        tags.push(Tag::DEFAULT_SCRIPT);
        return tags;
    }

    let iso = t::SCRIPT_ISO_CODES
        .get(script.0 as usize)
        .copied()
        .unwrap_or("Zzzz");

    if let Some(v2) = v2_tag(iso) {
        if v2 != Tag::from_bytes(b"mym2") {
            let mut b = v2.to_bytes();
            b[3] = b'3';
            tags.push(Tag::from_bytes(&b));
        }
        tags.push(v2);
    }
    tags.push(v1_tag(iso));
    tags
}

pub(crate) use crate::daecore::daeshaper::generated::language_tags::{LANGUAGE_TAGS, SUBTAG_WIDTH};

static TAG_FALLBACKS: &[(&[u8; 4], &[u8; 4])] = &[(b"DIV ", b"DHV "), (b"ZHTM", b"ZHH ")];

fn normalize_chinese(lower: &str) -> Option<&'static [u8; 4]> {
    let rest = lower.strip_prefix("zh")?;
    if !rest.is_empty() && !rest.starts_with('-') {
        return None;
    }
    if rest.contains("-hans") {
        return Some(b"ZHS ");
    }
    if rest.contains("-mo") {
        return Some(b"ZHTM");
    }
    if rest.contains("-hk") {
        return Some(b"ZHH ");
    }
    if rest.contains("-hant") || rest.contains("-tw") {
        return Some(b"ZHT ");
    }
    Some(b"ZHS ")
}

pub(crate) fn language_tag(language: &str) -> Option<Tag> {
    let lower: alloc::string::String = language.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }

    if let Some(t) = normalize_chinese(&lower) {
        return Some(Tag::from_bytes(t));
    }

    let lookup = |key: &str| {
        let bytes = key.as_bytes();
        if bytes.len() > SUBTAG_WIDTH {
            return None;
        }
        let mut probe = [0u8; SUBTAG_WIDTH];
        probe[..bytes.len()].copy_from_slice(bytes);
        LANGUAGE_TAGS
            .binary_search_by(|&(k, _)| k.cmp(&probe))
            .ok()
            .map(|i| Tag::from_bytes(&LANGUAGE_TAGS[i].1))
    };

    if let Some(t) = lookup(&lower) {
        return Some(t);
    }
    let primary = lower.split('-').next()?;
    lookup(primary)
}

pub(crate) fn language_tags(language: &str) -> heapless3::Tags {
    let mut tags = heapless3::Tags::new();
    let Some(primary) = language_tag(language) else { return tags };
    tags.push(primary);
    for &(specific, fallback) in TAG_FALLBACKS {
        if Tag::from_bytes(specific) == primary {
            tags.push(Tag::from_bytes(fallback));
        }
    }
    tags
}

pub(crate) mod heapless3 {
    use super::Tag;

    #[derive(Debug)]
    pub struct Tags {
        items: [Tag; 3],
        len: usize,
    }

    impl Tags {
        pub fn new() -> Tags {
            Tags { items: [Tag(0); 3], len: 0 }
        }

        pub(crate) fn push(&mut self, tag: Tag) {
            if self.len < self.items.len() {
                self.items[self.len] = tag;
                self.len += 1;
            }
        }

        pub fn as_slice(&self) -> &[Tag] {
            &self.items[..self.len]
        }

    }
}
