pub mod auto;
pub mod cff;
pub mod f26dot6;
mod interp;
mod state;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::decoder::{read_i16_be, read_u16_be};
use super::instancer::{extract_coords_into, GlyphCoords};
pub use state::{HintMode, FLAG_ON_CURVE};
use interp::{Interpreter, Machine, ProgramKind, Programs};
use state::{GraphicsState, Zone};
use crate::daecore::daetype::TableBytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintedOutline {
    pub x: Vec<i32>,
    pub y: Vec<i32>,
    pub flags: Vec<u8>,
    pub contour_ends: Vec<usize>,
}

pub struct HintContext {
    interp: Interpreter,
    fpgm: Vec<u8>,
    prep: Vec<u8>,
    max_points: usize,
    twilight: Zone,
    coords: GlyphCoords,
}

impl HintContext {
    pub fn new(
        table_map: &BTreeMap<String, TableBytes>,
        ppem: u16,
        upm: u16,
        mode: HintMode,
    ) -> Option<HintContext> {
        if !mode.runs_bytecode() { return None; }
        let fpgm = table_map.get("fpgm").map(TableBytes::to_owned_vec).unwrap_or_default();
        let prep = table_map.get("prep").map(TableBytes::to_owned_vec).unwrap_or_default();
        let cvt_raw = table_map.get("cvt ").map(TableBytes::to_owned_vec).unwrap_or_default();
        if fpgm.is_empty() && prep.is_empty() && cvt_raw.is_empty() { return None; }

        let maxp = table_map.get("maxp")?;
        let max_storage = read_u16_be(maxp, 18).unwrap_or(64) as usize;
        let max_functions = read_u16_be(maxp, 20).unwrap_or(64) as usize;
        let max_points = read_u16_be(maxp, 6).unwrap_or(0) as usize;

        let cvt: Vec<i32> = cvt_raw
            .chunks_exact(2)
            .map(|c| f26dot6::scale(i16::from_be_bytes([c[0], c[1]]) as i32, ppem, upm))
            .collect();

        let mut ctx = HintContext {
            interp: Interpreter {
                gs: GraphicsState::default(),
                prep_gs: GraphicsState::default(),
                storage: alloc::vec![0; max_storage.min(64 * 1024)],
                cvt,
                prep_storage: Vec::new(),
                prep_cvt: Vec::new(),
                functions: alloc::vec![None; max_functions.min(4096)],
                prep_functions: Vec::new(),
                functions_dirty: false,
                iup_touched: Vec::new(),
                stack: Vec::new(),
                frames: Vec::new(),
                storage_dirty: false,
                cvt_dirty: false,
                twilight_dirty: false,
                mode,
                ppem,
                upm,
                point_size: (ppem as i32) * f26dot6::ONE,
            },
            fpgm,
            prep,
            max_points,
            twilight: Zone::with_capacity(max_points.max(16)),
            coords: GlyphCoords::default(),
        };
        ctx.run_font_programs();
        Some(ctx)
    }

    fn run_font_programs(&mut self) {
        let mut twilight = Zone::with_capacity(self.max_points.max(16));
        let mut glyph = Zone::default();
        {
            let programs = Programs { font: &self.fpgm, control_value: &self.prep, glyph: &[] };
            let mut m = Machine::new(&mut self.interp, programs, &mut twilight, &mut glyph);
            m.run(ProgramKind::Font);
        }
        {
            let programs = Programs { font: &self.fpgm, control_value: &self.prep, glyph: &[] };
            let mut m = Machine::new(&mut self.interp, programs, &mut twilight, &mut glyph);
            m.run(ProgramKind::ControlValue);
        }
        self.interp.prep_gs = self.interp.gs.clone();
        self.interp.prep_storage = self.interp.storage.clone();
        self.interp.prep_cvt = self.interp.cvt.clone();
        self.interp.prep_functions = self.interp.functions.clone();
        self.interp.storage_dirty = false;
        self.interp.cvt_dirty = false;
        self.interp.twilight_dirty = false;
        self.interp.functions_dirty = false;
    }

