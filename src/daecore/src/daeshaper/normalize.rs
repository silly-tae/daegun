use super::buffer::{Buffer, GlyphInfo};
use super::face::Face;
use super::unicode;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Mode {
    None,
    Decomposed,
    ComposedDiacritics,
    ComposedDiacriticsNoShortCircuit,
    #[default]
    Auto,
}

pub(crate) const MAX_COMBINING_MARKS: usize = 32;

pub(crate) struct Context {
    pub(crate) has_gpos_mark: bool,
}

pub(crate) type DecomposeFn = fn(&Context, char) -> Option<(char, Option<char>)>;
pub(crate) type ComposeFn = fn(&Context, char, char) -> Option<char>;

fn unicode_decompose(_: &Context, ab: char) -> Option<(char, Option<char>)> {
    unicode::decompose(ab)
}

fn unicode_compose(_: &Context, a: char, b: char) -> Option<char> {
    unicode::compose(a, b)
}

pub(crate) struct Hooks {
    pub(crate) mode: Mode,
    pub(crate) decompose: DecomposeFn,
    pub(crate) compose: ComposeFn,
    pub(crate) reorder_marks: Option<fn(&mut Buffer, usize, usize)>,
}

impl Default for Hooks {
    fn default() -> Self {
        Hooks {
            mode: Mode::Auto,
            decompose: unicode_decompose,
            compose: unicode_compose,
            reorder_marks: None,
        }
    }
}

struct Ctx<'a, 'b> {
    buffer: &'a mut Buffer,
    face: &'a Face<'b>,
    hooks: &'a Hooks,
    has_gpos_mark: bool,
}

impl Ctx<'_, '_> {
    fn hook_context(&self) -> Context {
        Context { has_gpos_mark: self.has_gpos_mark }
    }

    fn output_char(&mut self, unichar: u32, glyph: u32) {
        self.buffer.cur_mut(0).glyph_index = glyph;
        self.buffer.output_glyph(unichar);
        let mut flags = self.buffer.scratch_flags;
        self.buffer.prev_mut().init_unicode_props(&mut flags);
        self.buffer.scratch_flags = flags;
    }

    fn next_char(&mut self, glyph: u32) {
        self.buffer.cur_mut(0).glyph_index = glyph;
        self.buffer.next_glyph();
    }

    fn glyph_of(&self, c: char) -> Option<u16> {
        self.face.glyph_index(c as u32)
    }
}

fn decompose(ctx: &mut Ctx, shortest: bool, ab: char) -> u32 {
    let Some((a, b)) = (ctx.hooks.decompose)(&ctx.hook_context(), ab) else { return 0 };

    let a_glyph = ctx.glyph_of(a);
    let b_glyph = match b {
        Some(b) => match ctx.glyph_of(b) {
            Some(g) => Some(g),
            None => return 0,
        },
        None => None,
    };

    if !shortest || a_glyph.is_none() {
        let ret = decompose(ctx, shortest, a);
        if ret != 0 {
            if let (Some(b), Some(b_glyph)) = (b, b_glyph) {
                ctx.output_char(b as u32, u32::from(b_glyph));
                return ret + 1;
            }
            return ret;
        }
    }

    if let Some(a_glyph) = a_glyph {
        ctx.output_char(a as u32, u32::from(a_glyph));
        if let (Some(b), Some(b_glyph)) = (b, b_glyph) {
            ctx.output_char(b as u32, u32::from(b_glyph));
            return 2;
        }
        return 1;
    }

    0
}

fn decompose_current_character(ctx: &mut Ctx, shortest: bool) {
    let Some(u) = char::from_u32(ctx.buffer.cur(0).id) else {
        ctx.next_char(0);
        return;
    };
    let glyph = ctx.glyph_of(u);

    if (!shortest || glyph.is_none()) && decompose(ctx, shortest, u) > 0 {
        ctx.buffer.skip_glyph();
        return;
    }

    if let Some(glyph) = glyph {
        ctx.next_char(u32::from(glyph));
        return;
    }

    if let Some(kind) = unicode::space_fallback(u)
        && let Some(space) = ctx.glyph_of(' ').or(ctx.buffer.invisible) {
            ctx.buffer.cur_mut(0).set_space_fallback(kind);
            ctx.next_char(u32::from(space));
            ctx.buffer.scratch_flags |= super::buffer::scratch_flags::HAS_SPACE_FALLBACK;
            return;
        }

    if u == '\u{2011}'
        && let Some(other) = ctx.glyph_of('\u{2010}') {
            ctx.next_char(u32::from(other));
            return;
        }

    ctx.next_char(0);
}

