use alloc::string::String;
use core::ffi::{CStr, c_char, c_void};
use core::mem;

pub type Id = *mut c_void;
pub type Sel = *const c_void;
// `signed char` on every ARM64 Apple platform, never `int`.
pub type Bool = i8;

pub const NO: Bool = 0;
const NSUTF8_STRING_ENCODING: u64 = 4;

#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

pub fn class(name: &CStr) -> Id {
    unsafe { objc_getClass(name.as_ptr()) }
}

pub fn sel(name: &CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}

macro_rules! send {
    ($name:ident $(, $arg:ident: $ty:ident)*) => {
        #[allow(clippy::too_many_arguments, reason = "the arity is the selector's; only send9 trips it")]
        #[inline]
        // objc_msgSend has no one signature – the ARM64 ABI requires calling through a pointer
        // typed for the method, and a wrong shape is not a compile error but silent corruption.
        // So the transmute lives in these six functions and every call site writes its types out.
        pub unsafe fn $name<$($ty,)* R>(obj: Id, sel: Sel $(, $arg: $ty)*) -> R {
            let send: unsafe extern "C" fn(Id, Sel $(, $ty)*) -> R =
                unsafe { mem::transmute(objc_msgSend as *const c_void) };
            unsafe { send(obj, sel $(, $arg)*) }
        }
    };
}

send!(send0);
send!(send1, a: A);
send!(send2, a: A, b: B);
send!(send3, a: A, b: B, c: C);
send!(send4, a: A, b: B, c: C, d: D);
send!(send9, a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I);

pub struct Owned(Id);

impl Owned {
    pub unsafe fn new(id: Id) -> Option<Owned> {
        (!id.is_null()).then_some(Owned(id))
    }

    pub fn id(&self) -> Id {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { send0::<()>(self.0, sel(c"release")) }
    }
}

pub unsafe fn retain(id: Id) -> Option<Owned> {
    if id.is_null() {
        return None;
    }
    unsafe { Owned::new(send0(id, sel(c"retain"))) }
}

pub struct Pool(*mut c_void);

impl Pool {
    pub fn new() -> Pool {
        Pool(unsafe { objc_autoreleasePoolPush() })
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}

pub fn nsstring(s: &str) -> Option<Owned> {
    let alloc: Id = unsafe { send0(class(c"NSString"), sel(c"alloc")) };
    let id: Id = unsafe {
        send3(
            alloc,
            sel(c"initWithBytes:length:encoding:"),
            s.as_ptr().cast::<c_void>(),
            s.len() as u64,
            NSUTF8_STRING_ENCODING,
        )
    };
    unsafe { Owned::new(id) }
}

pub unsafe fn to_string(s: Id) -> String {
    if s.is_null() {
        return String::new();
    }
    let utf8: *const c_char = unsafe { send0(s, sel(c"UTF8String")) };
    if utf8.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(utf8) }.to_string_lossy().into_owned()
}

pub unsafe fn error_message(err: Id) -> String {
    if err.is_null() {
        return String::new();
    }
    let desc: Id = unsafe { send0(err, sel(c"localizedDescription")) };
    unsafe { to_string(desc) }
}
