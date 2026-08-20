use alloc::vec::Vec;
use super::f26dot6::{self, ONE};
use super::state::{GraphicsState, HintMode, RoundState, Vector, Zone, FLAG_TOUCHED_X, FLAG_TOUCHED_Y};

const MAX_CALL_DEPTH: usize = 64;
const MAX_STEPS: usize = 1_000_000;
const MAX_STACK: usize = 4096;

#[derive(Clone, Copy)]
pub(crate) struct Function {
    pub start: usize,
    pub program: ProgramKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ProgramKind {
    Font,
    ControlValue,
    Glyph,
}

pub(crate) struct Interpreter {
    pub gs: GraphicsState,
    pub prep_gs: GraphicsState,
    pub storage: Vec<i32>,
    pub cvt: Vec<i32>,
    pub prep_storage: Vec<i32>,
    pub prep_cvt: Vec<i32>,
    pub storage_dirty: bool,
    pub cvt_dirty: bool,
    pub twilight_dirty: bool,
    // The glyph cache keys on `(gid, size)`, which is only sound if rendering is order-independent –
    // and these dirty flags plus the saved functions are what hold that. `WS`/`WCVTP`/`WCVTF` are
    // writable from a glyph program, and `FDEF` is legal inside one, so a function a glyph defined
    // outlived it and later glyphs called a body the font never meant for them: on Times New Roman
    // at 16 ppem, gid 34 defines one and 1,757 glyphs then disagree with themselves.
    pub prep_functions: Vec<Option<Function>>,
    pub functions_dirty: bool,
    pub stack: Vec<i32>,
    pub frames: Vec<Frame>,
    pub functions: Vec<Option<Function>>,
    pub iup_touched: Vec<usize>,
    pub mode: HintMode,
    pub ppem: u16,
    pub upm: u16,
    pub point_size: i32,
}

pub(crate) struct Programs<'a> {
    pub font: &'a [u8],
    pub control_value: &'a [u8],
    pub glyph: &'a [u8],
}

impl<'a> Programs<'a> {
    fn get(&self, kind: ProgramKind) -> &'a [u8] {
        match kind {
            ProgramKind::Font => self.font,
            ProgramKind::ControlValue => self.control_value,
            ProgramKind::Glyph => self.glyph,
        }
    }
}

pub(crate) struct Frame {
    program: ProgramKind,
    return_to: usize,
    remaining: i32,
    body_start: usize,
}

pub(crate) struct Machine<'a, 'z> {
    interp: &'a mut Interpreter,
    programs: Programs<'a>,
    zones: [&'z mut Zone; 2],
    steps: usize,
}

type Stop = ();

impl<'a, 'z> Machine<'a, 'z> {
    pub fn new(
        interp: &'a mut Interpreter,
        programs: Programs<'a>,
        twilight: &'z mut Zone,
        glyph: &'z mut Zone,
    ) -> Machine<'a, 'z> {
        Machine { interp, programs, zones: [twilight, glyph], steps: 0 }
    }

    pub(crate) fn run(&mut self, kind: ProgramKind) {
        let _ = self.execute(kind);
    }

    #[inline]
    fn zone_index(which: usize) -> usize { which.min(1) }

    fn push(&mut self, v: i32) -> Result<(), Stop> {
        if self.interp.stack.len() >= MAX_STACK { return Err(()); }
        self.interp.stack.push(v);
        Ok(())
    }
    fn pop(&mut self) -> Result<i32, Stop> { self.interp.stack.pop().ok_or(()) }
    fn pop_idx(&mut self) -> Result<usize, Stop> {
        let v = self.pop()?;
        if v < 0 { return Err(()); }
        Ok(v as usize)
    }

