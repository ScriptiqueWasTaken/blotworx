//! Arithmetic port of DOL atan 0x80023bc0 and atan2 0x800206ec.
//! Constants are exact words from 0x8064d8c8 and 0x806fdd20.
//! Do not contract the separate multiply/add operations into FMAs.
//! FPSCR exceptions and NaN payload propagation still need native validation.

/// DOL 80021c04..80021e64, small/medium argument reduction. The original
/// large-exponent multiword path is not implemented here and fails explicitly.
pub fn reduce_pio2_medium(x: f64) -> anyhow::Result<(i32, [f64; 2])> {
    let high = ((x.to_bits() >> 32) as u32) & 0x7fffffff;
    anyhow::ensure!(
        high <= 0x413921fb,
        "Large-angle native reduction is not implemented"
    );
    if high <= 0x3fe921fb {
        return Ok((0, [x, 0.]));
    }
    let p1 = f64::from_bits(0x3ff921fb54400000);
    let t1 = f64::from_bits(0x3dd0b4611a626331);
    let p2 = f64::from_bits(0x3dd0b4611a600000);
    let t2 = f64::from_bits(0x3ba3198a2e037073);
    if high < 0x4002d97c {
        if x > 0. {
            let mut r = x - p1;
            let tail = if high == 0x3ff921fb {
                r -= p2;
                t2
            } else {
                t1
            };
            let hi = r - tail;
            return Ok((1, [hi, (r - hi) - tail]));
        } else {
            let mut r = x + p1;
            let tail = if high == 0x3ff921fb {
                r += p2;
                t2
            } else {
                t1
            };
            let hi = tail + r;
            return Ok((-1, [hi, tail + (r - hi)]));
        }
    }
    const HIGH: [u32; 32] = [
        0x3ff921fb, 0x400921fb, 0x4012d97c, 0x401921fb, 0x401f6a7a, 0x4022d97c, 0x4025fdbb,
        0x402921fb, 0x402c463a, 0x402f6a7a, 0x4031475c, 0x4032d97c, 0x40346b9c, 0x4035fdbb,
        0x40378fdb, 0x403921fb, 0x403ab41b, 0x403c463a, 0x403dd85a, 0x403f6a7a, 0x40407e4c,
        0x4041475c, 0x4042106c, 0x4042d97c, 0x4043a28c, 0x40446b9c, 0x404534ac, 0x4045fdbb,
        0x4046c6cb, 0x40478fdb, 0x404858eb, 0x404921fb,
    ];
    let n = (0.5 + f64::from_bits(0x3fe45f306dc9c883) * x.abs()) as i32;
    let count = n as f64;
    let mut r = x.abs() - p1 * count;
    let mut w = t1 * count;
    let mut hi = r - w;
    if n >= 32 || high == HIGH[n as usize - 1] {
        let exponent = (high >> 20) as i32;
        let loss = |value: f64| exponent - (((value.to_bits() >> 52) & 0x7ff) as i32);
        if loss(hi) > 16 {
            let old = r;
            let product = p2 * count;
            r -= product;
            w = t2 * count - ((old - r) - product);
            hi = r - w;
            if loss(hi) > 49 {
                let old = r;
                let product = f64::from_bits(0x3ba3198a2e000000) * count;
                r -= product;
                w = f64::from_bits(0x397b839a252049c1) * count - ((old - r) - product);
                hi = r - w;
            }
        }
    }
    let lo = (r - hi) - w;
    Ok(if x < 0. {
        (-n, [-hi, -lo])
    } else {
        (n, [hi, lo])
    })
}

/// Original sine/cosine quadrant dispatch for supported reduction range.
/// No host trig fallback for the still-unimplemented large-angle path.
pub fn sin_cos_medium(x: f64) -> anyhow::Result<(f64, f64)> {
    if ((x.to_bits() >> 32) & 0x7fffffff) <= 0x3fe921fb {
        return Ok((kernel_sin(x, 0., false), kernel_cos(x, 0.)));
    }
    let (n, [hi, lo]) = reduce_pio2_medium(x)?;
    let s = kernel_sin(hi, lo, true);
    let c = kernel_cos(hi, lo);
    Ok(match n & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    })
}

