// The machinery ~200 entry points are built from, so all of them can be mechanical. C has no
// borrow checker, so a mistake here is undefined behavior rather than a compile error:
//
//   1. A fallible call returns `Status` and writes its result through an out-parameter.
//   2. A null pointer is `Status::Null`, never a dereference.
//   3. daegun allocates and daegun frees. A C caller's `free()` never touches a daegun pointer.
//   4. A borrowed view is a `const` pointer plus a count, valid until its owner is freed.

use alloc::boxed::Box;
use core::ffi::c_char;

#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
// The values are frozen: a C caller compares against the constants in `daegun.h`, so renumbering
// silently breaks every compiled consumer rather than failing to build. New codes append.
pub enum Status {
    Ok = 0,
    Null = -1,
    Parse = -2,
    Range = -3,
    Absent = -4,
    Unsupported = -5,
}

#[inline]
pub unsafe fn deliver<T>(out: *mut *mut T, value: T) -> Status {
    if out.is_null() {
        return Status::Null;
    }
    unsafe { *out = Box::into_raw(Box::new(value)) };
    Status::Ok
}

#[inline]
// Tolerating null is a safety property, not politeness: a caller freeing in a cleanup path after a
// failed open would otherwise guard every call, and the guard it forgets is a crash.
pub unsafe fn release<T>(handle: *mut T) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[inline]
pub unsafe fn borrow<'a, T>(handle: *const T) -> Option<&'a T> {
    if handle.is_null() {
        None
    } else {
        Some(unsafe { &*handle })
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bytes {
    pub data: *const u8,
    pub len: usize,
}

const _: () = assert!(size_of::<Bytes>() == 2 * size_of::<usize>());
const _: () = assert!(align_of::<Bytes>() == align_of::<usize>());

#[allow(dead_code, reason = "the borrowed-view machinery lands with the calls that return runs of bytes; round 0 only carries the string half")]
impl Bytes {
    pub const EMPTY: Bytes = Bytes { data: core::ptr::null(), len: 0 };

    pub fn of(slice: &[u8]) -> Bytes {
        Bytes { data: slice.as_ptr(), len: slice.len() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Str {
    pub data: *const c_char,
    pub len: usize,
}

const _: () = assert!(size_of::<Str>() == 2 * size_of::<usize>());
const _: () = assert!(align_of::<Str>() == align_of::<usize>());

impl Str {
    pub const EMPTY: Str = Str { data: c"".as_ptr(), len: 0 };
}

pub struct OwnedStr(alloc_cstring::CString);

mod alloc_cstring {
    pub use std::ffi::CString;
}

impl OwnedStr {
    // Interior NULs are replaced rather than refused – `CString::new` rejects them, which would turn
    // a font with an odd name into a failed call. `len` still describes the whole string.
    pub fn new(s: &str) -> OwnedStr {
        let cleaned: alloc::string::String =
            s.chars().map(|c| if c == '\0' { '\u{fffd}' } else { c }).collect();
        OwnedStr(alloc_cstring::CString::new(cleaned).unwrap_or_default())
    }

    pub fn as_str(&self) -> Str {
        let bytes = self.0.as_bytes();
        Str { data: self.0.as_ptr(), len: bytes.len() }
    }
}

extern crate alloc;
