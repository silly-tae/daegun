use daegun::{BreakStrategy, Font, LayoutOptions};

fn font() -> Font {
    let path = format!("{}/inter/InterVariable.ttf", crate::FONTS);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_bytes(&bytes).expect("Inter parses")
}

/// A line with nothing to fit into must not break at every word.
///
/// `LayoutOptions::max_inline_size` defaults to infinity, and a line that may stretch without cost
/// carries infinite stretch, so the ratio between them is infinity over infinity. That is NaN, and
/// NaN loses every comparison in the optimal search: no node stays active, and the strategy falls
/// back to a break at each opportunity without raising anything. `ratio_between` answers 0.0 for
/// that pair instead, which is what this holds in place.
#[test]
fn an_unbounded_line_does_not_break_at_every_word() {
    let f = font();
    let text = "The optimal strategy searches for the least bad set of breaks in a paragraph.";
    let words = text.split_whitespace().count();

    for strategy in [BreakStrategy::Optimal, BreakStrategy::Greedy] {
        let opts = LayoutOptions { strategy, ..LayoutOptions::default() };
        let layout = f.layout(text, &[], &opts).expect("lays out");
        assert_eq!(
            layout.lines.len(),
            1,
            "{strategy:?} broke {} words into {} lines with no width to fit, which is what an \
             unguarded NaN ratio does",
            words,
            layout.lines.len(),
        );
    }
}