    fn execute(&mut self, kind: ProgramKind) -> Result<(), Stop> {
        let mut program = kind;
        let mut pos = 0usize;
        self.interp.stack.clear();
        self.interp.frames.clear();
        let mut code = self.programs.get(program);

        loop {
            self.steps += 1;
            if self.steps > MAX_STEPS { return Err(()); }

            if pos >= code.len() {
                match Self::unwind(&mut self.interp.frames, &mut program, &mut pos) {
                    true => { code = self.programs.get(program); continue; }
                    false => return Ok(()),
                }
            }

            let op = code[pos];
            pos += 1;

            match op {
                0x00 | 0x01 => {
                    let v = if op == 0x01 { Vector::X_AXIS } else { Vector::Y_AXIS };
                    self.interp.gs.projection = v;
                    self.interp.gs.dual_projection = v;
                    self.interp.gs.freedom = v;
                }
                0x02 | 0x03 => {
                    let v = if op == 0x03 { Vector::X_AXIS } else { Vector::Y_AXIS };
                    self.interp.gs.projection = v;
                    self.interp.gs.dual_projection = v;
                }
                0x04 | 0x05 => {
                    self.interp.gs.freedom = if op == 0x05 { Vector::X_AXIS } else { Vector::Y_AXIS };
                }
                0x0C => { let v = self.interp.gs.projection; self.push(v.x)?; self.push(v.y)?; }
                0x0D => { let v = self.interp.gs.freedom; self.push(v.x)?; self.push(v.y)?; }
                0x0E => { self.interp.gs.freedom = self.interp.gs.projection; }
                0x0F => { for _ in 0..5 { self.pop()?; } }

                0x10 => { self.interp.gs.rp0 = self.pop_idx()?; }
                0x11 => { self.interp.gs.rp1 = self.pop_idx()?; }
                0x12 => { self.interp.gs.rp2 = self.pop_idx()?; }
                0x13 => { self.interp.gs.zp0 = self.pop_idx()?.min(1); }
                0x14 => { self.interp.gs.zp1 = self.pop_idx()?.min(1); }
                0x15 => { self.interp.gs.zp2 = self.pop_idx()?.min(1); }
                0x16 => {
                    let z = self.pop_idx()?.min(1);
                    self.interp.gs.zp0 = z; self.interp.gs.zp1 = z; self.interp.gs.zp2 = z;
                }

                0x17 => { self.interp.gs.loop_count = self.pop()?.max(0); }
                0x18 => { self.interp.gs.round_state = RoundState::ToGrid; }
                0x19 => { self.interp.gs.round_state = RoundState::ToHalfGrid; }
                0x1A => { self.interp.gs.minimum_distance = self.pop()?; }
                0x1C => {
                    let offset = self.pop()?;
                    pos = Self::jump(pos, offset, 1, code.len())?;
                }
                0x1D => { self.interp.gs.control_value_cut_in = self.pop()?; }
                0x1E => { self.interp.gs.single_width_cut_in = self.pop()?; }
                0x1F => { self.interp.gs.single_width_value = self.pop()?; }

                0x20 => { let v = self.pop()?; self.push(v)?; self.push(v)?; }
                0x21 => { self.pop()?; }
                0x22 => self.interp.stack.clear(),
                0x23 => { let (a, b) = (self.pop()?, self.pop()?); self.push(a)?; self.push(b)?; }
                0x24 => { let d = self.interp.stack.len() as i32; self.push(d)?; }
                0x25 => {
                    let k = self.pop_idx()?;
                    if k == 0 || k > self.interp.stack.len() { return Err(()); }
                    let v = self.interp.stack[self.interp.stack.len() - k];
                    self.push(v)?;
                }
                0x26 => {
                    let k = self.pop_idx()?;
                    if k == 0 || k > self.interp.stack.len() { return Err(()); }
                    let at = self.interp.stack.len() - k;
                    let v = self.interp.stack.remove(at);
                    self.push(v)?;
                }
                0x27 => { self.pop()?; self.pop()?; }
                0x29 => { let _ = self.pop_idx()?; }

                0x2A | 0x2B => {
                    let id = self.pop_idx()?;
                    let count = if op == 0x2A { self.pop()? } else { 1 };
                    if count <= 0 { continue; }
                    let Some(Some(f)) = self.interp.functions.get(id).copied() else { return Err(()); };
                    if self.interp.frames.len() >= MAX_CALL_DEPTH { return Err(()); }
                    self.interp.frames.push(Frame { program, return_to: pos, remaining: count, body_start: f.start });
                    program = f.program;
                    code = self.programs.get(program);
                    pos = f.start;
                }
                0x2C => {
                    let id = self.pop_idx()?;
                    let start = pos;
                    let end = Self::skip_to_endf(code, pos, &mut self.steps)?;
                    if id < self.interp.functions.len() {
                        self.interp.functions[id] = Some(Function { start, program });
                        self.interp.functions_dirty = true;
                    }
                    pos = end;
                }
                0x89 => { let _ = self.pop_idx()?; pos = Self::skip_to_endf(code, pos, &mut self.steps)?; }
                0x2D => {
                    if !Self::unwind(&mut self.interp.frames, &mut program, &mut pos) { return Ok(()); }
                    code = self.programs.get(program);
                }

                0x2E | 0x2F => self.mdap(op == 0x2F)?,
                0x30 | 0x31 => self.iup(op == 0x30)?,
                0x32..=0x35 => self.consume_loop_points()?,
                0x36..=0x37 => { self.pop_idx()?; }
                0x38 => self.shpix()?,
                0x39 => self.consume_loop_points()?,
                0x3A | 0x3B => { self.pop()?; self.pop_idx()?; }
                0x3C => self.consume_loop_points()?,
                0x3D => { self.interp.gs.round_state = RoundState::ToDoubleGrid; }
                0x3E | 0x3F => self.miap(op == 0x3F)?,

                0x40 | 0x41 => {
                    let n = *code.get(pos).ok_or(())? as usize;
                    pos += 1;
                    pos = self.push_run(code, pos, n, op == 0x41)?;
                }
                0xB0..=0xB7 => { pos = self.push_run(code, pos, (op - 0xB0) as usize + 1, false)?; }
                0xB8..=0xBF => { pos = self.push_run(code, pos, (op - 0xB8) as usize + 1, true)?; }

                0x42 => { let (v, i) = (self.pop()?, self.pop_idx()?); if i < self.interp.storage.len() { self.interp.storage[i] = v; self.interp.storage_dirty = true; } }
                0x43 => { let i = self.pop_idx()?; let v = self.interp.storage.get(i).copied().unwrap_or(0); self.push(v)?; }
                0x44 => { let (v, i) = (self.pop()?, self.pop_idx()?); if i < self.interp.cvt.len() { self.interp.cvt[i] = v; self.interp.cvt_dirty = true; } }
                0x45 => { let i = self.pop_idx()?; let v = self.interp.cvt.get(i).copied().unwrap_or(0); self.push(v)?; }
                0x70 => {
                    let (v, i) = (self.pop()?, self.pop_idx()?);
                    if i < self.interp.cvt.len() {
                        self.interp.cvt[i] = f26dot6::scale(v, self.interp.ppem, self.interp.upm);
                        self.interp.cvt_dirty = true;
                    }
                }

                0x46 | 0x47 => {
                    let p = self.pop_idx()?;
                    let gs = &self.interp.gs;
                    let z = Self::zone_index(gs.zp2);
                    let v = if op == 0x46 { self.zones[z].project(gs, p) } else { self.zones[z].dual_project(gs, p) };
                    self.push(v)?;
                }
                0x48 => { self.pop()?; self.pop_idx()?; }
                0x49 | 0x4A => {
                    let (p2, p1) = (self.pop_idx()?, self.pop_idx()?);
                    let gs = &self.interp.gs;
                    let (z1, z0) = (Self::zone_index(gs.zp1), Self::zone_index(gs.zp0));
                    let d = if op == 0x49 {
                        self.zones[z1].project(gs, p2)
                            .saturating_sub(self.zones[z0].project(gs, p1))
                    } else {
                        self.zones[z1].dual_project(gs, p2)
                            .saturating_sub(self.zones[z0].dual_project(gs, p1))
                    };
                    self.push(d)?;
                }
                0x4B => { let v = self.interp.ppem as i32; self.push(v)?; }
                0x4C => { let v = self.interp.point_size; self.push(v)?; }
                0x4D => self.interp.gs.auto_flip = true,
                0x4E => self.interp.gs.auto_flip = false,
                0x4F => { self.pop()?; }

                0x50 => { let (b, a) = (self.pop()?, self.pop()?); self.push((a < b) as i32)?; }
                0x51 => { let (b, a) = (self.pop()?, self.pop()?); self.push((a <= b) as i32)?; }
                0x52 => { let (b, a) = (self.pop()?, self.pop()?); self.push((a > b) as i32)?; }
                0x53 => { let (b, a) = (self.pop()?, self.pop()?); self.push((a >= b) as i32)?; }
                0x54 => { let (b, a) = (self.pop()?, self.pop()?); self.push((a == b) as i32)?; }
                0x55 => { let (b, a) = (self.pop()?, self.pop()?); self.push((a != b) as i32)?; }
                0x56 => { let a = self.pop()?; self.push(((f26dot6::round_to_grid(a) / ONE) & 1 == 1) as i32)?; }
                0x57 => { let a = self.pop()?; self.push(((f26dot6::round_to_grid(a) / ONE) & 1 == 0) as i32)?; }
                0x5A => { let (b, a) = (self.pop()?, self.pop()?); self.push((a != 0 && b != 0) as i32)?; }
                0x5B => { let (b, a) = (self.pop()?, self.pop()?); self.push((a != 0 || b != 0) as i32)?; }
                0x5C => { let a = self.pop()?; self.push((a == 0) as i32)?; }

                0x58 => {
                    let cond = self.pop()?;
                    if cond == 0 { pos = Self::skip_to_else_or_eif(code, pos, &mut self.steps)?; }
                }
                0x1B => { pos = Self::skip_past_eif(code, pos, &mut self.steps)?; }
                0x59 => {}
                0x78 | 0x79 => {
                    let cond = self.pop()?;
                    let offset = self.pop()?;
                    let take = if op == 0x78 { cond != 0 } else { cond == 0 };
                    if take { pos = Self::jump(pos, offset, 2, code.len())?; }
                }

                0x5D | 0x71..=0x72 => self.delta_p()?,
                0x73..=0x75 => self.delta_c()?,
                0x5E => { self.interp.gs.delta_base = self.pop()?; }
                0x5F => { self.interp.gs.delta_shift = self.pop()?; }

                0x60 => { let (b, a) = (self.pop()?, self.pop()?); self.push(a.wrapping_add(b))?; }
                0x61 => { let (b, a) = (self.pop()?, self.pop()?); self.push(a.wrapping_sub(b))?; }
                0x62 => { let (b, a) = (self.pop()?, self.pop()?); self.push(f26dot6::div(a, b))?; }
                0x63 => { let (b, a) = (self.pop()?, self.pop()?); self.push(f26dot6::mul(a, b))?; }
                0x64 => { let a = self.pop()?; self.push(a.saturating_abs())?; }
                0x65 => { let a = self.pop()?; self.push(a.wrapping_neg())?; }
                0x66 => { let a = self.pop()?; self.push(f26dot6::floor_pixel(a))?; }
                0x67 => { let a = self.pop()?; self.push(f26dot6::ceil_pixel(a))?; }
                0x68..=0x6B => { let a = self.pop()?; let r = self.interp.gs.round_state.apply(a); self.push(r)?; }
                0x6C..=0x6F => { let a = self.pop()?; self.push(a)?; }
                0x76 | 0x77 => { let n = self.pop()?; self.interp.gs.round_state = Self::super_round(n, op == 0x77); }
                0x7A => { self.interp.gs.round_state = RoundState::Off; }
                0x7C => { self.interp.gs.round_state = RoundState::UpToGrid; }
                0x7D => { self.interp.gs.round_state = RoundState::DownToGrid; }
                0x7E => { self.pop()?; }
                0x7F => { self.pop()?; }
                0x80 => self.consume_loop_points()?,
                0x81 | 0x82 => { self.pop_idx()?; self.pop_idx()?; }
                0x85 => { let v = self.pop()?; self.interp.gs.scan_control = v != 0; }
                0x86 | 0x87 => { self.pop_idx()?; self.pop_idx()?; }
                0x88 => { let selector = self.pop()?; let v = self.get_info(selector); self.push(v)?; }
                0x8A => {
                    let (c, b, a) = (self.pop()?, self.pop()?, self.pop()?);
                    self.push(b)?; self.push(c)?; self.push(a)?;
                }
                0x8B => { let (b, a) = (self.pop()?, self.pop()?); self.push(a.max(b))?; }
                0x8C => { let (b, a) = (self.pop()?, self.pop()?); self.push(a.min(b))?; }
                0x8D => { self.interp.gs.scan_type = self.pop()?; }
                0x8E => { self.interp.gs.instruct_control = self.pop()?; self.pop()?; }

                0xC0..=0xDF => self.mdrp(op)?,
                0xE0..=0xFF => self.mirp(op)?,

                _ => return Err(()),
            }
        }
    }