fn handle_variation_selector_cluster(ctx: &mut Ctx, end: usize) {
    while ctx.buffer.idx + 1 < end && ctx.buffer.successful {
        let next = char::from_u32(ctx.buffer.cur(1).id);
        if next.is_some_and(unicode::is_variation_selector) {
            let base = char::from_u32(ctx.buffer.cur(0).id);
            let selected = base
                .zip(next)
                .and_then(|(b, s)| ctx.face.glyph_variation_index(b as u32, s as u32));

            if let Some(glyph) = selected {
                ctx.buffer.cur_mut(0).glyph_index = u32::from(glyph);
                let unicode = ctx.buffer.cur(0).id;
                ctx.buffer.replace_glyphs(2, &[unicode]);
            } else {
                set_glyph(ctx);
                ctx.buffer.next_glyph();

                ctx.buffer.scratch_flags |=
                    super::buffer::scratch_flags::HAS_VARIATION_SELECTOR_FALLBACK;
                ctx.buffer.cur_mut(0).set_variation_selector(true);
                if ctx.buffer.not_found_variation_selector.is_some() {
                    ctx.buffer.cur_mut(0).clear_default_ignorable();
                }

                set_glyph(ctx);
                ctx.buffer.next_glyph();
            }

            while ctx.buffer.idx < end
                && char::from_u32(ctx.buffer.cur(0).id).is_some_and(unicode::is_variation_selector)
            {
                set_glyph(ctx);
                ctx.buffer.next_glyph();
            }
        } else {
            set_glyph(ctx);
            ctx.buffer.next_glyph();
        }
    }

    if ctx.buffer.idx < end {
        set_glyph(ctx);
        ctx.buffer.next_glyph();
    }
}

fn set_glyph(ctx: &mut Ctx) {
    let id = ctx.buffer.cur(0).id;
    if let Some(glyph) = ctx.face.glyph_index(id) {
        ctx.buffer.cur_mut(0).glyph_index = u32::from(glyph);
    }
}

fn decompose_multi_char_cluster(ctx: &mut Ctx, end: usize, short_circuit: bool) {
    for i in ctx.buffer.idx..end {
        if char::from_u32(ctx.buffer.info[i].id).is_some_and(unicode::is_variation_selector) {
            handle_variation_selector_cluster(ctx, end);
            return;
        }
    }

    while ctx.buffer.idx < end && ctx.buffer.successful {
        decompose_current_character(ctx, short_circuit);
    }
}

fn higher_combining_class(a: &GlyphInfo, b: &GlyphInfo) -> bool {
    a.modified_combining_class() > b.modified_combining_class()
}

