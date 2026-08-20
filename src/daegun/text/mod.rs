pub(crate) mod justify;
pub(crate) mod layout;
pub(crate) mod linebreak;
pub use crate::daecore::text::shape;
pub use shape::{Ignorables, ShapeOptions};
pub(crate) mod width;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidiParagraph {
    pub base_level: u8,
    pub levels: alloc::vec::Vec<u8>,
    pub visual_order: alloc::vec::Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualRun {
    pub chars: alloc::vec::Vec<usize>,
    pub level: u8,
}

pub fn line_visual_runs(
    para: &BidiParagraph,
    text: &str,
    start: usize,
    end: usize,
) -> alloc::vec::Vec<VisualRun> {
    let chars: alloc::vec::Vec<char> = text.chars().collect();
    let runs = crate::daeshaper::unicode::bidi::line_visual_runs(
        para.base_level,
        &para.levels,
        &chars,
        start,
        end,
    );
    runs.iter()
        .map(|(indices, level)| VisualRun { chars: indices.to_vec(), level })
        .collect()
}

pub fn script_runs(text: &str) -> alloc::vec::Vec<ScriptRun> {
    let chars: alloc::vec::Vec<char> = text.chars().collect();
    crate::daeshaper::unicode::itemize::script_runs(&chars)
}

pub use crate::daeshaper::unicode::itemize::ScriptRun;
pub use crate::daeshaper::unicode::Script;
pub use crate::daeshaper::unicode::{general_category, GeneralCategory};
pub use crate::daeshaper::unicode::is_upright;
pub use crate::daeshaper::unicode::vertical_form;

pub fn resolve_bidi(text: &str, base: Option<bool>) -> BidiParagraph {
    let p = crate::daeshaper::unicode::bidi::resolve(text, base);
    BidiParagraph { base_level: p.base_level, levels: p.levels, visual_order: p.visual_order }
}

pub fn grapheme_boundaries(text: &str) -> alloc::vec::Vec<usize> {
    crate::daeshaper::unicode::segment::grapheme_boundaries(text)
}

pub fn word_boundaries(text: &str) -> alloc::vec::Vec<usize> {
    crate::daeshaper::unicode::segment::word_boundaries(text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineBreak {
    pub at: usize,
    pub mandatory: bool,
}

pub fn line_break_opportunities(text: &str) -> alloc::vec::Vec<LineBreak> {
    crate::daeshaper::unicode::linebreak::line_break_opportunities(text)
        .into_iter()
        .map(|b| LineBreak { at: b.at, mandatory: b.mandatory })
        .collect()
}
