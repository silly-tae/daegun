use alloc::string::String;
use alloc::vec::Vec;
use super::pen::OutlinePen;
use super::super::format::cff::{decode_charstring_number, resolve_fd_select, subr_bias};
use super::super::subsetter::{
    cff_index_spans, parse_cff_index_refs, parse_top_dict, parse_private_subrs_offset, parse_fd_dict_private,
    seac_offsets, standard_encoding_sid, sid_to_gid,
};

type Span = (u32, u32);

pub struct CffOutlines {
    charstrings:  Vec<Span>,
    global_subrs: Vec<Span>,
    local_subrs:  Vec<Vec<Span>>,
    fd_select:    Option<Vec<u16>>,
    charset_off:  Option<usize>,
}

impl CffOutlines {
    pub fn parse(cff: &[u8]) -> Result<CffOutlines, String> {
        if cff.len() < 4 {
            return Err("CFF: file too short".into());
        }
        let hdr_size = cff[2] as usize;

        let (_, after_name)        = cff_index_spans(cff, hdr_size, false)?;
        let (top_dicts, after_top) = parse_cff_index_refs(cff, after_name, false)?;
        let top_dict = top_dicts.into_iter().next().ok_or("CFF: empty Top DICT INDEX")?;
        let fields = parse_top_dict(top_dict)?;

        let (_, after_strings) = cff_index_spans(cff, after_top, false)?;
        let (global_subrs, _)  = cff_index_spans(cff, after_strings, false)?;

        let (charstrings, _) = cff_index_spans(cff, fields.charstrings_off, false)?;
        let n_glyphs = charstrings.len();

        let (fd_select, local_subrs) = if let Some(fd_array_off) = fields.fd_array_off {
            let fd_select_off = fields.fd_select_off.ok_or("CFF CID: missing FDSelect offset")?;
            let fd_select = resolve_fd_select(cff, fd_select_off, n_glyphs)?;
            let (fd_dicts, _) = parse_cff_index_refs(cff, fd_array_off, false)?;
            let mut per_fd = Vec::with_capacity(fd_dicts.len());
            for fd_dict in &fd_dicts {
                let (priv_size, priv_off, _) = parse_fd_dict_private(fd_dict);
                per_fd.push(local_subrs_at(cff, priv_off, priv_size)?);
            }
            (Some(fd_select), per_fd)
        } else {
            (None, vec![local_subrs_at(cff, fields.private_off, fields.private_size)?])
        };

        Ok(CffOutlines {
            charstrings,
            global_subrs,
            local_subrs,
            fd_select,
            charset_off: fields.charset_off,
        })
    }
}

pub fn outline_cff_glyph_with(
    outlines: &CffOutlines,
    cff: &[u8],
    gid: u16,
    pen: &mut dyn OutlinePen,
) -> Result<(), String> {
    let charstring = span_bytes(cff, *outlines
        .charstrings
        .get(gid as usize)
        .ok_or("CFF: glyph index out of range")?)?;

    let local_subrs = match &outlines.fd_select {
        Some(fd_select) => {
            let fd_idx = *fd_select.get(gid as usize).unwrap_or(&0) as usize;
            outlines
                .local_subrs
                .get(fd_idx)
                .ok_or("CFF CID: FDSelect references an FD index out of range")?
        }
        None => outlines.local_subrs.first().ok_or("CFF: no Private DICT")?,
    };

    if let Some((adx, ady, bchar, achar)) = seac_offsets(charstring) {
        return draw_seac(
            cff, &outlines.charstrings, outlines.charset_off, &outlines.global_subrs, local_subrs,
            adx, ady, bchar, achar, pen,
        );
    }

    draw_charstring(cff, charstring, &outlines.global_subrs, local_subrs, pen)
}