    fn unwind(frames: &mut Vec<Frame>, program: &mut ProgramKind, pos: &mut usize) -> bool {
        match frames.pop() {
            None => false,
            Some(mut f) => {
                if f.remaining > 1 {
                    f.remaining -= 1;
                    *pos = f.body_start;
                    frames.push(f);
                } else {
                    *program = f.program;
                    *pos = f.return_to;
                }
                true
            }
        }
    }

    fn jump(pos: usize, offset: i32, from_op: usize, len: usize) -> Result<usize, Stop> {
        let base = pos as i64 - from_op as i64;
        let target = base + offset as i64;
        if target < 0 || target > len as i64 { return Err(()); }
        Ok(target as usize)
    }

    fn push_run(&mut self, code: &[u8], mut pos: usize, n: usize, words: bool) -> Result<usize, Stop> {
        for _ in 0..n {
            if words {
                let hi = *code.get(pos).ok_or(())? as i32;
                let lo = *code.get(pos + 1).ok_or(())? as i32;
                self.push((((hi << 8) | lo) as u16) as i16 as i32)?;
                pos += 2;
            } else {
                self.push(*code.get(pos).ok_or(())? as i32)?;
                pos += 1;
            }
        }
        Ok(pos)
    }

    fn skip_operands(code: &[u8], pos: usize, op: u8) -> Result<usize, Stop> {
        Ok(match op {
            0x40 => pos + 1 + *code.get(pos).ok_or(())? as usize,
            0x41 => pos + 1 + 2 * *code.get(pos).ok_or(())? as usize,
            0xB0..=0xB7 => pos + (op - 0xB0) as usize + 1,
            0xB8..=0xBF => pos + 2 * ((op - 0xB8) as usize + 1),
            _ => pos,
        })
    }