/// Original tan80024860 and kernel80023970. Same explicit small/medium
/// reduction boundary as sin/cos; no host trigonometry fallback.
pub fn tan_medium(x: f64) -> anyhow::Result<f64> {
    anyhow::ensure!(x.is_finite(), "Nonfinite native tan input");
    if ((x.to_bits() >> 32) & 0x7fffffff) <= 0x3fe921fb {
        return Ok(kernel_tan(x, 0., 1));
    }
    let (n, [hi, lo]) = reduce_pio2_medium(x)?;
    Ok(kernel_tan(hi, lo, 1 - ((n & 1) << 1)))
}
pub fn kernel_tan(mut x: f64, mut y: f64, mode: i32) -> f64 {
    const T: [f64; 13] = [
        f64::from_bits(0x3fd5555555555563),
        f64::from_bits(0x3fc111111110fe7a),
        f64::from_bits(0x3faba1ba1bb341fe),
        f64::from_bits(0x3f9664f48406d637),
        f64::from_bits(0x3f8226e3e96e8493),
        f64::from_bits(0x3f6d6d22c9560328),
        f64::from_bits(0x3f57dbc8fee08315),
        f64::from_bits(0x3f4344d8f2f26501),
        f64::from_bits(0x3f3026f71a8d1068),
        f64::from_bits(0x3f147e88a03792a6),
        f64::from_bits(0x3f12b80f32f0a7e9),
        f64::from_bits(0xbef375cbdb605373),
        f64::from_bits(0x3efb2a7074bf7ad4),
    ];
    let bits = x.to_bits();
    let high = ((bits >> 32) as u32) & 0x7fffffff;
    if high < 0x3e300000 {
        if x == 0. && mode == -1 {
            return 1. / x.abs();
        }
        return if mode == 1 { x } else { -1. / x };
    }
    if high >= 0x3fe59428 {
        if bits >> 63 != 0 {
            x = -x;
            y = -y;
        }
        x = (f64::from_bits(0x3fe921fb54442d18) - x) + (f64::from_bits(0x3c81a62633145c07) - y);
        y = 0.;
    }
    let z = x * x;
    let w = z * z;
    let s = z * x;
    let odd = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let even = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let r = y + s * (odd + even);
    let r = (y + z * r) + T[0] * s;
    let w = x + r;
    if high >= 0x3fe59428 {
        let v = mode as f64;
        let sign = if bits >> 63 != 0 { -1. } else { 1. };
        return sign * (v - 2. * (x - ((w * w / (w + v)) - r)));
    }
    if mode == 1 {
        return w;
    }
    let truncated = f64::from_bits(w.to_bits() & 0xffffffff00000000);
    let residual = r - (truncated - x);
    let inverse = -1. / w;
    let coarse = f64::from_bits(inverse.to_bits() & 0xffffffff00000000);
    coarse + inverse * ((1. + coarse * truncated) + coarse * residual)
}

/// Cosine kernel, 800220e8. Inputs are the reduced high/low angle pair;
/// full-range reduction belongs to 80021c04. All operations are separate f64
/// instructions, not fused. Constants read with original r2=80705b60.
pub fn kernel_cos(x: f64, y: f64) -> f64 {
    let high = ((x.to_bits() >> 32) as u32) & 0x7fffffff;
    if high < 0x3e400000 {
        return 1.;
    }
    let z = x * x;
    let mut r = f64::from_bits(0xbda8fae9be8838d4) * z;
    r = f64::from_bits(0x3e21ee9ebdb4b1c4) + r;
    r = z * r;
    r = f64::from_bits(0xbe927e4f809c52ad) + r;
    r = z * r;
    r = f64::from_bits(0x3efa01a019cb1590) + r;
    r = z * r;
    r = f64::from_bits(0xbf56c16c16c15177) + r;
    r = z * r;
    r = f64::from_bits(0x3fa555555555554c) + r;
    r = z * r;
    if high < 0x3fd33333 {
        return 1. - (0.5 * z - (z * r - x * y));
    }
    let q = if high > 0x3fe90000 {
        0.28125
    } else {
        f64::from_bits(((high - 0x00200000) as u64) << 32)
    };
    let hz = 0.5 * z - q;
    let a = 1. - q;
    a - (hz - (z * r - x * y))
}

