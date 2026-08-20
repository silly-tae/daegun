use core::ffi::c_void;

use crate::OutlinePen;

#[repr(C)]
#[derive(Clone, Copy)]
// Reentrancy is safe and the header promises it: a caller will want to ask the font something from
// inside its own pen, and daegun holds no guard across the callback – the instanced path takes its
// lock only long enough to clone an `Arc` out of the cache. Adding a guard here breaks that.
//
// A null callback means ignore that event, so a caller wanting only the on-curve points supplies
// three of the five. A callback that unwinds through these frames is undefined behaviour and
// nothing here can prevent it.
pub struct Pen {
    pub move_to: Option<extern "C" fn(*mut c_void, f32, f32)>,
    pub line_to: Option<extern "C" fn(*mut c_void, f32, f32)>,
    pub quad_to: Option<extern "C" fn(*mut c_void, f32, f32, f32, f32)>,
    pub curve_to: Option<extern "C" fn(*mut c_void, f32, f32, f32, f32, f32, f32)>,
    pub close: Option<extern "C" fn(*mut c_void)>,
    pub user: *mut c_void,
}

const _: () = assert!(size_of::<Pen>() == 6 * size_of::<usize>());
const _: () = assert!(size_of::<Option<extern "C" fn(*mut c_void)>>() == size_of::<usize>());

pub struct PenBridge(pub Pen);

impl OutlinePen for PenBridge {
    fn move_to(&mut self, x: f32, y: f32) {
        if let Some(f) = self.0.move_to {
            f(self.0.user, x, y);
        }
    }

    fn line_to(&mut self, x: f32, y: f32) {
        if let Some(f) = self.0.line_to {
            f(self.0.user, x, y);
        }
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        if let Some(f) = self.0.quad_to {
            f(self.0.user, cx, cy, x, y);
        }
    }

    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        if let Some(f) = self.0.curve_to {
            f(self.0.user, c1x, c1y, c2x, c2y, x, y);
        }
    }

    fn close(&mut self) {
        if let Some(f) = self.0.close {
            f(self.0.user);
        }
    }
}