    fn skip_to_endf(code: &[u8], mut pos: usize, steps: &mut usize) -> Result<usize, Stop> {
        let mut depth = 0usize;
        while pos < code.len() {
            *steps += 1;
            if *steps > MAX_STEPS { return Err(()); }
            let op = code[pos];
            pos = Self::skip_operands(code, pos + 1, op)?;
            match op {
                0x2C | 0x89 => depth += 1,
                0x2D => {
                    if depth == 0 { return Ok(pos); }
                    depth -= 1;
                }
                _ => {}
            }
        }
        Err(())
    }

    fn skip_to_else_or_eif(code: &[u8], mut pos: usize, steps: &mut usize) -> Result<usize, Stop> {
        let mut depth = 0usize;
        while pos < code.len() {
            *steps += 1;
            if *steps > MAX_STEPS { return Err(()); }
            let op = code[pos];
            pos = Self::skip_operands(code, pos + 1, op)?;
            match op {
                0x58 => depth += 1,
                0x1B if depth == 0 => return Ok(pos),
                0x59 => {
                    if depth == 0 { return Ok(pos); }
                    depth -= 1;
                }
                _ => {}
            }
        }
        Err(())
    }

    fn skip_past_eif(code: &[u8], mut pos: usize, steps: &mut usize) -> Result<usize, Stop> {
        let mut depth = 0usize;
        while pos < code.len() {
            *steps += 1;
            if *steps > MAX_STEPS { return Err(()); }
            let op = code[pos];
            pos = Self::skip_operands(code, pos + 1, op)?;
            match op {
                0x58 => depth += 1,
                0x59 => {
                    if depth == 0 { return Ok(pos); }
                    depth -= 1;
                }
                _ => {}
            }
        }
        Err(())
    }

