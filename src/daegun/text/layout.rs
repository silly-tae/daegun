use alloc::string::String;
use alloc::vec::Vec;

use crate::cache::{FontCache, RunContext};
use crate::text::linebreak::{break_lines, Breakpoint, Fit, Line};
use crate::text::shape::ShapedRun;
use crate::daeshaper::unicode::bidi;
use crate::daeshaper::unicode::itemize::script_runs;
use crate::daeshaper::unicode::{general_category, is_upright, vertical_form, GeneralCategory};

pub use crate::text::linebreak::BreakStrategy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    End,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WritingMode {
    #[default]
    Horizontal,
    VerticalRl,
    VerticalLr,
}

impl WritingMode {
    pub fn is_vertical(self) -> bool {
        !matches!(self, WritingMode::Horizontal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextOrientation {
    #[default]
    Mixed,
    Upright,
    Sideways,
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions<'a> {
    pub max_inline_size: f64,
    pub align: Align,
    pub writing_mode: WritingMode,
    pub text_orientation: TextOrientation,
    pub base_direction: Option<bool>,
    pub language: Option<&'a str>,
    pub line_height: Option<f64>,
    pub strategy: BreakStrategy,
    pub max_lines: Option<usize>,
}

impl Default for LayoutOptions<'_> {
    fn default() -> Self {
        LayoutOptions {
            max_inline_size: f64::INFINITY,
            align: Align::Start,
            writing_mode: WritingMode::Horizontal,
            text_orientation: TextOrientation::Mixed,
            base_direction: None,
            language: None,
            line_height: None,
            strategy: BreakStrategy::Greedy,
            max_lines: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PositionedRun {
    pub run: ShapedRun,
    pub offset: (f64, f64),
    pub level: u8,
    pub chars: (usize, usize),
    pub upright: bool,
}

impl PositionedRun {
    fn width(&self) -> f64 {
        self.run.advances.iter().sum()
    }
}

#[derive(Debug, Clone)]
pub struct LayoutLine {
    pub runs: Vec<PositionedRun>,
    pub chars: (usize, usize),
    pub baseline: f64,
    pub inline_size: f64,
    pub ascent: f64,
    pub descent: f64,
    pub hard_break: bool,
}

#[derive(Debug, Clone)]
pub struct TextLayout {
    pub lines: Vec<LayoutLine>,
    pub inline_size: f64,
    pub block_size: f64,
    pub truncated: Option<usize>,
}

fn is_expandable_space(c: char) -> bool {
    general_category(c) == GeneralCategory::SpaceSeparator
}

fn is_break_char(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

#[derive(Clone, Copy, Debug)]
struct Piece {
    start: usize,
    end: usize,
    upright: bool,
    level: u8,
    script: crate::daeshaper::unicode::Script,
}

fn pieces(chars: &[char], levels: &[u8], start: usize, end: usize, opts: &LayoutOptions) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    if start >= end {
        return out;
    }
    let upright_of = |c: char| match (opts.writing_mode.is_vertical(), opts.text_orientation) {
        (false, _) | (true, TextOrientation::Upright) => true,
        (true, TextOrientation::Sideways) => false,
        (true, TextOrientation::Mixed) => is_upright(c, vertical_form(c).is_some()),
    };

    let mut prev: Option<(u16, u8, bool)> = None;
    for sr in script_runs(&chars[start..end]) {
        for i in sr.start..sr.end {
            let at = start + i;
            let key = (sr.script.0, levels.get(at - start).copied().unwrap_or(0), upright_of(chars[at]));
            match out.last_mut() {
                Some(p) if prev == Some(key) && p.end == at => p.end = at + 1,
                _ => out.push(
                    Piece { start: at, end: at + 1, upright: key.2, level: key.1, script: sr.script },
                ),
            }
            prev = Some(key);
        }
    }
    out
}

const CONTEXT: usize = 5;

fn context_before(chars: &[char], start: usize) -> String {
    chars[start.saturating_sub(CONTEXT)..start].iter().collect()
}

fn context_after(chars: &[char], end: usize) -> String {
    chars[end..(end + CONTEXT).min(chars.len())].iter().collect()
}

fn shapes_vertically(piece: &Piece, vertical: bool) -> bool {
    vertical && piece.upright
}

fn measure_piece(
    fc: &FontCache,
    axes: &[(String, f64)],
    chars: &[char],
    piece: &Piece,
    vertical: bool,
    language: Option<&str>,
    advances: &mut [f64],
) {
    let text: String = chars[piece.start..piece.end].iter().collect();
    let (before, after) = (context_before(chars, piece.start), context_after(chars, piece.end));
    let ctx = RunContext {
        // The bidi level decides the direction, not the text. A piece of only Common characters –
        // the ") " between an embedded Latin word and the Arabic around it – has no directional
        // signal to guess from, and guessing left the parenthesis unmirrored.
        rtl: Some(piece.level % 2 == 1),
        before: &before,
        after: &after,
        language,
        seed_script: Some(piece.script),
        ..Default::default()
    };
    let Some(run) = fc.shaped_run_in_context(axes, &text, shapes_vertically(piece, vertical), &ctx)
    else { return };
    for (i, &cluster) in run.clusters.iter().enumerate() {
        let at = piece.start + cluster as usize;
        if let (Some(slot), Some(&adv)) = (advances.get_mut(at), run.advances.get(i)) {
            *slot += adv;
        }
    }
}

fn breakpoints(
    chars: &[char],
    advances: &[f64],
    ops: &[(usize, bool)],
    start: usize,
    justify: bool,
) -> Vec<Breakpoint> {
    let mut bps = alloc::vec![Breakpoint { at: start, ..Breakpoint::start() }];
    let mut prev = start;
    for &(at, forced) in ops {
        let mut ink_end = at;
        while ink_end > prev && (is_expandable_space(chars[ink_end - 1]) || is_break_char(chars[ink_end - 1])) {
            ink_end -= 1;
        }
        let ink: f64 = advances[prev..ink_end].iter().sum();
        let space: f64 = advances[ink_end..at].iter().sum();
        let (stretch, shrink) = if justify { (space / 2.0, space / 3.0) } else { (0.0, 0.0) };
        bps.push(Breakpoint {
            at,
            ink,
            space,
            stretch,
            shrink,
            penalty: if forced { f64::NEG_INFINITY } else { 0.0 },
        });
        prev = at;
    }
    bps
}

struct Para<'a> {
    bidi: &'a bidi::Paragraph,
    chars: &'a [char],
    para_chars: &'a [char],
    start: usize,
    piece_of: &'a [usize],
    pieces: &'a [Piece],
}

fn build_line(
    fc: &FontCache,
    axes: &[(String, f64)],
    p: &Para,
    range: (usize, usize),
    vertical: bool,
    language: Option<&str>,
) -> Vec<PositionedRun> {
    let (chars, para_start) = (p.chars, p.start);
    let (all_pieces, piece_of, para, para_chars) = (p.pieces, p.piece_of, p.bidi, p.para_chars);
    let (from, to) = range;
    let mut ink_end = to;
    while ink_end > from && (is_expandable_space(chars[ink_end - 1]) || is_break_char(chars[ink_end - 1])) {
        ink_end -= 1;
    }

    let mut runs: Vec<PositionedRun> = Vec::new();
    let emit = |piece: usize, s: usize, e: usize, level: u8, runs: &mut Vec<PositionedRun>| {
        let text: String = chars[s..e].iter().collect();
        let upright = all_pieces.get(piece).is_none_or(|p| p.upright);
        let as_vertical = vertical && upright;
        let seed_script = all_pieces.get(piece).map(|p| p.script);
        let (before, after) = (context_before(chars, s), context_after(chars, e));
        let ctx = RunContext {
            rtl: Some(level % 2 == 1),
            before: &before,
            after: &after,
            language,
            seed_script,
            ..Default::default()
        };
        let Some(run) = fc.shaped_run_in_context(axes, &text, as_vertical, &ctx) else { return };
        runs.push(PositionedRun {
            run: (*run).clone(),
            offset: (0.0, 0.0),
            level,
            chars: (s, e),
            upright,
        });
    };

    let visual = bidi::line_visual_runs(
        para.base_level, &para.levels, para_chars, from - para_start, ink_end - para_start,
    );
    for (indices, level) in visual.iter() {
        let mut logical = indices.to_vec();
        logical.sort_unstable();

        let mut split: Vec<PositionedRun> = Vec::new();
        let mut seg: Option<(usize, usize, usize)> = None;
        for rel in logical {
            let i = rel + para_start;
            let p = piece_of.get(rel).copied().unwrap_or(usize::MAX);
            match seg {
                Some((cur, s, e)) if cur == p && e == i => seg = Some((cur, s, i + 1)),
                other => {
                    if let Some((op, os, oe)) = other {
                        emit(op, os, oe, level, &mut split);
                    }
                    seg = Some((p, i, i + 1));
                }
            }
        }
        if let Some((p, s, e)) = seg {
            emit(p, s, e, level, &mut split);
        }

        // Pieces come out in logical order, and for a right-to-left run that is backwards: a space
        // itemizing to Latin beside Arabic at the same level is one bidi run and two pieces, and
        // emitting them ascending puts the space on the wrong side of the word.
        if level % 2 == 1 {
            split.reverse();
        }
        runs.append(&mut split);
    }
    runs
}

fn place(runs: &mut [PositionedRun], vertical: bool, offset: f64) -> f64 {
    let mut inline = offset;
    for r in runs.iter_mut() {
        r.offset = if vertical { (0.0, inline) } else { (inline, 0.0) };
        inline += r.width();
    }
    inline - offset
}

const MAX_SHRINK: f64 = 1.0 / 3.0;

fn distribute(runs: &mut [PositionedRun], chars: &[char], slack: f64) -> bool {
    let space_width = |r: &PositionedRun, i: usize| -> f64 {
        let at = r.chars.0 + r.run.clusters.get(i).copied().unwrap_or(0) as usize;
        if chars.get(at).copied().is_some_and(is_expandable_space) {
            r.run.advances.get(i).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    };
    let total: f64 = runs
        .iter()
        .map(|r| (0..r.run.advances.len()).map(|i| space_width(r, i)).sum::<f64>())
        .sum();
    if total <= 0.0 {
        return false;
    }
    let scale = (slack / total).max(-MAX_SHRINK);
    for r in runs.iter_mut() {
        for i in 0..r.run.advances.len() {
            let w = space_width(r, i);
            if w > 0.0 {
                r.run.advances[i] += w * scale;
            }
        }
    }
    true
}

pub(crate) fn layout_text(
    fc: &FontCache,
    axes: &[(String, f64)],
    text: &str,
    opts: &LayoutOptions,
) -> Option<TextLayout> {
    let chars: Vec<char> = text.chars().collect();
    let vertical = opts.writing_mode.is_vertical();
    let metrics = fc.line_metrics(vertical);
    let line_height = opts.line_height.unwrap_or_else(|| metrics.line_height());
    let justify = opts.align == Align::Justify;

    let mut out =
        TextLayout { lines: Vec::new(), inline_size: 0.0, block_size: 0.0, truncated: None };
    if chars.is_empty() {
        return Some(out);
    }

    let opportunities = crate::daeshaper::unicode::linebreak::line_break_opportunities(text);
    let mut advances = alloc::vec![0.0f64; chars.len()];
    let mut baseline = metrics.ascent;
    let mut para_start = 0usize;
    let mut op_cursor = 0usize;
    debug_assert!(
        opportunities.windows(2).all(|w| w[0].at <= w[1].at),
        "the cursor below assumes break opportunities arrive in order",
    );

    for para_end in opportunities.iter().filter(|b| b.mandatory).map(|b| b.at) {
        if para_start >= para_end {
            continue;
        }
        let mut content_end = para_end;
        while content_end > para_start && is_break_char(chars[content_end - 1]) {
            content_end -= 1;
        }

        let para_chars: Vec<char> = chars[para_start..para_end].to_vec();
        let para_text: String = para_chars.iter().collect();
        let para = bidi::resolve(&para_text, opts.base_direction);

        let all_pieces = pieces(&chars, &para.levels, para_start, content_end, opts);
        let mut piece_of = alloc::vec![usize::MAX; para_end - para_start];
        for (pi, p) in all_pieces.iter().enumerate() {
            for slot in piece_of.iter_mut().take(p.end - para_start).skip(p.start - para_start) {
                *slot = pi;
            }
            measure_piece(fc, axes, &chars, p, vertical, opts.language, &mut advances);
        }

        while op_cursor < opportunities.len() && opportunities[op_cursor].at <= para_start {
            op_cursor += 1;
        }
        let ops_from = op_cursor;
        while op_cursor < opportunities.len() && opportunities[op_cursor].at <= para_end {
            op_cursor += 1;
        }
        let ops: Vec<(usize, bool)> = opportunities[ops_from..op_cursor]
            .iter()
            .map(|b| (b.at, b.mandatory))
            .collect();
        let bps = breakpoints(&chars, &advances, &ops, para_start, justify);

        let fit = Fit {
            target: opts.max_inline_size,
            line_end_stretch: if justify { 0.0 } else { opts.max_inline_size * 0.25 },
            last_line_stretch: f64::INFINITY,
        };

        for line in break_lines(&bps, &fit, opts.strategy) {
            if opts.max_lines.is_some_and(|n| out.lines.len() >= n) {
                out.truncated = Some(bps[line.from].at);
                return Some(finish(out));
            }
            let range = (bps[line.from].at, bps[line.to].at);
            let ctx = Para {
                bidi: &para,
                chars: &chars,
                para_chars: &para_chars,
                start: para_start,
                piece_of: &piece_of,
                pieces: &all_pieces,
            };
            let mut runs = build_line(fc, axes, &ctx, range, vertical, opts.language);

            let natural = place(&mut runs, vertical, 0.0);
            let inline_size =
                align_line(&mut runs, &chars, &bps, &line, natural, para.base_level, opts, vertical);

            out.inline_size = out.inline_size.max(inline_size);
            out.lines.push(LayoutLine {
                runs,
                chars: range,
                baseline,
                inline_size,
                ascent: metrics.ascent,
                descent: metrics.descent,
                hard_break: bps[line.to].is_forced(),
            });
            baseline += line_height;
        }

        para_start = para_end;
    }

    Some(finish(out))
}

#[allow(clippy::too_many_arguments, reason = "everything one line's placement depends on")]
fn align_line(
    runs: &mut [PositionedRun],
    chars: &[char],
    bps: &[Breakpoint],
    line: &Line,
    natural: f64,
    base_level: u8,
    opts: &LayoutOptions,
    vertical: bool,
) -> f64 {
    let slack = opts.max_inline_size - natural;
    if !slack.is_finite() {
        return natural;
    }
    let last = bps[line.to].is_forced();

    if opts.align == Align::Justify && !last && slack != 0.0 && distribute(runs, chars, slack) {
        return place(runs, vertical, 0.0);
    }

    let rtl = base_level % 2 == 1;
    let offset = match opts.align {
        Align::Center => slack / 2.0,
        Align::End => {
            if rtl { 0.0 } else { slack }
        }
        _ => {
            if rtl { slack } else { 0.0 }
        }
    };
    place(runs, vertical, offset);
    natural
}

fn finish(mut out: TextLayout) -> TextLayout {
    let (Some(first), Some(last)) = (out.lines.first(), out.lines.last()) else {
        return out;
    };
    out.block_size = last.baseline - first.baseline + first.ascent - last.descent;
    out
}