/// Sine kernel, 800238b0. `tail` matches native integer argument r3; when
/// false the low residual y is ignored. Does not perform argument reduction.
pub fn kernel_sin(x: f64, y: f64, tail: bool) -> f64 {
    let high = ((x.to_bits() >> 32) as u32) & 0x7fffffff;
    if high < 0x3e400000 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let mut r = f64::from_bits(0x3de5d93a5acfd57c) * z;
    r = f64::from_bits(0xbe5ae5e68a2b9ceb) + r;
    r = z * r;
    r = f64::from_bits(0x3ec71de357b1fe7d) + r;
    r = z * r;
    r = f64::from_bits(0xbf2a01a019c161d5) + r;
    r = z * r;
    r = f64::from_bits(0x3f8111111110f8a6) + r;
    let s1 = f64::from_bits(0xbfc5555555555549);
    if !tail {
        x + v * (s1 + z * r)
    } else {
        x - ((z * (0.5 * y - v * r) - y) - s1 * v)
    }
}

/// Havok's distinct float approximation, DOL 8014eba4..8014ec6c.
/// This is not the double libc atan2 below. Keep rounded products and the two
/// explicit fused operations; signed zero does not take negative branches.
pub fn havok_atan2(y: f32, x: f32) -> anyhow::Result<f32> {
    anyhow::ensure!(
        x.is_finite() && y.is_finite(),
        "Nonfinite Havok atan2 input"
    );
    let (ay, ax) = (y.abs(), x.abs());
    let small_y = ay <= ax;
    let ratio = if small_y {
        ay / (ax + f32::from_bits(0x34000000))
    } else {
        ax / (ay + f32::from_bits(0x34000000))
    };
    let linear = f32::from_bits(0xbdf7f844) * ratio;
    let cubic = f32::from_bits(0xbdbf88e4) * ratio;
    let base = ratio.mul_add(linear, ratio);
    let cubic = ratio * cubic;
    let approximation = ratio.mul_add(cubic, base);
    let mut result = if small_y {
        approximation
    } else {
        f32::from_bits(0x3fc90fdb) - approximation
    };
    if x < 0. {
        result = f32::from_bits(0x40490fdb) - result;
    }
    if y < 0. {
        result = -result;
    }
    Ok(result)
}
const ATAN: [f64; 19] = [
    f64::from_bits(0x3fddac670561bb4f),
    f64::from_bits(0x3fe921fb54442d18),
    f64::from_bits(0x3fef730bd281f69b),
    f64::from_bits(0x3ff921fb54442d18),
    f64::from_bits(0x3c7a2b7f222f65e2),
    f64::from_bits(0x3c81a62633145c07),
    f64::from_bits(0x3c7007887af0cbbd),
    f64::from_bits(0x3c91a62633145c07),
    f64::from_bits(0x3fd555555555550d),
    f64::from_bits(0xbfc999999998ebc4),
    f64::from_bits(0x3fc24924920083ff),
    f64::from_bits(0xbfbc71c6fe231671),
    f64::from_bits(0x3fb745cdc54c206e),
    f64::from_bits(0xbfb3b0f2af749a6d),
    f64::from_bits(0x3fb10d66a0d03d51),
    f64::from_bits(0xbfadde2d52defd9a),
    f64::from_bits(0x3fa97b4b24760deb),
    f64::from_bits(0xbfa2b4442c6a6c2f),
    f64::from_bits(0x3f90ad3ae322da11),
];
const QUADRANT: [f64; 11] = [
    f64::from_bits(0x400921fb54442d18),
    f64::from_bits(0xc00921fb54442d18),
    f64::from_bits(0xbff921fb54442d18),
    f64::from_bits(0x3ff921fb54442d18),
    f64::from_bits(0x3fe921fb54442d18),
    f64::from_bits(0xbfe921fb54442d18),
    f64::from_bits(0x4002d97c7f3321d2),
    f64::from_bits(0xc002d97c7f3321d2),
    f64::from_bits(0x0000000000000000),
    f64::from_bits(0x8000000000000000),
    f64::from_bits(0x3ca1a62633145c07),
];

