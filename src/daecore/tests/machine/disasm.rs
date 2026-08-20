#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn daegun_probe_atan2(y: f64, x: f64) -> f64 {
    daegun::daecore::daemachine::float::atan2(y, x)
}

#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn daegun_probe_sin_cos(v: f64) -> f64 {
    let (s, c) = daegun::daecore::daemachine::float::sin_cos(v);
    s + c
}

#[test]
fn probes_are_reachable() {
    assert!((daegun_probe_atan2(1.0, 1.0) - core::f64::consts::FRAC_PI_4).abs() < 1e-15);
    assert!(daegun_probe_sin_cos(0.0) - 1.0 == 0.0);
}

#[test]
#[ignore]
fn atan2_instruction_load() {
    let mut acc = 0.0f64;
    let mut x = 0.5f64;
    for i in 0..4_000_000u32 {
        x = if i % 3 == 0 { x * 1.0000037 } else { x * 0.9999971 };
        acc += daegun_probe_atan2(x, 1.0) + daegun_probe_atan2(1.0, x);
    }
    assert!(acc.is_finite(), "the loop produced nothing usable");
    core::hint::black_box(acc);
}