pub(crate) fn normalize(buffer: &mut Buffer, face: &Face, hooks: &Hooks, has_gpos_mark: bool) {
    if buffer.is_empty() {
        return;
    }

    let mode = match hooks.mode {
        Mode::Auto if has_gpos_mark => Mode::ComposedDiacritics,
        Mode::Auto => Mode::ComposedDiacritics,
        m => m,
    };

    let always_short_circuit = mode == Mode::None;
    let might_short_circuit = always_short_circuit
        || (mode != Mode::Decomposed && mode != Mode::ComposedDiacriticsNoShortCircuit);

    let mut ctx = Ctx { buffer, face, hooks, has_gpos_mark };
    let mut all_simple = true;

    {
        ctx.buffer.clear_output();
        let count = ctx.buffer.len;
        ctx.buffer.idx = 0;

        loop {
            let mut end = ctx.buffer.idx + 1;
            while end < count && !is_unicode_mark(&ctx.buffer.info[end]) {
                end += 1;
            }
            if end < count {
                end -= 1;
            }

            if might_short_circuit {
                let len = end - ctx.buffer.idx;
                let mut done = 0;
                while done < len {
                    let id = ctx.buffer.cur(done).id;
                    match ctx.face.glyph_index(id) {
                        Some(g) => ctx.buffer.cur_mut(done).glyph_index = u32::from(g),
                        None => break,
                    }
                    done += 1;
                }
                ctx.buffer.next_glyphs(done);
            }

            while ctx.buffer.idx < end && ctx.buffer.successful {
                decompose_current_character(&mut ctx, might_short_circuit);
            }

            if ctx.buffer.idx == count || !ctx.buffer.successful {
                break;
            }

            all_simple = false;

            end = ctx.buffer.idx + 1;
            while end < count && is_unicode_mark(&ctx.buffer.info[end]) {
                end += 1;
            }

            decompose_multi_char_cluster(&mut ctx, end, always_short_circuit);

            if ctx.buffer.idx >= count || !ctx.buffer.successful {
                break;
            }
        }

        ctx.buffer.sync();
    }
    if !all_simple {
        let count = ctx.buffer.len;
        let mut i = 0;
        while i < count {
            if ctx.buffer.info[i].modified_combining_class() == 0 {
                i += 1;
                continue;
            }

            let mut end = i + 1;
            while end < count && ctx.buffer.info[end].modified_combining_class() != 0 {
                end += 1;
            }

            if end - i <= MAX_COMBINING_MARKS {
                ctx.buffer.sort(i, end, higher_combining_class);
                if let Some(reorder) = hooks.reorder_marks {
                    reorder(ctx.buffer, i, end);
                }
            }

            i = end + 1;
        }
    }

    if ctx.buffer.scratch_flags & super::buffer::scratch_flags::HAS_CGJ != 0 {
        for i in 1..ctx.buffer.len.saturating_sub(1) {
            if ctx.buffer.info[i].id == 0x034F {
                let last = ctx.buffer.info[i - 1].modified_combining_class();
                let next = ctx.buffer.info[i + 1].modified_combining_class();
                if next == 0 || last <= next {
                    // A CGJ is there to block reordering, so one the sort would not have moved
                    // anything across did no work – and leaving it hidden from GSUB breaks matching
                    // for nothing.
                    ctx.buffer.info[i].unhide();
                }
            }
        }
    }

    let recompose = mode == Mode::ComposedDiacritics || mode == Mode::ComposedDiacriticsNoShortCircuit;
    if !all_simple && ctx.buffer.successful && recompose {
        let count = ctx.buffer.len;
        let mut starter = 0;
        ctx.buffer.clear_output();
        ctx.buffer.next_glyph();

        while ctx.buffer.idx < count && ctx.buffer.successful {
            let cur = *ctx.buffer.cur(0);
            let blocked = starter != ctx.buffer.out_len - 1
                && ctx.buffer.prev().modified_combining_class() >= cur.modified_combining_class();

            if is_unicode_mark(&cur) && !blocked {
                let a = char::from_u32(ctx.buffer.out_info()[starter].id);
                let b = char::from_u32(cur.id);
                let hook = ctx.hook_context();
                let composed = a.zip(b).and_then(|(a, b)| (hooks.compose)(&hook, a, b));

                if let Some(composed) = composed
                    && let Some(glyph) = ctx.face.glyph_index(composed as u32) {
                        ctx.buffer.next_glyph();
                        if !ctx.buffer.successful {
                            return;
                        }

                        let out_len = ctx.buffer.out_len;
                        ctx.buffer.merge_out_clusters(starter, out_len);
                        ctx.buffer.out_len -= 1;

                        let mut flags = ctx.buffer.scratch_flags;
                        let info = &mut ctx.buffer.out_info_mut()[starter];
                        info.id = composed as u32;
                        info.glyph_index = u32::from(glyph);
                        info.init_unicode_props(&mut flags);
                        ctx.buffer.scratch_flags = flags;

                        continue;
                    }
            }

            ctx.buffer.next_glyph();
            if ctx.buffer.prev().modified_combining_class() == 0 {
                starter = ctx.buffer.out_len - 1;
            }
        }

        ctx.buffer.sync();
    }
}

fn is_unicode_mark(info: &GlyphInfo) -> bool {
    unicode::GeneralCategory::from_stored(info.general_category()).is_mark()
}
