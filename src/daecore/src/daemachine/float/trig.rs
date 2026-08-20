#[cfg(all(not(feature = "std"), not(test)))]
use super::FloatExt;

pub fn sin_cos(radians: f64) -> (f64, f64) {
    if !radians.is_finite() {
        return (0.0, 1.0);
    }

    let n = (radians / core::f64::consts::FRAC_PI_2).round_ties_even();
    let x = radians - n * core::f64::consts::FRAC_PI_2;
    let (sin, cos) = (kernel_sin(x), kernel_cos(x));

    match (n as i64).rem_euclid(4) {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

#[rustfmt::skip]
fn kernel_sin(x: f64) -> f64 {
    const S1: f64 = -0.16666666666666632;
    const S2: f64 = 0.00833333333332249;
    const S3: f64 = -0.0001984126982985795;
    const S4: f64 = 2.7557313707070068e-06;
    const S5: f64 = -2.5050760253406863e-08;
    const S6: f64 = 1.58969099521155e-10;

    let z = x * x;
    x + x * z * (S1 + z * (S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)))))
}

#[rustfmt::skip]
fn kernel_cos(x: f64) -> f64 {
    const C1: f64 = 0.0416666666666666;
    const C2: f64 = -0.001388888888887411;
    const C3: f64 = 2.480158728947673e-05;
    const C4: f64 = -2.7557314351390663e-07;
    const C5: f64 = 2.087572321298175e-09;
    const C6: f64 = -1.1359647557788195e-11;

    let z = x * x;
    1.0 - 0.5 * z + z * z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))))
}

pub fn atan2(y: f64, x: f64) -> f64 {
    const PI: f64 = core::f64::consts::PI;

    if !(y.is_finite() && x.is_finite() && y != 0.0 && x != 0.0) {
        return atan2_special(y, x);
    }

    let a = atan((y / x).abs());
    match (x > 0.0, y > 0.0) {
        (true, true) => a,
        (true, false) => -a,
        (false, true) => PI - a,
        (false, false) => a - PI,
    }
}

#[cold]
#[inline(never)]
fn atan2_special(y: f64, x: f64) -> f64 {
    const PI: f64 = core::f64::consts::PI;
    const FRAC_PI_2: f64 = core::f64::consts::FRAC_PI_2;
    const FRAC_PI_4: f64 = core::f64::consts::FRAC_PI_4;

    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        return if x.is_sign_negative() {
            if y.is_sign_negative() { -PI } else { PI }
        } else {
            y
        };
    }
    if x == 0.0 {
        return if y < 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 };
    }
    if y.is_infinite() {
        let a = if x.is_infinite() {
            if x > 0.0 { FRAC_PI_4 } else { 3.0 * FRAC_PI_4 }
        } else {
            FRAC_PI_2
        };
        return if y < 0.0 { -a } else { a };
    }
    if x.is_infinite() {
        return if x > 0.0 { if y < 0.0 { -0.0 } else { 0.0 } } else if y < 0.0 { -PI } else { PI };
    }
    let a = atan((y / x).abs());
    match (x > 0.0, y > 0.0) {
        (true, true) => a,
        (true, false) => -a,
        (false, true) => PI - a,
        (false, false) => a - PI,
    }
}

#[rustfmt::skip]
fn atan(x: f64) -> f64 {
    const AT: [f64; 11] = [
        0.3333333333333293, -0.19999999999876483, 0.14285714272503466, -0.11111110405462356,
        0.09090887133436507, -0.0769187620504483, 0.06661073137387531, -0.058335701337905735,
        0.049768779946159324, -0.036531572744216916, 0.016285820115365782,
    ];
    const HI: [f64; 4] = [
        0.4636476090008061,
        core::f64::consts::FRAC_PI_4,
        0.982793723247329,
        core::f64::consts::FRAC_PI_2,
    ];
    const LO: [f64; 4] = [
        2.2698777452961687e-17, 3.061616997868383e-17, 1.3903311031230998e-17, 6.123233995736766e-17,
    ];

    let negative = x.is_sign_negative();
    let ax = x.abs();
    if ax >= 1.0e300 {
        let r = HI[3] + LO[3];
        return if negative { -r } else { r };
    }

    let (k, t) = if ax < 0.4375 {
        (-1i32, ax)
    } else if ax < 0.6875 {
        (0, (2.0 * ax - 1.0) / (2.0 + ax))
    } else if ax < 1.1875 {
        (1, (ax - 1.0) / (ax + 1.0))
    } else if ax < 2.4375 {
        (2, (ax - 1.5) / (1.0 + 1.5 * ax))
    } else {
        (3, -1.0 / ax)
    };

    let z = t * t;
    let w = z * z;
    let odd  = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let even = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));

    let r = if k < 0 {
        t - t * (odd + even)
    } else {
        let i = k as usize;
        HI[i] - ((t * (odd + even) - LO[i]) - t)
    };
    if negative { -r } else { r }
}