/// DOL 0x800249d0 -> 0x80020188. Separate double operations are intentional.
/// The linked square-root helper rounds its input and result to single precision.
/// Floating-point exception flags and the original errno side effect are not modeled.
pub fn acos(x: f64) -> f64 {
    const P: [f64; 6] = [
        f64::from_bits(0x3fc5555555555555),
        f64::from_bits(0xbfd4d61203eb6f7d),
        f64::from_bits(0x3fc9c1550e884455),
        f64::from_bits(0xbfa48228b5688f3b),
        f64::from_bits(0x3f49efe07501b288),
        f64::from_bits(0x3f023de10dfdf709),
    ];
    const Q: [f64; 4] = [
        f64::from_bits(0xc0033a271c8a2d4b),
        f64::from_bits(0x40002ae59c598ac8),
        f64::from_bits(0xbfe6066c1b8d0159),
        f64::from_bits(0x3fb3b8c5b12e9282),
    ];
    const LOW: f64 = f64::from_bits(0x3c91a62633145c07);
    let high = (x.to_bits() >> 32) as u32;
    let magnitude = high & 0x7fffffff;
    if magnitude >= 0x3ff00000 {
        if x == 1. {
            return 0.;
        }
        if x == -1. {
            return QUADRANT[0];
        }
        return f64::NAN;
    }
    let rational = |z: f64| {
        let p = z * (P[0] + z * (P[1] + z * (P[2] + z * (P[3] + z * (P[4] + z * P[5])))));
        let q = 1. + z * (Q[0] + z * (Q[1] + z * (Q[2] + z * Q[3])));
        p / q
    };
    if magnitude < 0x3fe00000 {
        if magnitude <= 0x3c600000 {
            return QUADRANT[3];
        }
        return QUADRANT[3] - (x - (LOW - x * rational(x * x)));
    }
    if high & 0x80000000 != 0 {
        let z = 0.5 * (1. + x);
        let root = super::cursor::native_sqrt(z as f32) as f64;
        QUADRANT[0] - 2. * (root + (rational(z) * root - LOW))
    } else {
        let z = 0.5 * (1. - x);
        let root = super::cursor::native_sqrt(z as f32) as f64;
        let high_root = f64::from_bits(root.to_bits() & 0xffffffff00000000);
        let correction = (z - high_root * high_root) / (root + high_root);
        2. * (high_root + (correction + rational(z) * root))
    }
}

/// DOL800249d4 ->80020454. Separate double operations and the single-
/// precision square-root helper are intentional; errno/FPSCR are not modeled.
pub fn asin(x: f64) -> f64 {
    const P: [f64; 6] = [
        f64::from_bits(0x3fc5555555555555),
        f64::from_bits(0xbfd4d61203eb6f7d),
        f64::from_bits(0x3fc9c1550e884455),
        f64::from_bits(0xbfa48228b5688f3b),
        f64::from_bits(0x3f49efe07501b288),
        f64::from_bits(0x3f023de10dfdf709),
    ];
    const Q: [f64; 4] = [
        f64::from_bits(0xc0033a271c8a2d4b),
        f64::from_bits(0x40002ae59c598ac8),
        f64::from_bits(0xbfe6066c1b8d0159),
        f64::from_bits(0x3fb3b8c5b12e9282),
    ];
    const HI: f64 = f64::from_bits(0x3ff921fb54442d18);
    const LO: f64 = f64::from_bits(0x3c91a62633145c07);
    const QUARTER: f64 = f64::from_bits(0x3fe921fb54442d18);
    let magnitude = ((x.to_bits() >> 32) as u32) & 0x7fffffff;
    if magnitude >= 0x3ff00000 {
        return if x.abs() == 1. {
            (HI * x) + (LO * x)
        } else {
            f64::NAN
        };
    }
    let rational = |z: f64| {
        let p = z * (P[0] + z * (P[1] + z * (P[2] + z * (P[3] + z * (P[4] + z * P[5])))));
        let q = 1. + z * (Q[0] + z * (Q[1] + z * (Q[2] + z * Q[3])));
        p / q
    };
    if magnitude < 0x3fe00000 {
        if magnitude < 0x3e400000 {
            return x;
        }
        return x + x * rational(x * x);
    }
    let z = (1. - x.abs()) * 0.5;
    let root = super::cursor::native_sqrt(z as f32) as f64;
    let ratio = rational(z);
    let value = if magnitude >= 0x3fef3333 {
        HI - (2. * (root + root * ratio) - LO)
    } else {
        let high = f64::from_bits(root.to_bits() & 0xffffffff00000000);
        let correction = (z - high * high) / (root + high);
        let p = (2. * root) * ratio - (LO - 2. * correction);
        QUARTER - (p - (QUARTER - 2. * high))
    };
    if x > 0. {
        value
    } else {
        -value
    }
}

