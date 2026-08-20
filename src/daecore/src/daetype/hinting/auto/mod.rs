mod blues;
mod latin;
pub mod points;

use alloc::vec::Vec;

pub use blues::{BlueZone, BlueZones};
pub use points::{AutoPoints, CollectPen, CONIC, CUBIC, ON_CURVE};

use super::{state::FLAG_ON_CURVE, HintedOutline};

pub struct AutoHinter {
    blues: BlueZones,
    upm: u16,
    scratch: latin::Scratch,
}

impl AutoHinter {
    pub fn new(
        upm: u16,
        resolve: &mut dyn FnMut(char) -> Option<u16>,
        outline_of: &mut dyn FnMut(u16) -> Option<AutoPoints>,
    ) -> Option<AutoHinter> {
        if upm == 0 {
            return None;
        }
        let blues = blues::compute(resolve, outline_of);
        if blues.zones.len() < 2 {
            return None;
        }
        Some(AutoHinter { blues, upm, scratch: latin::Scratch::default() })
    }

    pub fn from_zones(blues: BlueZones, upm: u16) -> Option<AutoHinter> {
        (upm != 0 && blues.zones.len() >= 2).then_some(AutoHinter { blues, upm, scratch: latin::Scratch::default() })
    }

    pub fn compute_zones(
        upm: u16,
        resolve: &mut dyn FnMut(char) -> Option<u16>,
        outline_of: &mut dyn FnMut(u16) -> Option<AutoPoints>,
    ) -> BlueZones {
        if upm == 0 { return BlueZones::default() }
        blues::compute(resolve, outline_of)
    }

    pub fn zones(&self) -> &BlueZones {
        &self.blues
    }

    pub fn hint(&mut self, pts: &AutoPoints, ppem: u16) -> HintedOutline {
        let y = latin::fit(pts, &self.blues, ppem, self.upm, &mut self.scratch).to_vec();
        let x = pts
            .x
            .iter()
            .map(|&v| super::f26dot6::scale(v as i32, ppem, self.upm))
            .collect::<Vec<i32>>();
        let flags = pts
            .flags
            .iter()
            .map(|&f| if f & ON_CURVE != 0 { FLAG_ON_CURVE } else { 0 })
            .collect();
        HintedOutline { x, y, flags, contour_ends: pts.contour_ends.clone() }
    }
}