pub fn outline_cff_glyph_hinted(
    outlines: &CffOutlines,
    cff: &[u8],
    gid: u16,
    pen: &mut dyn OutlinePen,
) -> Result<CffHints, String> {
    let charstring = span_bytes(cff, *outlines
        .charstrings
        .get(gid as usize)
        .ok_or("CFF: glyph index out of range")?)?;

    let local_subrs = match &outlines.fd_select {
        Some(fd_select) => {
            let fd_idx = *fd_select.get(gid as usize).unwrap_or(&0) as usize;
            outlines
                .local_subrs
                .get(fd_idx)
                .ok_or("CFF CID: FDSelect references an FD index out of range")?
        }
        None => outlines.local_subrs.first().ok_or("CFF: no Private DICT")?,
    };

    if seac_offsets(charstring).is_some() {
        outline_cff_glyph_with(outlines, cff, gid, pen)?;
        return Ok(CffHints::default());
    }

    let mut pen = CloseElidingPen { inner: pen, start: (0.0, 0.0), pending: None };
    let pen = &mut pen;
    let mut state = State {
        stack: Vec::with_capacity(TYPE2_OPERAND_LIMIT), x: 0.0, y: 0.0, n_stems: 0,
        hints: Some(CffHints::default()), points: 0,
        width_taken: false, open_path: false, depth: 0,
        budget: MAX_CHARSTRING_STEPS,
    };
    let global_bias = subr_bias(outlines.global_subrs.len());
    let local_bias = subr_bias(local_subrs.len());
    run(cff, charstring, &outlines.global_subrs, local_subrs, global_bias, local_bias, &mut state, pen)?;
    if state.open_path {
        pen.close();
    }
    Ok(state.hints.take().unwrap_or_default())
}

#[allow(clippy::too_many_arguments)]
fn draw_seac(
    cff:          &[u8],
    charstrings:  &[Span],
    charset_off:  Option<usize>,
    global_subrs: &[Span],
    local_subrs:  &[Span],
    adx: f64, ady: f64, bchar: u8, achar: u8,
    pen: &mut dyn OutlinePen,
) -> Result<(), String> {
    let n_glyphs = charstrings.len();
    let base_gid = sid_to_gid(cff, charset_off, n_glyphs, standard_encoding_sid(bchar))
        .ok_or("CFF: seac base character not found in charset")?;
    let accent_gid = sid_to_gid(cff, charset_off, n_glyphs, standard_encoding_sid(achar))
        .ok_or("CFF: seac accent character not found in charset")?;

    let base_cs = span_bytes(cff, *charstrings.get(base_gid as usize)
        .ok_or("CFF: seac base glyph index out of range")?)?;
    draw_charstring(cff, base_cs, global_subrs, local_subrs, pen)?;

    let accent_cs = span_bytes(cff, *charstrings.get(accent_gid as usize)
        .ok_or("CFF: seac accent glyph index out of range")?)?;
    let mut offset_pen = SeacOffsetPen { inner: pen, dx: adx as f32, dy: ady as f32 };
    draw_charstring(cff, accent_cs, global_subrs, local_subrs, &mut offset_pen)
}

// A charstring may walk back to where its contour began, and that segment is the one `close`
// already draws. Held one step back so it can be dropped when the contour ends there.
struct CloseElidingPen<'a> {
    inner:   &'a mut dyn OutlinePen,
    start:   (f32, f32),
    pending: Option<(f32, f32)>,
}

impl CloseElidingPen<'_> {
    fn flush(&mut self) {
        if let Some((x, y)) = self.pending.take() {
            self.inner.line_to(x, y);
        }
    }
}

impl OutlinePen for CloseElidingPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.start = (x, y);
        self.inner.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.pending = Some((x, y));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.flush();
        self.inner.quad_to(cx, cy, x, y);
    }
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.flush();
        self.inner.curve_to(c1x, c1y, c2x, c2y, x, y);
    }
    fn close(&mut self) {
        if self.pending == Some(self.start) {
            self.pending = None;
        }
        self.flush();
        self.inner.close();
    }
}

struct SeacOffsetPen<'a> { inner: &'a mut dyn OutlinePen, dx: f32, dy: f32 }