    fn super_round(n: i32, gray45: bool) -> RoundState {
        let grid_period: i32 = if gray45 { 0x2D41 } else { 0x4000 };
        let mut period = match n & 0xC0 {
            0x00 => grid_period / 2,
            0x40 => grid_period,
            0x80 => grid_period * 2,
            _ => grid_period,
        };
        let mut phase = match n & 0x30 {
            0x00 => 0,
            0x10 => period / 4,
            0x20 => period / 2,
            _ => period * 3 / 4,
        };
        let raw = n & 0x0F;
        let mut threshold = if raw == 0 { period - 1 } else { (raw - 4) * period / 8 };
        period >>= 8;
        phase >>= 8;
        threshold >>= 8;
        RoundState::Super { period: period.max(1), phase, threshold, gray45 }
    }

    fn get_info(&self, selector: i32) -> i32 {
        let mut out = 0;
        if selector & 1 != 0 {
            out |= match self.interp.mode {
                HintMode::Classic => 35,
                _ => 40,
            };
        }
        if selector & 0x40 != 0 && self.interp.mode == HintMode::Subpixel { out |= 1 << 17; }
        if selector & 0x400 != 0 && self.interp.mode == HintMode::Subpixel { out |= 1 << 13; }
        out
    }

    fn mdap(&mut self, round: bool) -> Result<(), Stop> {
        let p = self.pop_idx()?;
        let moves_x = self.interp.mode.moves_x();
        let z = Self::zone_index(self.interp.gs.zp0);
        self.interp.twilight_dirty |= z == 0;
        let gs = &self.interp.gs;
        let cur = self.zones[z].project(gs, p);
        let distance = if round { gs.round_state.apply(cur).saturating_sub(cur) } else { 0 };
        self.zones[z].move_point(gs, p, distance, moves_x);
        self.interp.gs.rp0 = p;
        self.interp.gs.rp1 = p;
        Ok(())
    }

