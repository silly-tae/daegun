use daegun::{Font, bytes::read_u16_be};

const FACES: &[&str] = &[
    "inter/InterVariable.ttf",
    "eb-garamond/EBGaramond.ttf",
    "noto-devanagari/NotoSansDevanagari-Regular.ttf",
    "noto-khmer/NotoSansKhmer-Regular.ttf",
    "colr-v1-test-glyphs/test_glyphs.ttf",
    "stix-two-math/STIX2Math.otf",
];

#[test]
fn cached_scalars_agree_with_the_tables() {
    let mut checked = 0;
    for rel in FACES {
        let path = format!("{}/{}", crate::FONTS, rel);
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::from_bytes(&bytes) else { continue };

        let head = font.table("head");
        let want_upm = head
            .filter(|h| h.len() >= 20)
            .and_then(|h| read_u16_be(h, 18))
            .filter(|&v| v > 0)
            .unwrap_or(2048);
        assert_eq!(font.upm(), want_upm, "{rel}: cached upm disagrees with head");

        let want_glyphs = font.table("maxp").and_then(|m| read_u16_be(m, 4));
        assert_eq!(
            font.num_glyphs(),
            want_glyphs.unwrap_or(0),
            "{rel}: cached num_glyphs disagrees with maxp"
        );

        assert_eq!(
            font.os2_info().is_some(),
            font.has_table("OS/2"),
            "{rel}: OS/2 presence disagrees with the table map"
        );

        assert_eq!(font.cap_height(), font.cap_height(), "{rel}: cap_height is not stable");
        assert_eq!(font.ascender(), font.ascender(), "{rel}: ascender is not stable");
        let a = font.line_metrics(false);
        let b = font.line_metrics(false);
        assert_eq!(a.ascent, b.ascent, "{rel}: line_metrics is not stable");
        assert_eq!(a.descent, b.descent, "{rel}: line_metrics is not stable");

        checked += 1;
    }
    assert!(checked >= 4, "only {checked} fixtures were readable, so this proved little");
}

#[test]
fn an_instance_caches_its_own_tables() {
    let path = format!("{}/inter/InterVariable.ttf", crate::FONTS);
    let Ok(bytes) = std::fs::read(&path) else { return };
    let font = Font::from_bytes(&bytes).expect("the fixture parses");

    let instanced = font.instance(&[("wght", 700.0)]);
    let sub = Font::from_bytes(&instanced).expect("the instance parses");

    let want = sub
        .table("head")
        .filter(|h| h.len() >= 20)
        .and_then(|h| read_u16_be(h, 18))
        .filter(|&v| v > 0)
        .unwrap_or(2048);
    assert_eq!(sub.upm(), want, "the instance's cached upm disagrees with its own head");
    assert_eq!(sub.upm(), font.upm(), "instancing changed the em, which it must not");
}