impl OutlinePen for SeacOffsetPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) { self.inner.move_to(x + self.dx, y + self.dy) }
    fn line_to(&mut self, x: f32, y: f32) { self.inner.line_to(x + self.dx, y + self.dy) }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.inner.quad_to(cx + self.dx, cy + self.dy, x + self.dx, y + self.dy)
    }
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.inner.curve_to(c1x + self.dx, c1y + self.dy, c2x + self.dx, c2y + self.dy, x + self.dx, y + self.dy)
    }
    fn close(&mut self) { self.inner.close() }
}

fn local_subrs_at(cff: &[u8], priv_off: usize, priv_size: usize) -> Result<Vec<Span>, String> {
    if priv_size == 0 {
        return Ok(vec![]);
    }
    let priv_end = priv_off.saturating_add(priv_size);
    let priv_data = cff.get(priv_off..priv_end).ok_or("CFF: Private DICT out of bounds")?;
    let subrs_rel = parse_private_subrs_offset(priv_data);
    if subrs_rel == 0 {
        return Ok(vec![]);
    }
    let abs = priv_off + subrs_rel;
    if abs >= cff.len() {
        return Ok(vec![]);
    }
    Ok(cff_index_spans(cff, abs, false)?.0)
}

const MAX_SUBR_DEPTH: usize = 10;

const TYPE2_OPERAND_LIMIT: usize = 48;