    fn miap(&mut self, round: bool) -> Result<(), Stop> {
        let cvt_index = self.pop_idx()?;
        let p = self.pop_idx()?;
        let moves_x = self.interp.mode.moves_x();
        let mut value = self.interp.cvt.get(cvt_index).copied().unwrap_or(0);
        let z = Self::zone_index(self.interp.gs.zp0);
        self.interp.twilight_dirty |= z == 0;
        let gs = &self.interp.gs;
        let cur = self.zones[z].project(gs, p);
        if round {
            if value.saturating_sub(cur).saturating_abs() > gs.control_value_cut_in { value = cur; }
            value = gs.round_state.apply(value);
        }
        self.zones[z].move_point(gs, p, value.saturating_sub(cur), moves_x);
        self.interp.gs.rp0 = p;
        self.interp.gs.rp1 = p;
        Ok(())
    }

    fn mdrp(&mut self, op: u8) -> Result<(), Stop> {
        let p = self.pop_idx()?;
        let moves_x = self.interp.mode.moves_x();
        let (z1, z0) = (Self::zone_index(self.interp.gs.zp1), Self::zone_index(self.interp.gs.zp0));
        self.interp.twilight_dirty |= z1 == 0;
        let gs = &self.interp.gs;
        let rp0 = gs.rp0;

        let orig = self.zones[z1].dual_project(gs, p)
            .saturating_sub(self.zones[z0].dual_project(gs, rp0));
        let cur = self.zones[z1].project(gs, p)
            .saturating_sub(self.zones[z0].project(gs, rp0));

        let mut orig = orig;
        if gs.single_width_cut_in > 0
            && orig < gs.single_width_value.saturating_add(gs.single_width_cut_in)
            && orig > gs.single_width_value.saturating_sub(gs.single_width_cut_in)
        {
            orig = if orig >= 0 { gs.single_width_value } else { gs.single_width_value.saturating_neg() };
        }

        let mut distance = orig;
        if op & 0x04 != 0 { distance = gs.round_state.apply(distance); }
        if op & 0x08 != 0 {
            if distance >= 0 { distance = distance.max(gs.minimum_distance); }
            else { distance = distance.min(gs.minimum_distance.saturating_neg()); }
        }
        self.zones[z1].move_point(gs, p, distance.saturating_sub(cur), moves_x);
        self.interp.gs.rp1 = rp0;
        self.interp.gs.rp2 = p;
        if op & 0x10 != 0 { self.interp.gs.rp0 = p; }
        Ok(())
    }

