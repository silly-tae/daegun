fn scalar_coverage(a: &mut [f32], length: usize) {
    let mut height = 0.0f32;
    for slot in a.iter_mut().take(length) {
        height += *slot;
        *slot = f32::from_bits(height.to_bits() & 0x7fff_ffff).clamp(0.0, 1.0);
    }
}

fn scalar_bitmap(a: &[f32], length: usize) -> Vec<u8> {
    let mut height = 0.0f32;
    a.iter()
        .take(length)
        .map(|delta| {
            height += delta;
            (f32::from_bits(height.to_bits() & 0x7fff_ffff) * 255.9).clamp(0.0, 255.0) as u8
        })
        .collect()
}

fn scalar_tap3(cov: &[f32], w0: &[f32], w1: &[f32], w2: &[f32]) -> (f32, f32, f32) {
    let (mut s0, mut s1, mut s2) = (0.0f32, 0.0, 0.0);
    for i in 0..cov.len() {
        s0 += cov[i] * w0[i];
        s1 += cov[i] * w1[i];
        s2 += cov[i] * w2[i];
    }
    (s0, s1, s2)
}

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f32 / (1u64 << 53) as f32
    }
    fn deltas(&mut self, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let u = self.unit();
                if u < 0.75 {
                    0.0
                } else if i % 97 == 0 {
                    (self.unit() - 0.5) * 2.0
                } else {
                    (self.unit() - 0.5) * 0.25
                }
            })
            .collect()
    }
}

#[test]
fn coverage_in_place_tracks_the_scalar_sum() {
    let mut rng = Rng::new(0x2545_F491_4F6C_DD1D);
    let mut worst = 0.0f32;
    let mut checked = 0usize;

    let lengths: Vec<usize> = (0..=40).chain([63, 64, 65, 255, 256, 257, 1023, 4096, 9973]).collect();
    for &n in &lengths {
        for _ in 0..40 {
            let deltas = rng.deltas(n);
            let mut a = deltas.clone();
            let mut b = deltas;
            scalar_coverage(&mut a, n);
            daegun::daerizer::daecpu::simd::coverage_in_place(&mut b, n);
            assert_eq!(a.len(), b.len(), "length {n}: the kernels returned different sizes");
            for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                assert!(x.is_finite() && y.is_finite(), "length {n} slot {i}: non-finite coverage");
                assert!((0.0..=1.0).contains(y), "length {n} slot {i}: coverage {y} outside 0..=1");
                worst = worst.max((x - y).abs());
                checked += 1;
            }
        }
    }
    assert!(checked > 500_000, "only {checked} values compared");
    assert!(worst < 1e-5, "coverage diverged by {worst:.3e}, past what reordering explains");
}

#[test]
fn get_bitmap_matches_the_scalar_bytes() {
    let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
    let mut differing = 0usize;
    let mut total = 0usize;
    let mut worst = 0i32;

    let lengths: Vec<usize> = (0..=40).chain([63, 64, 65, 255, 256, 257, 1023, 4096, 9973]).collect();
    for &n in &lengths {
        for _ in 0..40 {
            let deltas = rng.deltas(n);
            let s = scalar_bitmap(&deltas, n);
            let v = daegun::daerizer::daecpu::simd::get_bitmap(&deltas, n);
            assert_eq!(s.len(), v.len(), "length {n}: byte counts differ");
            for (x, y) in s.iter().zip(&v) {
                total += 1;
                if x != y {
                    differing += 1;
                    worst = worst.max((i32::from(*x) - i32::from(*y)).abs());
                }
            }
        }
    }
    assert!(total > 500_000, "only {total} bytes compared");
    assert!(
        worst <= 1,
        "a byte differs from the scalar collapse by {worst} levels; reordering a sum can move the \
         quantised result by one, not more",
    );
    let rate = differing as f64 / total as f64;
    assert!(
        rate < 1e-4,
        "{differing} of {total} bytes differ ({:.5}%), past the ~4-in-100,000 that summation order \
         accounts for",
        rate * 100.0,
    );
}

#[test]
fn tap_run3_tracks_the_scalar_dot_products() {
    let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
    let mut worst = 0.0f64;
    let mut checked = 0usize;

    for span in 0..=32usize {
        for _ in 0..300 {
            let cov: Vec<f32> = (0..span).map(|_| rng.unit()).collect();
            let w0: Vec<f32> = (0..span).map(|_| rng.unit() * 0.3).collect();
            let w1: Vec<f32> = (0..span).map(|_| rng.unit() * 0.3).collect();
            let w2: Vec<f32> = (0..span).map(|_| rng.unit() * 0.3).collect();
            let s = scalar_tap3(&cov, &w0, &w1, &w2);
            let v = daegun::daerizer::daecpu::simd::tap_run3(&cov, &w0, &w1, &w2);
            for (a, b) in [(s.0, v.0), (s.1, v.1), (s.2, v.2)] {
                assert!(b.is_finite(), "span {span}: non-finite sum");
                let rel = if a.abs() > 1e-6 {
                    f64::from((a - b) / a).abs()
                } else {
                    f64::from(a - b).abs()
                };
                worst = worst.max(rel);
                checked += 1;
            }
        }
    }
    assert!(checked > 25_000, "only {checked} sums compared");
    assert!(worst < 1e-5, "tap sums diverged by {worst:.3e}, past rounding");
}

#[test]
fn the_kernels_survive_degenerate_input() {
    let mut empty: [f32; 0] = [];
    daegun::daerizer::daecpu::simd::coverage_in_place(&mut empty, 0);
    assert!(daegun::daerizer::daecpu::simd::get_bitmap(&[], 0).is_empty());
    assert_eq!(daegun::daerizer::daecpu::simd::tap_run3(&[], &[], &[], &[]), (0.0, 0.0, 0.0));

    let mut one = [0.5f32];
    daegun::daerizer::daecpu::simd::coverage_in_place(&mut one, 99);
    assert_eq!(one[0], 0.5);
    assert_eq!(daegun::daerizer::daecpu::simd::get_bitmap(&[1.0], 99).len(), 1);

    let (a, b, c) = daegun::daerizer::daecpu::simd::tap_run3(&[1.0, 1.0, 1.0], &[1.0], &[1.0, 1.0], &[1.0]);
    assert_eq!((a, b, c), (1.0, 1.0, 1.0));
}