    pub fn hint_glyph(
        &mut self,
        glyf: &[u8],
        loca: &[usize],
        gid: u16,
        ppem: u16,
        upm: u16,
    ) -> Option<HintedOutline> {
        let gid = gid as usize;
        if gid + 1 >= loca.len() { return None; }
        let (start, end) = (loca[gid], loca[gid + 1]);
        if end <= start { return None; }

        let n_contours = read_i16_be(glyf, start)?;
        if n_contours < 0 { return None; }
        let n_contours = n_contours as usize;

        let instructions = glyph_instructions(glyf, start, n_contours)?;
        if instructions.is_empty() { return None; }

        extract_coords_into(glyf, start, n_contours, &mut self.coords);
        if self.coords.num_points == 0 { return None; }

        let n = self.coords.num_points;
        let total = n + 4;
        let mut zone = Zone::with_capacity(total);
        for i in 0..n {
            let x = f26dot6::scale(self.coords.x_coords[i], ppem, upm);
            let y = f26dot6::scale(self.coords.y_coords[i], ppem, upm);
            zone.current_x[i] = x;
            zone.current_y[i] = y;
            zone.original_x[i] = x;
            zone.original_y[i] = y;
            zone.flags[i] = self.coords.flags[i] & FLAG_ON_CURVE;
        }
        zone.contour_ends.clone_from(&self.coords.end_pts);

        if self.interp.twilight_dirty {
            self.twilight.reset_zeroed(self.max_points.max(16));
            self.interp.twilight_dirty = false;
        }
        self.interp.gs = self.interp.prep_gs.clone();
        if self.interp.storage_dirty {
            self.interp.storage.clone_from(&self.interp.prep_storage);
            self.interp.storage_dirty = false;
        }
        if self.interp.cvt_dirty {
            self.interp.cvt.clone_from(&self.interp.prep_cvt);
            self.interp.cvt_dirty = false;
        }
        if self.interp.functions_dirty {
            self.interp.functions.clone_from(&self.interp.prep_functions);
            self.interp.functions_dirty = false;
        }
        {
            let programs = Programs { font: &self.fpgm, control_value: &self.prep, glyph: instructions };
            let mut m = Machine::new(&mut self.interp, programs, &mut self.twilight, &mut zone);
            m.run(ProgramKind::Glyph);
        }

        zone.current_x.truncate(n);
        zone.current_y.truncate(n);
        zone.flags.truncate(n);
        Some(HintedOutline {
            x: zone.current_x,
            y: zone.current_y,
            flags: zone.flags,
            contour_ends: zone.contour_ends,
        })
    }
}

fn glyph_instructions(glyf: &[u8], start: usize, n_contours: usize) -> Option<&[u8]> {
    let len_off = start + 10 + n_contours * 2;
    let len = read_u16_be(glyf, len_off)? as usize;
    let from = len_off + 2;
    glyf.get(from..from + len)
}

pub fn draw_hinted(out: &HintedOutline, pen: &mut dyn super::outline::OutlinePen) {
    let mut start = 0usize;
    for &end in &out.contour_ends {
        if end >= out.x.len() { break; }
        super::outline::draw_contour_over(&HintedContour { out, start, end }, pen);
        start = end + 1;
    }
}

struct HintedContour<'a> {
    out: &'a HintedOutline,
    start: usize,
    end: usize,
}

impl super::outline::ContourPoints for HintedContour<'_> {
    fn len(&self) -> usize { self.end - self.start + 1 }
    fn get(&self, i: usize) -> (f32, f32, bool) {
        let k = self.start + i;
        let px = |v: i32| v as f32 / f26dot6::ONE as f32;
        (px(self.out.x[k]), px(self.out.y[k]), self.out.flags[k] & FLAG_ON_CURVE != 0)
    }
}