    fn mirp(&mut self, op: u8) -> Result<(), Stop> {
        let cvt_index = self.pop_idx()?;
        let p = self.pop_idx()?;
        let moves_x = self.interp.mode.moves_x();
        let mut cvt = self.interp.cvt.get(cvt_index).copied().unwrap_or(0);
        let (z1, z0) = (Self::zone_index(self.interp.gs.zp1), Self::zone_index(self.interp.gs.zp0));
        self.interp.twilight_dirty |= z1 == 0;
        let gs = &self.interp.gs;
        let rp0 = gs.rp0;

        if cvt.saturating_sub(gs.single_width_value).saturating_abs() < gs.single_width_cut_in {
            cvt = if cvt >= 0 { gs.single_width_value } else { gs.single_width_value.saturating_neg() };
        }

        let orig = self.zones[z1].dual_project(gs, p)
            .saturating_sub(self.zones[z0].dual_project(gs, rp0));
        let cur = self.zones[z1].project(gs, p)
            .saturating_sub(self.zones[z0].project(gs, rp0));

        if gs.auto_flip && (orig < 0) != (cvt < 0) { cvt = cvt.saturating_neg(); }

        let mut distance = cvt;
        if op & 0x04 != 0 {
            if gs.zp0 == gs.zp1
                && cvt.saturating_sub(orig).saturating_abs() > gs.control_value_cut_in
            {
                distance = orig;
            }
            distance = gs.round_state.apply(distance);
        }
        if op & 0x08 != 0 {
            if orig >= 0 { distance = distance.max(gs.minimum_distance); }
            else { distance = distance.min(gs.minimum_distance.saturating_neg()); }
        }
        self.zones[z1].move_point(gs, p, distance.saturating_sub(cur), moves_x);
        self.interp.gs.rp1 = rp0;
        self.interp.gs.rp2 = p;
        if op & 0x10 != 0 { self.interp.gs.rp0 = p; }
        Ok(())
    }

