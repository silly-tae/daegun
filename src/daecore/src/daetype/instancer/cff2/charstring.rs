use alloc::string::String;
use alloc::vec::Vec;
use crate::daecore::daetype::format::cff::{decode_charstring_number_fx, charstring_number_error, subr_bias};
use crate::daecore::daetype::format::ivs::{ItemVariationStore, region_scalars};
use crate::daecore::daetype::format::round::banker_round;

pub(crate) struct Scratch {
    stack:        [Fx; MAX_OPERANDS],
    scalar_cache: Vec<ScalarSet>,
}

impl Default for Scratch {
    fn default() -> Self {
        Self { stack: [0; MAX_OPERANDS], scalar_cache: Vec::new() }
    }
}

const MAX_OPERANDS: usize = 513;

type Fx = i64;
const FX_SHIFT: u32 = 16;
const FX_ONE: Fx = 1 << FX_SHIFT;
const FX_TO_F64: f64 = 1.0 / FX_ONE as f64;

#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_charstring_into(
    charstring:      &[u8],
    global_subrs:    &[&[u8]],
    local_subrs:     &[&[u8]],
    default_vsindex: u16,
    vstore:          Option<&ItemVariationStore>,
    location:        &[f64],
    budget:          &mut u32,
    scratch:         &mut Scratch,
    out:             &mut Vec<u8>,
) -> Result<(), String> {
    // Both apply, and the outer one is the caller's on purpose: a per-charstring ceiling does not
    // compose, since a fresh budget per glyph makes the font's cost the ceiling times the glyph
    // count, from an input that need not grow at all.
    let per_call = (*budget).min(MAX_CHARSTRING_STEPS);
    let mut state = State {
        stack:      &mut scratch.stack,
        sp:         0,
        vsindex:    default_vsindex,
        hint_count: 0,
        depth:      0,
        budget:     per_call,
    };
    let global_bias = subr_bias(global_subrs.len());
    let local_bias  = subr_bias(local_subrs.len());
    let result = run(charstring, global_subrs, local_subrs, global_bias, local_bias, vstore, location, &mut state, out, &mut scratch.scalar_cache);
    *budget -= per_call - state.budget;
    result
}

struct ScalarSet {
    vsindex:  usize,
    scalars:  Vec<f64>,
    all_zero: bool,
}

const MAX_SUBR_DEPTH: usize = 10;

const MAX_CHARSTRING_STEPS: u32 = 1_000_000;

struct State<'a> {
    stack:      &'a mut [Fx; MAX_OPERANDS],
    sp:         usize,
    vsindex:    u16,
    hint_count: usize,
    depth:      usize,
    budget:     u32,
}

