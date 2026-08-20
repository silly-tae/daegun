use alloc::vec::Vec;

// No runtime detection: SSE2 is part of the base x86-64 spec and NEON is mandatory on aarch64,
// so both are selected by `cfg` and always present. `is_x86_feature_detected!` is std-only.
pub const BACKEND: &str = if cfg!(target_arch = "x86_64") {
    "sse2"
} else if cfg!(target_arch = "aarch64") {
    "neon"
} else {
    "scalar"
};

// 128 bits and not 256: a glyph at text sizes is a handful of pixels across, so wider lanes go
// mostly to tail handling – and AVX-512 is fused off on consumer Intel and absent on Apple.
pub fn coverage_in_place(a: &mut [f32], length: usize) {
    let n = length.min(a.len());
    let (head, tail) = a[..n].split_at_mut(n - n % 4);
    let mut carry = 0.0f32;

    for block in head.chunks_exact_mut(4) {
        carry = prefix_block(block, carry);
    }
    for slot in tail {
        carry += *slot;
        *slot = clamp01(abs(carry));
    }
}

pub fn get_bitmap(a: &[f32], length: usize) -> Vec<u8> {
    let n = length.min(a.len());
    let mut out = Vec::with_capacity(n);
    let mut carry = 0.0f32;
    let mut i = 0;

    while i + 4 <= n {
        let mut block = [a[i], a[i + 1], a[i + 2], a[i + 3]];
        carry = prefix_block_scaled(&mut block, carry);
        out.extend_from_slice(&[block[0] as u8, block[1] as u8, block[2] as u8, block[3] as u8]);
        i += 4;
    }
    while i < n {
        carry += a[i];
        out.push(clamp(abs(carry) * 255.9, 0.0, 255.0) as u8);
        i += 1;
    }
    out
}

#[inline(always)]
fn prefix_block(block: &mut [f32], carry: f32) -> f32 {
    let (s0, s1, s2, s3) = prefix4(block[0], block[1], block[2], block[3], carry);
    block[0] = clamp01(abs(s0));
    block[1] = clamp01(abs(s1));
    block[2] = clamp01(abs(s2));
    block[3] = clamp01(abs(s3));
    s3
}

#[inline(always)]
fn prefix_block_scaled(block: &mut [f32; 4], carry: f32) -> f32 {
    let (s0, s1, s2, s3) = prefix4(block[0], block[1], block[2], block[3], carry);
    block[0] = clamp(abs(s0) * 255.9, 0.0, 255.0);
    block[1] = clamp(abs(s1) * 255.9, 0.0, 255.0);
    block[2] = clamp(abs(s2) * 255.9, 0.0, 255.0);
    block[3] = clamp(abs(s3) * 255.9, 0.0, 255.0);
    s3
}