    fn shpix(&mut self) -> Result<(), Stop> {
        let amount = self.pop()?;
        let moves_x = self.interp.mode.moves_x();
        let (count, z) = (self.interp.gs.loop_count.max(0), Self::zone_index(self.interp.gs.zp2));
        self.interp.twilight_dirty |= z == 0;
        for _ in 0..count {
            let p = self.pop_idx()?;
            self.zones[z].move_point(&self.interp.gs, p, amount, moves_x);
        }
        self.interp.gs.loop_count = 1;
        Ok(())
    }

    fn consume_loop_points(&mut self) -> Result<(), Stop> {
        let n = self.interp.gs.loop_count.max(0);
        for _ in 0..n { self.pop_idx()?; }
        self.interp.gs.loop_count = 1;
        Ok(())
    }

    fn delta_p(&mut self) -> Result<(), Stop> {
        let n = self.pop_idx()?;
        for _ in 0..n { self.pop()?; self.pop_idx()?; }
        Ok(())
    }

    fn delta_c(&mut self) -> Result<(), Stop> {
        let n = self.pop_idx()?;
        for _ in 0..n { self.pop()?; self.pop_idx()?; }
        Ok(())
    }

    fn iup(&mut self, vertical: bool) -> Result<(), Stop> {
        if !vertical && !self.interp.mode.moves_x() { return Ok(()); }
        let bit = if vertical { FLAG_TOUCHED_Y } else { FLAG_TOUCHED_X };
        let scratch = &mut self.interp.iup_touched;
        let zone = &mut *self.zones[1];
        let ends = core::mem::take(&mut zone.contour_ends);
        let mut start = 0usize;
        for &end in &ends {
            if end >= zone.len() { break; }
            iup_contour(zone, start, end, bit, vertical, scratch);
            start = end + 1;
        }
        zone.contour_ends = ends;
        Ok(())
    }
}

fn iup_contour(zone: &mut Zone, start: usize, end: usize, touched_bit: u8, vertical: bool, touched: &mut Vec<usize>) {
    if end < start { return; }
    touched.clear();
    touched.extend((start..=end).filter(|&i| zone.flags[i] & touched_bit != 0));
    if touched.is_empty() { return; }

    if touched.len() == 1 {
        let t = touched[0];
        let delta = if vertical { zone.current_y[t] - zone.original_y[t] } else { zone.current_x[t] - zone.original_x[t] };
        for i in start..=end {
            if i == t { continue; }
            if vertical { zone.current_y[i] = zone.original_y[i] + delta; }
            else { zone.current_x[i] = zone.original_x[i] + delta; }
        }
        return;
    }

    for w in 0..touched.len() {
        let a = touched[w];
        let b = touched[(w + 1) % touched.len()];
        let mut i = if a == end { start } else { a + 1 };
        while i != b {
            interpolate(zone, i, a, b, vertical);
            i = if i == end { start } else { i + 1 };
        }
    }
}

fn interpolate(zone: &mut Zone, i: usize, a: usize, b: usize, vertical: bool) {
    let (orig, cur) = if vertical { (&zone.original_y, &zone.current_y) } else { (&zone.original_x, &zone.current_x) };
    let (oi, oa, ob) = (orig[i], orig[a], orig[b]);
    let (ca, cb) = (cur[a], cur[b]);
    let (lo_o, hi_o, lo_c, hi_c) = if oa <= ob { (oa, ob, ca, cb) } else { (ob, oa, cb, ca) };

    let value = if oi <= lo_o {
        lo_c + (oi - lo_o)
    } else if oi >= hi_o {
        hi_c + (oi - hi_o)
    } else if hi_o == lo_o {
        lo_c
    } else {
        lo_c + ((oi - lo_o) as i64 * (hi_c - lo_c) as i64 / (hi_o - lo_o) as i64) as i32
    };

    if vertical { zone.current_y[i] = value; } else { zone.current_x[i] = value; }
}
