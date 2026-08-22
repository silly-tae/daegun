use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use daegun::{Font, OutlinePen};

// Stores nothing, so what it counts is the extraction and not a pen filling its own buffers.
#[derive(Default)]
struct Sink(usize);

impl OutlinePen for Sink {
    fn move_to(&mut self, _: f32, _: f32) { self.0 += 1 }
    fn line_to(&mut self, _: f32, _: f32) { self.0 += 1 }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) { self.0 += 1 }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) { self.0 += 1 }
    fn close(&mut self) { self.0 += 1 }
}

const FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test-fonts");

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static COUNTING: AtomicBool = AtomicBool::new(false);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

const ROUNDS: usize = 200;

fn font() -> Font {
    let path = format!("{FONTS}/inter/InterVariable.ttf");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {path} ({e})"));
    Font::from_vec(bytes).expect("parses")
}

fn allocs_per_call(mut f: impl FnMut()) -> f64 {
    for _ in 0..8 {
        f();
    }
    ALLOCS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    for _ in 0..ROUNDS {
        f();
    }
    COUNTING.store(false, Ordering::Relaxed);
    ALLOCS.load(Ordering::Relaxed) as f64 / ROUNDS as f64
}

// One test, not three: the counter is process-wide, so cases measuring it cannot run concurrently.
// A hit allocates one string per axis tag, the vector holding them, and the text of the key.
#[test]
fn hot_paths_stay_within_their_allocation_budget() {
    let font = font();
    let text = "your all-in-one text engine";

    let axes: &[(&str, f64)] = &[("wght", 400.0)];
    let with_axes = allocs_per_call(|| {
        std::hint::black_box(font.shape(text, axes, false));
    });
    let without = allocs_per_call(|| {
        std::hint::black_box(font.shape(text, &[], false));
    });

    let gid = font.glyph_id('a' as u32).expect("font has a");
    let outline = allocs_per_call(|| {
        let mut pen = Sink::default();
        std::hint::black_box(font.outline_glyph(gid, &mut pen));
    });

    assert!(with_axes <= 3.0, "a shape cache hit allocated {with_axes} times, budget is 3");
    assert!(without <= 1.0, "an axis-free shape cache hit allocated {without} times, budget is 1");
    assert!(outline <= 2.0, "outline extraction allocated {outline} times, budget is 2");

    // Overlap resolution accounts for nearly all of a cold rasterization, so this budget is really
    // a bound on how often it rebuilds its edge lists.
    let cold = allocs_per_call(|| {
        font.clear_glyph_cache();
        std::hint::black_box(font.rasterize_glyph(gid, 40.0, &[]));
    });
    assert!(cold <= 18.0, "a cold rasterization allocated {cold} times, budget is 18");
}