#[inline(always)]
fn prefix4(d0: f32, d1: f32, d2: f32, d3: f32, carry: f32) -> (f32, f32, f32, f32) {
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::*;
        unsafe {
            let x = _mm_set_ps(d3, d2, d1, d0);
            let x = _mm_add_ps(x, _mm_castsi128_ps(_mm_slli_si128(_mm_castps_si128(x), 4)));
            let x = _mm_add_ps(x, _mm_castsi128_ps(_mm_slli_si128(_mm_castps_si128(x), 8)));
            let x = _mm_add_ps(x, _mm_set1_ps(carry));
            let mut out = [0.0f32; 4];
            _mm_storeu_ps(out.as_mut_ptr(), x);
            (out[0], out[1], out[2], out[3])
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        use core::arch::aarch64::*;
        unsafe {
            let x = {
                let v = [d0, d1, d2, d3];
                vld1q_f32(v.as_ptr())
            };
            let zero = vdupq_n_f32(0.0);
            let x = vaddq_f32(x, vextq_f32(zero, x, 3));
            let x = vaddq_f32(x, vextq_f32(zero, x, 2));
            let x = vaddq_f32(x, vdupq_n_f32(carry));
            let mut out = [0.0f32; 4];
            vst1q_f32(out.as_mut_ptr(), x);
            (out[0], out[1], out[2], out[3])
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let (a0, a1, a2, a3) = (d0, d1, d2, d3);
        let (b0, b1, b2, b3) = (a0, a1 + a0, a2 + a1, a3 + a2);
        let (c0, c1, c2, c3) = (b0, b1, b2 + b0, b3 + b1);
        (c0 + carry, c1 + carry, c2 + carry, c3 + carry)
    }
}

#[inline(always)]
fn abs(v: f32) -> f32 {
    f32::from_bits(v.to_bits() & 0x7fff_ffff)
}

#[inline(always)]
fn clamp01(v: f32) -> f32 {
    clamp(v, 0.0, 1.0)
}

#[inline(always)]
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[inline]
// This module is `pub` only so `simd_diff.rs` and `simd_agreement.rs` can grade these kernels
// against the scalar ones; an integration test is a separate crate. Nothing else calls them.
pub fn tap_run3(cov: &[f32], w0: &[f32], w1: &[f32], w2: &[f32]) -> (f32, f32, f32) {
    let n = cov.len().min(w0.len()).min(w1.len()).min(w2.len());

    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::*;
        unsafe {
            let (mut a0, mut a1, mut a2) = (_mm_setzero_ps(), _mm_setzero_ps(), _mm_setzero_ps());
            let mut i = 0;
            while i + 4 <= n {
                let c = _mm_loadu_ps(cov.as_ptr().add(i));
                a0 = _mm_add_ps(a0, _mm_mul_ps(c, _mm_loadu_ps(w0.as_ptr().add(i))));
                a1 = _mm_add_ps(a1, _mm_mul_ps(c, _mm_loadu_ps(w1.as_ptr().add(i))));
                a2 = _mm_add_ps(a2, _mm_mul_ps(c, _mm_loadu_ps(w2.as_ptr().add(i))));
                i += 4;
            }
            let (mut s0, mut s1, mut s2) = (hsum(a0), hsum(a1), hsum(a2));
            while i < n {
                let c = cov[i];
                s0 += c * w0[i];
                s1 += c * w1[i];
                s2 += c * w2[i];
                i += 1;
            }
            (s0, s1, s2)
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        use core::arch::aarch64::*;
        unsafe {
            let (mut a0, mut a1, mut a2) = (vdupq_n_f32(0.0), vdupq_n_f32(0.0), vdupq_n_f32(0.0));
            let mut i = 0;
            while i + 4 <= n {
                let c = vld1q_f32(cov.as_ptr().add(i));
                a0 = vfmaq_f32(a0, c, vld1q_f32(w0.as_ptr().add(i)));
                a1 = vfmaq_f32(a1, c, vld1q_f32(w1.as_ptr().add(i)));
                a2 = vfmaq_f32(a2, c, vld1q_f32(w2.as_ptr().add(i)));
                i += 4;
            }
            let (mut s0, mut s1, mut s2) = (vaddvq_f32(a0), vaddvq_f32(a1), vaddvq_f32(a2));
            while i < n {
                let c = cov[i];
                s0 += c * w0[i];
                s1 += c * w1[i];
                s2 += c * w2[i];
                i += 1;
            }
            (s0, s1, s2)
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let (mut s0, mut s1, mut s2) = (0.0f32, 0.0, 0.0);
        for i in 0..n {
            let c = cov[i];
            s0 += c * w0[i];
            s1 += c * w1[i];
            s2 += c * w2[i];
        }
        (s0, s1, s2)
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn hsum(v: core::arch::x86_64::__m128) -> f32 {
    use core::arch::x86_64::*;
    unsafe {
        let t = _mm_add_ps(v, _mm_movehl_ps(v, v));
        let t = _mm_add_ss(t, _mm_shuffle_ps(t, t, 0x55));
        _mm_cvtss_f32(t)
    }
}
