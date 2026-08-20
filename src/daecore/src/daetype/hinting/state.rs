use alloc::vec::Vec;
use super::f26dot6::{self, F2DOT14_ONE};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HintMode {
    #[default]
    None,
    Subpixel,
    Classic,
    Auto,
    AutoForce,
}

impl HintMode {
    pub(crate) fn moves_x(self) -> bool {
        matches!(self, HintMode::Classic)
    }

    pub(crate) fn runs_bytecode(self) -> bool {
        matches!(self, HintMode::Subpixel | HintMode::Classic | HintMode::Auto)
    }

    pub fn may_autohint(self) -> bool {
        matches!(self, HintMode::Auto | HintMode::AutoForce)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Vector {
    pub x: i32,
    pub y: i32,
}

impl Vector {
    pub(crate) const X_AXIS: Vector = Vector { x: F2DOT14_ONE, y: 0 };
    pub(crate) const Y_AXIS: Vector = Vector { x: 0, y: F2DOT14_ONE };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RoundState {
    ToGrid,
    ToHalfGrid,
    ToDoubleGrid,
    DownToGrid,
    UpToGrid,
    Off,
    Super { period: i32, phase: i32, threshold: i32, gray45: bool },
}

impl RoundState {
    pub(crate) fn apply(self, distance: i32) -> i32 {
        use RoundState::*;
        match self {
            Off => distance,
            ToGrid => f26dot6::round_to_grid(distance),
            ToHalfGrid => f26dot6::round_to_half_grid(distance),
            DownToGrid => {
                if distance >= 0 {
                    f26dot6::floor_pixel(distance)
                } else {
                    f26dot6::floor_pixel(distance.saturating_neg()).saturating_neg()
                }
            }
            UpToGrid => {
                if distance >= 0 {
                    f26dot6::ceil_pixel(distance)
                } else {
                    f26dot6::ceil_pixel(distance.saturating_neg()).saturating_neg()
                }
            }
            ToDoubleGrid => {
                let half = f26dot6::ONE / 2;
                if distance >= 0 {
                    distance.saturating_add(half / 2) & !(half - 1)
                } else {
                    (distance.saturating_neg().saturating_add(half / 2) & !(half - 1)).saturating_neg()
                }
            }
            Super { period, phase, threshold, gray45 } => {
                if period <= 0 { return distance; }
                let neg = distance < 0;
                let d = if neg { distance.saturating_neg() } else { distance };
                let mut r = d.saturating_add(threshold).saturating_sub(phase);
                r = if gray45 {
                    (r / period).saturating_mul(period)
                } else {
                    r & period.saturating_neg()
                };
                if r < 0 { r = 0; }
                r = r.saturating_add(phase);
                if neg { r.saturating_neg() } else { r }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct GraphicsState {
    pub projection: Vector,
    pub dual_projection: Vector,
    pub freedom: Vector,
    pub rp0: usize,
    pub rp1: usize,
    pub rp2: usize,
    pub zp0: usize,
    pub zp1: usize,
    pub zp2: usize,
    pub round_state: RoundState,
    pub loop_count: i32,
    pub minimum_distance: i32,
    pub control_value_cut_in: i32,
    pub single_width_cut_in: i32,
    pub single_width_value: i32,
    pub auto_flip: bool,
    pub delta_base: i32,
    pub delta_shift: i32,
    pub instruct_control: i32,
    pub scan_control: bool,
    pub scan_type: i32,
}

impl Default for GraphicsState {
    fn default() -> Self {
        GraphicsState {
            projection: Vector::X_AXIS,
            dual_projection: Vector::X_AXIS,
            freedom: Vector::X_AXIS,
            rp0: 0, rp1: 0, rp2: 0,
            zp0: 1, zp1: 1, zp2: 1,
            round_state: RoundState::ToGrid,
            loop_count: 1,
            minimum_distance: f26dot6::ONE,
            control_value_cut_in: 68,
            single_width_cut_in: 0,
            single_width_value: 0,
            auto_flip: true,
            delta_base: 9,
            delta_shift: 3,
            instruct_control: 0,
            scan_control: false,
            scan_type: 0,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Zone {
    pub current_x: Vec<i32>,
    pub current_y: Vec<i32>,
    pub original_x: Vec<i32>,
    pub original_y: Vec<i32>,
    pub flags: Vec<u8>,
    pub contour_ends: Vec<usize>,
}

pub const FLAG_ON_CURVE: u8 = 0x01;
pub(crate) const FLAG_TOUCHED_X: u8 = 0x02;
pub(crate) const FLAG_TOUCHED_Y: u8 = 0x04;

impl Zone {
    pub(crate) fn with_capacity(n: usize) -> Zone {
        Zone {
            current_x: alloc::vec![0; n],
            current_y: alloc::vec![0; n],
            original_x: alloc::vec![0; n],
            original_y: alloc::vec![0; n],
            flags: alloc::vec![0; n],
            contour_ends: Vec::new(),
        }
    }

    pub(crate) fn reset_zeroed(&mut self, n: usize) {
        for v in [&mut self.current_x, &mut self.current_y, &mut self.original_x, &mut self.original_y] {
            v.clear();
            v.resize(n, 0);
        }
        self.flags.clear();
        self.flags.resize(n, 0);
        self.contour_ends.clear();
    }

    pub(crate) fn len(&self) -> usize {
        self.current_x.len()
    }

    pub(crate) fn move_point(&mut self, gs: &GraphicsState, index: usize, distance: i32, moves_x: bool) {
        if index >= self.len() { return; }
        let (fx, fy) = (gs.freedom.x, gs.freedom.y);
        let denom = f26dot6::mul_f2dot14(gs.projection.x, fx)
            .saturating_add(f26dot6::mul_f2dot14(gs.projection.y, fy));
        if denom == 0 { return; }

        if fx != 0 {
            let dx = f26dot6::clamp_i32(distance as i64 * fx as i64 / denom as i64);
            if moves_x {
                self.current_x[index] = self.current_x[index].saturating_add(dx);
            }
            self.flags[index] |= FLAG_TOUCHED_X;
        }
        if fy != 0 {
            let dy = f26dot6::clamp_i32(distance as i64 * fy as i64 / denom as i64);
            self.current_y[index] = self.current_y[index].saturating_add(dy);
            self.flags[index] |= FLAG_TOUCHED_Y;
        }
    }

    pub(crate) fn project(&self, gs: &GraphicsState, index: usize) -> i32 {
        if index >= self.len() { return 0; }
        f26dot6::mul_f2dot14(self.current_x[index], gs.projection.x)
            .saturating_add(f26dot6::mul_f2dot14(self.current_y[index], gs.projection.y))
    }

    pub(crate) fn dual_project(&self, gs: &GraphicsState, index: usize) -> i32 {
        if index >= self.len() { return 0; }
        f26dot6::mul_f2dot14(self.original_x[index], gs.dual_projection.x)
            .saturating_add(f26dot6::mul_f2dot14(self.original_y[index], gs.dual_projection.y))
    }
}