pub fn atan(mut x: f64) -> f64 {
    let bits = x.to_bits();
    let high = (bits >> 32) as u32;
    let magnitude = high & 0x7fffffff;
    if magnitude >= 0x44100000 {
        if x.is_nan() {
            return x + x;
        }
        return if high & 0x80000000 == 0 {
            ATAN[3] + ATAN[7]
        } else {
            -ATAN[3] - ATAN[7]
        };
    }
    let region;
    if magnitude < 0x3fdc0000 {
        if magnitude < 0x3e200000 {
            return x;
        }
        region = None;
    } else {
        x = x.abs();
        if magnitude < 0x3ff30000 {
            if magnitude < 0x3fe60000 {
                region = Some(0);
                x = (2. * x - 1.) / (2. + x);
            } else {
                region = Some(1);
                x = (x - 1.) / (1. + x);
            }
        } else if magnitude < 0x40038000 {
            region = Some(2);
            x = (x - 1.5) / (1. + 1.5 * x);
        } else {
            region = Some(3);
            x = -1. / x;
        }
    }
    let a = &ATAN[8..];
    let z = x * x;
    let w = z * z;
    let even = z * (a[0] + w * (a[2] + w * (a[4] + w * (a[6] + w * (a[8] + w * a[10])))));
    let odd = w * (a[1] + w * (a[3] + w * (a[5] + w * (a[7] + w * a[9]))));
    match region {
        None => x - x * (even + odd),
        Some(i) => {
            let value = ATAN[i] - ((x * (even + odd) - ATAN[4 + i]) - x);
            if high & 0x80000000 == 0 {
                value
            } else {
                -value
            }
        }
    }
}

pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    if x.to_bits() == 1f64.to_bits() {
        return atan(y);
    }
    let hx = (x.to_bits() >> 32) as u32;
    let hy = (y.to_bits() >> 32) as u32;
    let ix = hx & 0x7fffffff;
    let iy = hy & 0x7fffffff;
    let quadrant = ((hx >> 30) & 2) | ((hy >> 31) & 1);
    if y == 0. {
        return match quadrant {
            0 | 1 => y,
            2 => QUADRANT[0],
            _ => QUADRANT[1],
        };
    }
    if x == 0. {
        return if hy & 0x80000000 != 0 {
            QUADRANT[2]
        } else {
            QUADRANT[3]
        };
    }
    if x.is_infinite() {
        return if y.is_infinite() {
            QUADRANT[4 + quadrant as usize]
        } else {
            match quadrant {
                0 => QUADRANT[8],
                1 => QUADRANT[9],
                2 => QUADRANT[0],
                _ => QUADRANT[1],
            }
        };
    }
    if y.is_infinite() {
        return if hy & 0x80000000 != 0 {
            QUADRANT[2]
        } else {
            QUADRANT[3]
        };
    }
    let exponent_delta = (iy as i32 - ix as i32) >> 20;
    let z = if exponent_delta > 60 {
        QUADRANT[3]
    } else if hx & 0x80000000 != 0 && exponent_delta < -60 {
        0.
    } else {
        atan((y / x).abs())
    };
    match quadrant {
        0 => z,
        1 => f64::from_bits(z.to_bits() ^ 0x8000000000000000),
        2 => QUADRANT[0] - (z - QUADRANT[10]),
        _ => (z - QUADRANT[10]) - QUADRANT[0],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn medium_reduction_and_quadrants_cover_table_angles_and_cancellation() -> anyhow::Result<()> {
        for x in [
            0.,
            -0.,
            0.8,
            -0.8,
            1.,
            -1.,
            std::f64::consts::FRAC_PI_2,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            31. * std::f64::consts::FRAC_PI_2,
            32. * std::f64::consts::FRAC_PI_2,
            1000.,
            1e6,
            -1e6,
        ] {
            let (n, [hi, lo]) = super::reduce_pio2_medium(x)?;
            assert!(
                hi.abs() <= 0.786 && lo.abs() <= 1e-16,
                "Bad reduced pair {x}: {n}/{hi}/{lo}"
            );
            let (s, c) = super::sin_cos_medium(x)?;
            // Numerical sanity only; native runtime bit comparison is separate.
            assert!((s - x.sin()).abs() <= 2e-16, "sin {x}: {s}");
            assert!((c - x.cos()).abs() <= 2e-16, "cos {x}: {c}");
        }
        assert_eq!(super::sin_cos_medium(-0.)?.0.to_bits(), (-0f64).to_bits());
        assert!(super::sin_cos_medium(1e20).is_err());
        assert!(super::sin_cos_medium(f64::NAN).is_err());
        Ok(())
    }
    #[test]
    fn reduced_trig_kernels_preserve_zero_and_residual_paths() {
        assert_eq!(
            super::kernel_sin(-0., 0., false).to_bits(),
            (-0f64).to_bits()
        );
        assert_eq!(super::kernel_cos(-0., 0.).to_bits(), 1f64.to_bits());
        let tiny = 2f64.powi(-30);
        assert_eq!(super::kernel_sin(tiny, 0., false), tiny);
        assert_eq!(super::kernel_cos(tiny, 0.), 1.);
        // Host trig is only an independent numerical sanity check here,
        // not evidence of bit parity with original Wii runtime execution.
        let mut residual_observed = false;
        for x in [-0.78f64, -0.5, -0.3, 0.3, 0.5, 0.78] {
            assert!((super::kernel_sin(x, 0., false) - x.sin()).abs() <= 2e-16);
            assert!((super::kernel_cos(x, 0.) - x.cos()).abs() <= 2e-16);
            // A reduced low residual is at most about one high-part ULP;
            // the kernel is not a general sin(x+y) implementation.
            let y = 2f64.powi(-54);
            assert!((super::kernel_sin(x, y, true) - (x + y).sin()).abs() <= 2e-16);
            assert!((super::kernel_cos(x, y) - (x + y).cos()).abs() <= 2e-16);
            residual_observed |= super::kernel_sin(x, y, true) != super::kernel_sin(x, y, false);
        }
        assert!(residual_observed);
    }
    use super::*;
    #[test]
    fn acos_endpoints_branches_and_single_precision_root() {
        assert_eq!(acos(1.).to_bits(), 0);
        assert_eq!(acos(-1.), QUADRANT[0]);
        assert_eq!(acos(0.), QUADRANT[3]);
        assert!(acos(1.01).is_nan());
        assert!(acos(f64::NAN).is_nan());
        let mut previous = QUADRANT[0];
        for i in 0..=2000 {
            let x = -1. + i as f64 / 1000.;
            let actual = acos(x);
            assert!(actual <= previous);
            previous = actual;
            // Sanity bound only, not an original-execution parity tolerance.
            assert!((actual - x.acos()).abs() < 4e-7, "{x}: {actual}");
        }
        assert_ne!(acos(-0.7).to_bits(), (-0.7f64).acos().to_bits());
    }
    #[test]
    fn axes_signed_zero_and_infinite_quadrants() {
        for (y, x, expected) in [
            (0., -1., QUADRANT[0]),
            (-0., -1., QUADRANT[1]),
            (-0., 1., -0.),
            (-1., 0., QUADRANT[2]),
            (1., 0., QUADRANT[3]),
            (1., 1., QUADRANT[4]),
            (-1., 1., QUADRANT[5]),
        ] {
            assert_eq!(atan2(y, x).to_bits(), expected.to_bits());
        }
        for (i, (y, x)) in [(1., 1.), (-1., 1.), (1., -1.), (-1., -1.)]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                atan2(y * f64::INFINITY, x * f64::INFINITY).to_bits(),
                QUADRANT[4 + i].to_bits()
            );
        }
        assert_eq!(atan2(-1., f64::INFINITY).to_bits(), (-0f64).to_bits());
        assert!(atan2(f64::NAN, 1.).is_nan());
    }
    #[test]
    fn kernel_range_reductions_and_extreme_ratios() {
        // Host math is only an independent numerical smoke check, not the
        // original-game bitwise oracle. Cover both sides of every reduction.
        for x in [
            0., 1e-20, 0.437499, 0.4375, 0.687499, 0.6875, 1., 1.187499, 1.1875, 2.437499, 2.4375,
            1e30,
        ] {
            for sign in [-1., 1.] {
                let x: f64 = x * sign;
                assert!((atan(x) - x.atan()).abs() < 4e-16, "{x}");
            }
        }
        assert_eq!(atan2(1e100, 1e-100), QUADRANT[3]);
        assert_eq!(atan2(1e-100, -1e100), QUADRANT[0]);
    }
}