#[derive(Clone, Copy, Debug)]
pub struct CffStem {
    pub min: f32,
    pub max: f32,
    pub vertical: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CffHints {
    pub stems: Vec<CffStem>,
    pub masks: Vec<(usize, Vec<u8>)>,
}

struct State {
    stack:       Vec<f64>,
    x:           f64,
    y:           f64,
    n_stems:     usize,
    width_taken: bool,
    open_path:   bool,
    depth:       usize,
    budget:      u32,
    hints:       Option<CffHints>,
    points:      usize,
}

const MAX_CHARSTRING_STEPS: u32 = 1_000_000;

pub(crate) fn draw_charstring(
    cff:          &[u8],
    charstring:   &[u8],
    global_subrs: &[Span],
    local_subrs:  &[Span],
    pen:          &mut dyn OutlinePen,
) -> Result<(), String> {
    let mut pen = CloseElidingPen { inner: pen, start: (0.0, 0.0), pending: None };
    let pen = &mut pen;
    let mut state = State {
        stack: Vec::with_capacity(TYPE2_OPERAND_LIMIT), x: 0.0, y: 0.0, n_stems: 0, hints: None, points: 0,
        width_taken: false, open_path: false, depth: 0,
        budget: MAX_CHARSTRING_STEPS,
    };
    let global_bias = subr_bias(global_subrs.len());
    let local_bias  = subr_bias(local_subrs.len());
    run(cff, charstring, global_subrs, local_subrs, global_bias, local_bias, &mut state, pen)?;
    if state.open_path {
        pen.close();
    }
    Ok(())
}

fn take_width_fixed(state: &mut State, expected: &[usize]) {
    if state.width_taken { return; }
    state.width_taken = true;
    let n = state.stack.len();
    if expected.iter().any(|&c| n == c + 1) {
        state.stack.remove(0);
    }
}

fn record_stems(state: &mut State, vertical: bool) {
    let Some(_) = state.hints.as_ref() else { return };
    let mut edge = 0.0f64;
    let pairs: Vec<CffStem> = state
        .stack
        .chunks_exact(2)
        .map(|c| {
            let min = edge + c[0];
            let max = min + c[1];
            edge = max;
            CffStem { min: min as f32, max: max as f32, vertical }
        })
        .collect();
    if let Some(h) = state.hints.as_mut() {
        h.stems.extend(pairs);
    }
}

fn take_width_stem(state: &mut State) {
    if state.width_taken { return; }
    state.width_taken = true;
    if state.stack.len() % 2 == 1 {
        state.stack.remove(0);
    }
}

fn moveto(state: &mut State, pen: &mut dyn OutlinePen, dx: f64, dy: f64) {
    if state.open_path {
        pen.close();
    }
    state.x += dx;
    state.y += dy;
    pen.move_to(state.x as f32, state.y as f32);
    state.points += 1;
    state.open_path = true;
}

#[allow(clippy::too_many_arguments)]
fn curveto(state: &mut State, pen: &mut dyn OutlinePen, c1x: f64, c1y: f64, c2x: f64, c2y: f64, ex: f64, ey: f64) {
    pen.curve_to(c1x as f32, c1y as f32, c2x as f32, c2y as f32, ex as f32, ey as f32);
    state.points += 3;
    state.x = ex;
    state.y = ey;
}

fn lineto(state: &mut State, pen: &mut dyn OutlinePen, dx: f64, dy: f64) {
    state.x += dx;
    state.y += dy;
    pen.line_to(state.x as f32, state.y as f32);
    state.points += 1;
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn span_bytes(cff: &[u8], s: Span) -> Result<&[u8], String> {
    cff.get(s.0 as usize..s.1 as usize).ok_or_else(|| "CFF charstring: subr span out of bounds".into())
}

#[allow(clippy::too_many_arguments)]
fn run(
    cff:          &[u8],
    cs:           &[u8],
    global_subrs: &[Span],
    local_subrs:  &[Span],
    global_bias:  i32,
    local_bias:   i32,
    state:        &mut State,
    pen:          &mut dyn OutlinePen,
) -> Result<(), String> {
    if state.depth > MAX_SUBR_DEPTH {
        return Err("CFF charstring: subroutine nesting too deep".into());
    }
    let mut pos = 0usize;
    while pos < cs.len() {
        if state.budget == 0 {
            return Err("CFF charstring: work budget exhausted".into());
        }
        state.budget -= 1;
        let b0 = cs[pos];

        if b0 >= 32 || b0 == 28 {
            let (v, sz) = decode_charstring_number(cs, pos)?;
            state.stack.push(v);
            pos += sz;
            continue;
        }

        match b0 {
            1 | 3 | 18 | 23 => {
                take_width_stem(state);
                record_stems(state, b0 == 3 || b0 == 23);
                state.n_stems += state.stack.len() / 2;
                state.stack.clear();
                pos += 1;
            }
            4 => {
                take_width_fixed(state, &[1]);
                let dy = *state.stack.first().ok_or("CFF charstring: vmoveto missing operand")?;
                moveto(state, pen, 0.0, dy);
                state.stack.clear();
                pos += 1;
            }
            5 => {
                let n = state.stack.len();
                if !n.is_multiple_of(2) { return Err("CFF charstring: rlineto needs an even operand count".into()); }
                let mut i = 0;
                while i + 2 <= n {
                    lineto(state, pen, state.stack[i], state.stack[i + 1]);
                    i += 2;
                }
                state.stack.clear();
                pos += 1;
            }
            6 | 7 => {
                let mut horiz = b0 == 6;
                for i in 0..state.stack.len() {
                    let v = state.stack[i];
                    if horiz { lineto(state, pen, v, 0.0); } else { lineto(state, pen, 0.0, v); }
                    horiz = !horiz;
                }
                state.stack.clear();
                pos += 1;
            }
            8 => {
                let n = state.stack.len();
                if !n.is_multiple_of(6) || n == 0 { return Err("CFF charstring: rrcurveto needs a nonzero multiple-of-6 operand count".into()); }
                let mut i = 0;
                while i + 6 <= n {
                    draw_rrcurve(state, pen, six(&state.stack, i));
                    i += 6;
                }
                state.stack.clear();
                pos += 1;
            }
            10 => {
                let idx = state.stack.pop().ok_or("CFF charstring: callsubr with empty stack")?;
                let real_idx = idx as i32 + local_bias;
                let span = (real_idx >= 0).then(|| local_subrs.get(real_idx as usize)).flatten()
                    .ok_or("CFF charstring: local subr index out of range")?;
                let subr = span_bytes(cff, *span)?;
                state.depth += 1;
                run(cff, subr, global_subrs, local_subrs, global_bias, local_bias, state, pen)?;
                state.depth -= 1;
                pos += 1;
            }
            11 => { return Ok(()); }
            14 => {
                take_width_fixed(state, &[0, 4]);
                if !state.stack.is_empty() {
                    return Err("CFF charstring: endchar has leftover operands (seac-via-endchar isn't resolved at this level)".into());
                }
                pos += 1;
            }
            19 | 20 => {
                if !state.stack.is_empty() {
                    take_width_stem(state);
                    record_stems(state, true);
                    state.n_stems += state.stack.len() / 2;
                    state.stack.clear();
                }
                let mask_bytes = state.n_stems.div_ceil(8);
                if pos + 1 + mask_bytes > cs.len() {
                    return Err("CFF charstring: hintmask/cntrmask truncated".into());
                }
                if b0 == 19 && let Some(h) = state.hints.as_mut() {
                    h.masks.push((state.points, cs[pos + 1..pos + 1 + mask_bytes].to_vec()));
                }
                pos += 1 + mask_bytes;
            }
            21 => {
                take_width_fixed(state, &[2]);
                if state.stack.len() < 2 { return Err("CFF charstring: rmoveto missing operands".into()); }
                let (dx, dy) = (state.stack[0], state.stack[1]);
                moveto(state, pen, dx, dy);
                state.stack.clear();
                pos += 1;
            }
            22 => {
                take_width_fixed(state, &[1]);
                let dx = *state.stack.first().ok_or("CFF charstring: hmoveto missing operand")?;
                moveto(state, pen, dx, 0.0);
                state.stack.clear();
                pos += 1;
            }
            24 => {
                let n = state.stack.len();
                if n < 8 || !(n - 2).is_multiple_of(6) { return Err("CFF charstring: rcurveline needs 6k+2 operands".into()); }
                let mut i = 0;
                while n - i > 2 {
                    draw_rrcurve(state, pen, six(&state.stack, i));
                    i += 6;
                }
                let (a, b) = (state.stack[i], state.stack[i + 1]);
                lineto(state, pen, a, b);
                state.stack.clear();
                pos += 1;
            }
            25 => {
                let n = state.stack.len();
                if n < 8 || !(n - 6).is_multiple_of(2) { return Err("CFF charstring: rlinecurve needs 2k+6 operands".into()); }
                let mut i = 0;
                while n - i > 6 {
                    let (a, b) = (state.stack[i], state.stack[i + 1]);
                    lineto(state, pen, a, b);
                    i += 2;
                }
                draw_rrcurve(state, pen, six(&state.stack, i));
                state.stack.clear();
                pos += 1;
            }
            26 => {
                let n = state.stack.len();
                let mut i = 0;
                let mut cur_x = state.x;
                if n % 4 == 1 { cur_x = state.x + state.stack[0]; i = 1; }
                if !(n - i).is_multiple_of(4) || n == i {
                    return Err("CFF charstring: vvcurveto has a malformed operand count".into());
                }
                while i + 4 <= n {
                    let [dya, dxb, dyb, dyc] = four(&state.stack, i);
                    let c1x = cur_x; let c1y = state.y + dya;
                    let c2x = c1x + dxb; let c2y = c1y + dyb;
                    let ex = c2x; let ey = c2y + dyc;
                    curveto(state, pen, c1x, c1y, c2x, c2y, ex, ey);
                    cur_x = state.x;
                    i += 4;
                }
                state.stack.clear();
                pos += 1;
            }
            27 => {
                let n = state.stack.len();
                let mut i = 0;
                let mut cur_y = state.y;
                if n % 4 == 1 { cur_y = state.y + state.stack[0]; i = 1; }
                if !(n - i).is_multiple_of(4) || n == i {
                    return Err("CFF charstring: hhcurveto has a malformed operand count".into());
                }
                while i + 4 <= n {
                    let [dxa, dxb, dyb, dxc] = four(&state.stack, i);
                    let c1x = state.x + dxa; let c1y = cur_y;
                    let c2x = c1x + dxb; let c2y = c1y + dyb;
                    let ex = c2x + dxc; let ey = c2y;
                    curveto(state, pen, c1x, c1y, c2x, c2y, ex, ey);
                    cur_y = state.y;
                    i += 4;
                }
                state.stack.clear();
                pos += 1;
            }
            29 => {
                let idx = state.stack.pop().ok_or("CFF charstring: callgsubr with empty stack")?;
                let real_idx = idx as i32 + global_bias;
                let span = (real_idx >= 0).then(|| global_subrs.get(real_idx as usize)).flatten()
                    .ok_or("CFF charstring: global subr index out of range")?;
                let subr = span_bytes(cff, *span)?;
                state.depth += 1;
                run(cff, subr, global_subrs, local_subrs, global_bias, local_bias, state, pen)?;
                state.depth -= 1;
                pos += 1;
            }
            30 | 31 => {
                let n = state.stack.len();
                if n < 4 || !(n.is_multiple_of(4) || n % 4 == 1) {
                    return Err("CFF charstring: vh/hvcurveto has a malformed operand count".into());
                }
                let mut horiz = b0 == 31;
                let mut i = 0;
                while i + 4 <= n {
                    let is_last_group = i + 8 > n;
                    let last_extra = if is_last_group && n % 4 == 1 { Some(state.stack[n - 1]) } else { None };
                    let [a, b, c, d] = four(&state.stack, i);
                    if horiz {
                        let (dx1, dx2, dy2, dy3) = (a, b, c, d);
                        let c1x = state.x + dx1; let c1y = state.y;
                        let c2x = c1x + dx2; let c2y = c1y + dy2;
                        let ex = c2x + last_extra.unwrap_or(0.0); let ey = c2y + dy3;
                        curveto(state, pen, c1x, c1y, c2x, c2y, ex, ey);
                    } else {
                        let (dy1, dx2, dy2, dx3) = (a, b, c, d);
                        let c1x = state.x; let c1y = state.y + dy1;
                        let c2x = c1x + dx2; let c2y = c1y + dy2;
                        let ex = c2x + dx3; let ey = c2y + last_extra.unwrap_or(0.0);
                        curveto(state, pen, c1x, c1y, c2x, c2y, ex, ey);
                    }
                    i += 4;
                    horiz = !horiz;
                }
                state.stack.clear();
                pos += 1;
            }
            12 => {
                let b1 = *cs.get(pos + 1).ok_or("CFF charstring: truncated escape operator")?;
                match b1 {
                    34 => { draw_hflex(state, pen)?; }
                    35 => { draw_flex(state, pen)?; }
                    36 => { draw_hflex1(state, pen)?; }
                    37 => { draw_flex1(state, pen)?; }
                    _ => return Err(format!("CFF charstring: unsupported escape operator 12 {}", b1)),
                }
                state.stack.clear();
                pos += 2;
            }
            _ => return Err(format!("CFF charstring: unsupported operator {}", b0)),
        }
    }
    Ok(())
}

#[inline]
fn six(s: &[f64], i: usize) -> [f64; 6] {
    [s[i], s[i + 1], s[i + 2], s[i + 3], s[i + 4], s[i + 5]]
}

#[inline]
fn four(s: &[f64], i: usize) -> [f64; 4] {
    [s[i], s[i + 1], s[i + 2], s[i + 3]]
}

fn draw_rrcurve(state: &mut State, pen: &mut dyn OutlinePen, ops: [f64; 6]) {
    let (dxa, dya, dxb, dyb, dxc, dyc) = (ops[0], ops[1], ops[2], ops[3], ops[4], ops[5]);
    let c1x = state.x + dxa; let c1y = state.y + dya;
    let c2x = c1x + dxb; let c2y = c1y + dyb;
    let ex = c2x + dxc; let ey = c2y + dyc;
    curveto(state, pen, c1x, c1y, c2x, c2y, ex, ey);
}

fn draw_hflex(state: &mut State, pen: &mut dyn OutlinePen) -> Result<(), String> {
    let s = &state.stack;
    if s.len() != 7 { return Err("CFF charstring: hflex needs exactly 7 operands".into()); }
    let (dx1, dx2, dy2, dx3, dx4, dx5, dx6) = (s[0], s[1], s[2], s[3], s[4], s[5], s[6]);
    let y0 = state.y;
    let c1x = state.x + dx1; let c1y = state.y;
    let c2x = c1x + dx2; let c2y = c1y + dy2;
    let ex1 = c2x + dx3; let ey1 = c2y;
    curveto(state, pen, c1x, c1y, c2x, c2y, ex1, ey1);
    let c3x = state.x + dx4; let c3y = state.y;
    let c4x = c3x + dx5; let c4y = y0;
    let ex2 = c4x + dx6; let ey2 = y0;
    curveto(state, pen, c3x, c3y, c4x, c4y, ex2, ey2);
    Ok(())
}

fn draw_flex(state: &mut State, pen: &mut dyn OutlinePen) -> Result<(), String> {
    if state.stack.len() != 13 { return Err("CFF charstring: flex needs exactly 13 operands".into()); }
    let (first, second) = (six(&state.stack, 0), six(&state.stack, 6));
    draw_rrcurve(state, pen, first);
    draw_rrcurve(state, pen, second);
    Ok(())
}

fn draw_hflex1(state: &mut State, pen: &mut dyn OutlinePen) -> Result<(), String> {
    let s = &state.stack;
    if s.len() != 9 { return Err("CFF charstring: hflex1 needs exactly 9 operands".into()); }
    let (dx1, dy1, dx2, dy2, dx3, dx4, dx5, dy5, dx6) = (s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8]);
    let y0 = state.y;
    let c1x = state.x + dx1; let c1y = state.y + dy1;
    let c2x = c1x + dx2; let c2y = c1y + dy2;
    let ex1 = c2x + dx3; let ey1 = c2y;
    curveto(state, pen, c1x, c1y, c2x, c2y, ex1, ey1);
    let c3x = state.x + dx4; let c3y = state.y;
    let c4x = c3x + dx5; let c4y = c3y + dy5;
    let ex2 = c4x + dx6; let ey2 = y0;
    curveto(state, pen, c3x, c3y, c4x, c4y, ex2, ey2);
    Ok(())
}

fn draw_flex1(state: &mut State, pen: &mut dyn OutlinePen) -> Result<(), String> {
    let s = &state.stack;
    if s.len() != 11 { return Err("CFF charstring: flex1 needs exactly 11 operands".into()); }
    let (dx1, dy1, dx2, dy2, dx3, dy3, dx4, dy4, dx5, dy5, d6) =
        (s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8], s[9], s[10]);
    let (x0, y0) = (state.x, state.y);
    let c1x = state.x + dx1; let c1y = state.y + dy1;
    let c2x = c1x + dx2; let c2y = c1y + dy2;
    let ex1 = c2x + dx3; let ey1 = c2y + dy3;
    curveto(state, pen, c1x, c1y, c2x, c2y, ex1, ey1);
    let c3x = state.x + dx4; let c3y = state.y + dy4;
    let c4x = c3x + dx5; let c4y = c3y + dy5;
    let dx_total = dx1 + dx2 + dx3 + dx4 + dx5;
    let dy_total = dy1 + dy2 + dy3 + dy4 + dy5;
    let (ex2, ey2) = if dx_total.abs() > dy_total.abs() {
        (c4x + d6, y0)
    } else {
        (x0, c4y + d6)
    };
    curveto(state, pen, c3x, c3y, c4x, c4y, ex2, ey2);
    Ok(())
}