#[allow(clippy::too_many_arguments)]
fn run(
    cs:           &[u8],
    global_subrs: &[&[u8]],
    local_subrs:  &[&[u8]],
    global_bias:  i32,
    local_bias:   i32,
    vstore:       Option<&ItemVariationStore>,
    location:     &[f64],
    state:        &mut State<'_>,
    out:          &mut Vec<u8>,
    scalar_cache: &mut Vec<ScalarSet>,
) -> Result<(), String> {
    if state.depth > MAX_SUBR_DEPTH {
        return Err("CFF2 charstring: subroutine nesting too deep".into());
    }
    state.budget = state.budget
        .checked_sub(u32::try_from(cs.len()).unwrap_or(u32::MAX))
        .ok_or("CFF2 charstring: work budget exhausted")?;
    let mut sp = state.sp;
    let mut pos = 0usize;
    while pos < cs.len() {
        let b0 = cs[pos];

        if b0 >= 32 {
            if sp >= MAX_OPERANDS {
                return Err("CFF2 charstring: operand stack overflow".into());
            }
            if b0 <= 246 {
                state.stack[sp] = ((b0 as Fx) - 139) << FX_SHIFT;
                sp += 1;
                pos += 1;
                continue;
            }
            let Some((v, sz)) = decode_charstring_number_fx(cs, pos) else {
                return Err(charstring_number_error(cs, pos));
            };
            state.stack[sp] = v;
            sp += 1;
            pos += sz;
            continue;
        }
        if b0 == 28 {
            if sp >= MAX_OPERANDS {
                return Err("CFF2 charstring: operand stack overflow".into());
            }
            let Some((v, sz)) = decode_charstring_number_fx(cs, pos) else {
                return Err(charstring_number_error(cs, pos));
            };
            state.stack[sp] = v;
            sp += 1;
            pos += sz;
            continue;
        }

        match b0 {
            1 | 3 | 18 | 23 => {
                state.hint_count += sp / 2;
                flush(out, &state.stack[..sp], &mut sp, &[b0]);
                pos += 1;
            }
            19 | 20 => {
                if sp != 0 {
                    state.hint_count += sp / 2;
                }
                flush(out, &state.stack[..sp], &mut sp, &[b0]);
                let mask_bytes = state.hint_count.div_ceil(8);
                let mask = cs.get(pos + 1..pos + 1 + mask_bytes)
                    .ok_or("CFF2 charstring: hintmask/cntrmask truncated")?;
                out.extend_from_slice(mask);
                pos += 1 + mask_bytes;
            }
            10 => {
                let idx = pop(state.stack, &mut sp).ok_or("CFF2 charstring: callsubr with empty stack")?;
                let real_idx = fx_to_i32(idx) + local_bias;
                let subr = (real_idx >= 0).then(|| local_subrs.get(real_idx as usize)).flatten()
                    .ok_or("CFF2 charstring: local subr index out of range")?;
                state.depth += 1;
                state.sp = sp;
                run(subr, global_subrs, local_subrs, global_bias, local_bias, vstore, location, state, out, scalar_cache)?;
                sp = state.sp;
                state.depth -= 1;
                pos += 1;
            }
            29 => {
                let idx = pop(state.stack, &mut sp).ok_or("CFF2 charstring: callgsubr with empty stack")?;
                let real_idx = fx_to_i32(idx) + global_bias;
                let subr = (real_idx >= 0).then(|| global_subrs.get(real_idx as usize)).flatten()
                    .ok_or("CFF2 charstring: global subr index out of range")?;
                state.depth += 1;
                state.sp = sp;
                run(subr, global_subrs, local_subrs, global_bias, local_bias, vstore, location, state, out, scalar_cache)?;
                sp = state.sp;
                state.depth -= 1;
                pos += 1;
            }
            15 => {
                let v = pop(state.stack, &mut sp).ok_or("CFF2 charstring: vsindex with empty stack")?;
                state.vsindex = (v.max(0) >> FX_SHIFT).min(u16::MAX as Fx) as u16;
                sp = 0;
                pos += 1;
            }
            16 => {
                let vstore = vstore.ok_or("CFF2 charstring: blend operator with no vstore")?;
                let n = pop(state.stack, &mut sp).ok_or("CFF2 charstring: blend missing operand count")?;
                if n < 0 { return Err("CFF2 charstring: blend negative operand count".into()); }
                let n = (n >> FX_SHIFT) as usize;
                let vsindex = state.vsindex as usize;
                let set: &ScalarSet = if let Some(i) = scalar_cache.iter().position(|s| s.vsindex == vsindex) {
                    &scalar_cache[i]
                } else {
                    let scalars = region_scalars(vstore, vsindex, location)
                        .ok_or("CFF2 charstring: vsindex out of range in vstore")?;
                    let all_zero = scalars.iter().all(|&s| s == 0.0);
                    scalar_cache.push(ScalarSet { vsindex, scalars, all_zero });
                    &scalar_cache[scalar_cache.len() - 1]
                };
                let scalars: &[f64] = &set.scalars;
                let k = scalars.len();
                let need = n.checked_add(n.checked_mul(k).ok_or("CFF2 charstring: blend operand count overflow")?)
                    .ok_or("CFF2 charstring: blend operand count overflow")?;
                if sp < need {
                    return Err("CFF2 charstring: blend stack underflow".into());
                }
                let deltas_start   = sp - n * k;
                let defaults_start = deltas_start - n;
                if set.all_zero {
                    sp = defaults_start + n;
                    pos += 1;
                    continue;
                }
                if k == 1 {
                    let s = scalars[0];
                    for i in 0..n {
                        let delta = s * (state.stack[deltas_start + i] as f64 * FX_TO_F64);
                        state.stack[defaults_start + i] = state.stack[defaults_start + i]
                            .saturating_add((banker_round(delta) as Fx) << FX_SHIFT);
                    }
                } else {
                    for i in 0..n {
                        let mut delta = 0.0;
                        for (r, &s) in scalars.iter().enumerate() {
                            delta += s * (state.stack[deltas_start + i * k + r] as f64 * FX_TO_F64);
                        }
                        state.stack[defaults_start + i] = state.stack[defaults_start + i]
                            .saturating_add((banker_round(delta) as Fx) << FX_SHIFT);
                    }
                }
                sp = defaults_start + n;
                pos += 1;
            }
            12 => {
                let b1 = *cs.get(pos + 1).ok_or("CFF2 charstring: truncated escape operator")?;
                if !matches!(b1, 34..=37) {
                    return Err(format!("CFF2 charstring: operator 12 {b1} is not in CFF2"));
                }
                flush(out, &state.stack[..sp], &mut sp, &[b0, b1]);
                pos += 2;
            }
            11 | 14 => { state.sp = sp; return Ok(()); }
            4 | 5 | 6 | 7 | 8 | 21 | 22 | 24 | 25 | 26 | 27 | 30 | 31 => {
                flush(out, &state.stack[..sp], &mut sp, &[b0]);
                pos += 1;
            }
            _ => return Err(format!("CFF2 charstring: operator {b0} is not in CFF2")),
        }
    }
    state.sp = sp;
    Ok(())
}

fn pop(stack: &[Fx; MAX_OPERANDS], sp: &mut usize) -> Option<Fx> {
    let v = *stack.get(sp.checked_sub(1)?)?;
    *sp -= 1;
    Some(v)
}

fn fx_to_i32(v: Fx) -> i32 {
    (v / FX_ONE).clamp(i32::MIN as Fx, i32::MAX as Fx) as i32
}

fn flush(out: &mut Vec<u8>, operands: &[Fx], sp: &mut usize, op_bytes: &[u8]) {
    out.reserve(operands.len() * 5 + op_bytes.len());
    for &v in operands {
        encode_charstring_number(out, v);
    }
    out.extend_from_slice(op_bytes);
    *sp = 0;
}

fn encode_charstring_number(out: &mut Vec<u8>, v: Fx) {
    if v & (FX_ONE - 1) == 0 {
        let iv = v >> FX_SHIFT;
        if (-32768..=32767).contains(&iv) {
            let iv = iv as i32;
            if (-107..=107).contains(&iv) {
                out.push((iv + 139) as u8);
            } else if (108..=1131).contains(&iv) {
                out.push((((iv - 108) / 256) + 247) as u8);
                out.push(((iv - 108) % 256) as u8);
            } else if (-1131..=-108).contains(&iv) {
                out.push((((-iv - 108) / 256) + 251) as u8);
                out.push(((-iv - 108) % 256) as u8);
            } else {
                out.push(28);
                out.extend_from_slice(&(iv as i16).to_be_bytes());
            }
            return;
        }
    }
    out.push(255);
    out.extend_from_slice(&(v.clamp(i32::MIN as Fx, i32::MAX as Fx) as i32).to_be_bytes());
}
