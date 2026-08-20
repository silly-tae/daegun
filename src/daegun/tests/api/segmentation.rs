use daegun::{
    Ignorables, ShapeOptions, ShapedRun, GlyphClass, ClusterLevel,
    Script, ScriptRun, VisualRun, GeneralCategory, BidiParagraph,
    line_visual_runs, script_runs, general_category, is_upright, vertical_form,
    resolve_bidi, grapheme_boundaries, word_boundaries, line_break_opportunities,
    Font,
};

#[test]
fn a_line_is_ordered_per_line_and_not_by_slicing_the_paragraph() {
    let text = "\u{633}\u{644}\u{627}\u{645} abc  def \u{633}\u{644}\u{627}\u{645}";
    let para = resolve_bidi(text, None);
    assert_eq!(para.base_level, 1, "the fixture text is meant to be a right-to-left paragraph");

    let (start, end) = (0, 9);
    let per_line: Vec<usize> = line_visual_runs(&para, text, start, end)
        .into_iter().flat_map(|r| r.chars).collect();
    let sliced: Vec<usize> = para.visual_order.iter().copied()
        .filter(|&c| c >= start && c < end).collect();

    assert_ne!(per_line, sliced, "slicing agreed, so the fixture no longer exercises L1");
    assert_eq!(
        per_line.first(), Some(&8),
        "L1 did not move the line's trailing space to the reading end: {per_line:?}",
    );
    assert_eq!(sliced.last(), Some(&0), "sanity: the sliced order still ends at the paragraph start");

    let mut seen = per_line.clone();
    seen.sort_unstable();
    assert_eq!(seen, (start..end).collect::<Vec<_>>(), "a line's chars are not partitioned exactly");

    let whole: Vec<usize> = line_visual_runs(&para, text, 0, text.chars().count())
        .into_iter().flat_map(|r| r.chars).collect();
    assert_eq!(whole, para.visual_order, "one line should be the paragraph order");

    assert!(line_visual_runs(&para, text, 5, 5).is_empty(), "an empty range returned runs");
    assert!(line_visual_runs(&para, text, 0, 9_999).is_empty(), "a range past the text returned runs");
}

#[test]
fn script_runs_split_where_the_script_changes() {
    let text = "سلام abc 日本 हिन्दी";
    let runs = script_runs(text);
    let names: Vec<&str> = runs.iter().map(|r| r.script.name()).collect();
    assert_eq!(names, ["Arabic", "Latin", "Han", "Devanagari"], "wrong split: {names:?}");

    assert_eq!(runs[0].start, 0, "the first run does not start at 0");
    assert_eq!(runs.last().unwrap().end, text.chars().count(), "the last run does not reach the end");
    assert!(runs.windows(2).all(|w| w[0].end == w[1].start), "runs do not meet");

    assert_eq!(script_runs("ab, cd").len(), 1, "a comma between Latin words split the run");
    assert!(script_runs("").is_empty(), "empty text yielded runs");

    assert!(script_runs(", ")[0].script.is_context_dependent(), "Common is not context-dependent");
    assert!(!script_runs("abc")[0].script.is_context_dependent(), "Latin is context-dependent");
}

#[test]
fn vertical_orientation_is_answerable() {
    assert!(is_upright('日', false), "CJK should stand upright in vertical text");
    assert!(!is_upright('A', false), "Latin should be rotated in vertical text");

    assert_eq!(vertical_form('\u{3001}'), Some('\u{FE11}'), "the ideographic comma has a vertical form");
    assert_eq!(vertical_form('A'), None, "a letter has no vertical form");
    assert_eq!(vertical_form(','), None, "an ASCII comma is rotated, not substituted");

    assert_eq!(general_category(' '), GeneralCategory::SpaceSeparator);
    assert_eq!(general_category('A'), GeneralCategory::UppercaseLetter);
    assert_ne!(general_category('\u{0301}'), GeneralCategory::UppercaseLetter, "a mark is not a letter");
}

#[test]
fn a_script_can_be_asked_which_way_it_runs_and_what_a_font_calls_it() {
    let script_of = |text: &str| script_runs(text)[0].script;

    assert_eq!(script_of("سلام").is_rtl(), Some(true), "Arabic runs right to left");
    assert_eq!(script_of("עברית").is_rtl(), Some(true), "Hebrew runs right to left");
    assert_eq!(script_of("abc").is_rtl(), Some(false), "Latin runs left to right");
    assert_eq!(script_of("日本").is_rtl(), Some(false), "Han runs left to right");
    assert_eq!(
        script_of("ᚠᚢᚦ").is_rtl(), None,
        "Runic has been written both ways and must not claim one",
    );

    assert_eq!(
        script_of("हिन्दी").opentype_tags(), ["dev3", "dev2", "deva"],
        "Devanagari's tags are wrong or out of order",
    );
    assert_eq!(script_of("မြန်မာ").opentype_tags(), ["mym2", "mymr"], "Myanmar gained a mym3");
    assert_eq!(script_of("abc").opentype_tags(), ["latn"]);
    assert_eq!(script_of(", ").opentype_tags(), ["DFLT"], "Common should yield DFLT");

    for text in ["سلام", "abc", "日本", "हिन्दी", "ខ្មែរ", "עברית"] {
        for t in script_of(text).opentype_tags() {
            assert_eq!(t.len(), 4, "{text:?} produced a tag that is not four bytes: {t:?}");
        }
    }
}

#[allow(unused_imports, reason = "each name resolving is what is being tested")]
#[test]
fn every_name_the_daeshaper_rounds_added_is_reachable() {
    let f = Font::from_bytes(&std::fs::read(
        format!("{}/eb-garamond/EBGaramond.ttf", crate::FONTS)).unwrap()).unwrap();
    let _: Option<GlyphClass> = f.glyph_class(1);
    let _: u16 = f.mark_attachment_class(1);
    let _: Vec<String> = f.script_tags();
    let _: Vec<String> = f.language_tags("latn");
    let _: Vec<String> = f.feature_tags(None, None);
    let p: BidiParagraph = resolve_bidi("abc", None);
    let _: Vec<VisualRun> = line_visual_runs(&p, "abc", 0, 3);
    let r: Vec<ScriptRun> = script_runs("abc");
    let s: Script = r[0].script;
    let _: (&str, Option<bool>, Vec<String>, bool) =
        (s.name(), s.is_rtl(), s.opentype_tags(), s.is_context_dependent());
    let _: GeneralCategory = general_category('a');
    let _: bool = is_upright('日', false);
    let _: Option<char> = vertical_form('\u{3001}');
    let _ = (grapheme_boundaries("a"), word_boundaries("a"), line_break_opportunities("a"));
    let run: ShapedRun = f.shape_with_options("hi", &[], false, &ShapeOptions {
        report_unsafe_to_concat: true, report_tatweel_positions: true,
        ignorables: Ignorables::Remove, suppress_dotted_circle: true,
        invisible_glyph: Some(1), cluster_level: ClusterLevel::Characters,
        ..Default::default()
    }).unwrap();
    let _: (&Vec<bool>, &Vec<bool>, bool, bool, &str) = (
        &run.unsafe_to_concat, &run.safe_to_insert_tatweel,
        run.complete, run.has_broken_syllable, run.shaper,
    );
    let _: (bool, bool) = (ClusterLevel::Characters.is_monotone(), ClusterLevel::Graphemes.is_graphemes());
}
