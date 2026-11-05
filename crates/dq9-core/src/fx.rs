//! Fixed-point arithmetic of the game (`fx32`: 20.12, 4096 = 1.0) as the SDK routines and the
//! game's own helpers compute it, rounding included: the movement and collision modules rely
//! on every result matching the hardware divider and square-root unit bit for bit
//! (`docs/re/movement.md`, « Arithmétique »).
//!
//! port: 0x020c2c04, 0x020c2c14, 0x020c2da0, 0x020c2dd4, 0x020c2e08, 0x020c2e44, 0x020c2ec8,
//! port: 0x020c2f28, 0x020c3040, 0x020c339c, 0x02030ce8, 0x02030d34, 0x02030e3c

use std::sync::LazyLock;

pub use dq9_formats::map::{fix32_cos, fix32_sin, reduce_angle};

/// A fixed-point vector.
pub type Fx3 = [i32; 3];

/// One half turn (`FIX_PI`).
pub const PI: i32 = 0x3244;
/// One turn (`FIX_2PI`).
pub const TWO_PI: i32 = 0x6488;

/// `FX_Div` (`0x020c2c04`): `a / b` rounded to `fx32`, through the hardware divider
/// (`(a << 32) / b`, truncating, then `+ 0x80000 >> 20`). A zero divisor gives 0, the
/// divider's `±1` result rounded.
pub fn div(a: i32, b: i32) -> i32 {
    if b == 0 {
        return 0;
    }
    let quotient = (i64::from(a) << 32).wrapping_div(i64::from(b));
    (quotient.wrapping_add(0x80000) >> 20) as i32
}

/// The product `(a * b + 0x800) >> 12` that `Vector3fixMultiplyScalar` (`0x02030e3c`) and the
/// vector routines apply to each component, truncated to 32 bits like the game.
pub fn mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b) + 0x800) >> 12) as i32
}

/// `fix32abs` (`0x02030d34`).
pub fn abs(a: i32) -> i32 {
    if a < 0 { a.wrapping_neg() } else { a }
}

/// `fix32SignedAngleDistance` (`0x02030ce8`): the signed angle from `a` to `b`, in
/// `[-π, π]`.
pub fn signed_angle_distance(a: i32, b: i32) -> i32 {
    let mut d = 0;
    if a < b {
        d = b - a;
        if d > PI {
            d -= TWO_PI;
        }
    } else if b < a {
        d = -(a - b);
        if d < -PI {
            d += TWO_PI;
        }
    }
    d
}

/// `VEC_Add` (`0x020c2da0`).
pub fn add(a: Fx3, b: Fx3) -> Fx3 {
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
    ]
}

/// `VEC_Subtract` (`0x020c2dd4`).
pub fn sub(a: Fx3, b: Fx3) -> Fx3 {
    [
        a[0].wrapping_sub(b[0]),
        a[1].wrapping_sub(b[1]),
        a[2].wrapping_sub(b[2]),
    ]
}

/// `Vector3fixMultiplyScalar` (`0x02030e3c`).
pub fn scale(v: Fx3, s: i32) -> Fx3 {
    v.map(|c| mul(c, s))
}

/// `VEC_DotProduct` (`0x020c2e08`): one rounding for the whole sum.
pub fn dot(a: Fx3, b: Fx3) -> i32 {
    let sum = i64::from(a[0]) * i64::from(b[0])
        + i64::from(a[1]) * i64::from(b[1])
        + i64::from(a[2]) * i64::from(b[2])
        + 0x800;
    (sum >> 12) as i32
}

/// `VEC_CrossProduct` (`0x020c2e44`).
pub fn cross(a: Fx3, b: Fx3) -> Fx3 {
    let c = |p: i32, q: i32, r: i32, s: i32| {
        ((i64::from(p) * i64::from(q) - i64::from(r) * i64::from(s) + 0x800) >> 12) as i32
    };
    [
        c(a[1], b[2], a[2], b[1]),
        c(a[2], b[0], a[0], b[2]),
        c(a[0], b[1], a[1], b[0]),
    ]
}

/// Sum of squares in the 64 bits the hardware square-root unit receives.
fn square_sum(v: Fx3) -> u64 {
    v.iter()
        .map(|&c| i64::from(c).wrapping_mul(i64::from(c)))
        .fold(0i64, i64::wrapping_add) as u64
}

/// The hardware square root of a 64-bit value: `floor(sqrt(n))`.
fn isqrt(n: u64) -> u32 {
    let mut root = (n as f64).sqrt() as u64;
    while root.saturating_mul(root) > n {
        root -= 1;
    }
    while (root + 1).saturating_mul(root + 1) <= n {
        root += 1;
    }
    root as u32
}

/// `VEC_Mag` (`0x020c2ec8`): the length, rounded.
pub fn mag(v: Fx3) -> i32 {
    (isqrt(square_sum(v).wrapping_mul(4)) as i32).wrapping_add(1) >> 1
}

/// `VEC_Distance` (`0x020c3040`).
pub fn distance(a: Fx3, b: Fx3) -> i32 {
    mag(sub(a, b))
}

/// `VEC_Normalize` (`0x020c2f28`): the divider and square-root unit run side by side, then the
/// components are scaled by `2^56 / |v|²` times the root, with the hardware's truncations.
/// The zero vector stays zero.
pub fn normalize(v: Fx3) -> Fx3 {
    let sum = square_sum(v) as i64;
    if sum == 0 {
        return [0; 3];
    }
    let root = i64::from(isqrt(sum.wrapping_mul(4) as u64) as i32);
    let reciprocal = (1i64 << 56).wrapping_div(sum);
    let factor = root.wrapping_mul(reciprocal);
    v.map(|c| {
        let high = ((i128::from(factor) * i128::from(c)) >> 32) as i32;
        high.wrapping_add(0x1000) >> 13
    })
}

/// `FX_Sqrt` (`0x020c2c14`): the square root of an `fx32`, rounded; a value below 1 is
/// returned unchanged (the routine leaves its argument in `r0`).
pub fn sqrt(a: i32) -> i32 {
    if a < 1 {
        return a;
    }
    let root = isqrt(u64::from(a as u32) << 32);
    (root.wrapping_add(0x200) >> 10) as i32
}

/// The entries of the arctangent table at `0x020ed460`: `round(4096 · atan(i / 128))`,
/// `i` in `0..=128` (equal to the ROM's, checked by the real-ROM test).
pub fn atan_entry(i: usize) -> i16 {
    static TABLE: LazyLock<[i16; 129]> = LazyLock::new(|| {
        let mut table = [0i16; 129];
        for (i, entry) in table.iter_mut().enumerate() {
            *entry = ((i as f64 / 128.0).atan() * 4096.0).round() as i16;
        }
        table
    });
    TABLE[i]
}

/// `FX_Atan2` (`0x020c339c`): the angle in `(-π, π]` of the vector `(x, y)` measured from the
/// `y` axis towards `x`: `atan2(a, b)` with `a` the sine-like component. The ratio of the
/// smaller to the larger component indexes the table by steps of 1/128.
pub fn atan2(a: i32, b: i32) -> i32 {
    // Each case is an angle `base` plus (`rising`) or minus the arctangent of `num / den`.
    let (base, rising, num, den) = if a < 1 {
        if a > -1 {
            return if b < 0 { PI } else { 0 };
        }
        let na = -a;
        if b < 0 {
            let nb = -b;
            if a == b || nb < na {
                if na <= nb {
                    return -0x25b3;
                }
                (-0x1922, false, nb, na)
            } else {
                (-PI, true, na, nb)
            }
        } else if b < 1 {
            return -0x1922;
        } else if b < na {
            (-0x1922, true, b, na)
        } else if b <= na {
            return -0xc91;
        } else {
            (0, false, na, b)
        }
    } else if b < 1 {
        if b > -1 {
            return 0x1922;
        }
        let nb = -b;
        if nb < a {
            (0x1922, true, nb, a)
        } else if -a == b {
            return 0x25b3;
        } else {
            (PI, false, a, nb)
        }
    } else if a < b {
        (0, true, a, b)
    } else if a <= b {
        return 0xc91;
    } else {
        (0x1922, false, b, a)
    };
    if den == 0 {
        return 0;
    }
    let entry = i32::from(atan_entry((div(num, den) >> 5) as usize));
    let angle = if rising { base + entry } else { base - entry };
    i32::from(angle as i16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_rounds_and_truncates_like_the_hardware() {
        assert_eq!(div(0x1000, 0x2000), 0x800);
        assert_eq!(div(1, 3), 0x555);
        assert_eq!(div(-1, 3), -0x555);
        assert_eq!(div(5, 0), 0);
        assert_eq!(div(0x1000, 3), 5_592_405);
    }

    #[test]
    fn products_round_half_up() {
        assert_eq!(mul(0x1000, 0x1000), 0x1000);
        assert_eq!(mul(3, 0x800), 2);
        assert_eq!(mul(-3, 0x800), -1);
    }

    #[test]
    fn signed_distance_takes_the_short_way() {
        assert_eq!(signed_angle_distance(0, 0x100), 0x100);
        assert_eq!(signed_angle_distance(0x100, 0), -0x100);
        assert_eq!(signed_angle_distance(0x100, TWO_PI - 0x100), -0x200);
        assert_eq!(signed_angle_distance(TWO_PI - 0x100, 0x100), 0x200);
        assert_eq!(signed_angle_distance(0, PI), PI);
        assert_eq!(signed_angle_distance(PI, 0), -PI);
    }

    #[test]
    fn lengths_and_normals() {
        assert_eq!(mag([0x3000, 0x4000, 0]), 0x5000);
        assert_eq!(distance([0x1000, 0, 0], [0, 0, 0x1000]), 0x16a1);
        assert_eq!(normalize([0x3000, 0, 0x4000]), [0x99a, 0, 0xccd]);
        assert_eq!(normalize([0, 0, 0]), [0; 3]);
        assert_eq!(normalize([0, 0, -0x10]), [0, 0, -0x1000]);
    }

    #[test]
    fn root_is_rounded() {
        assert_eq!(sqrt(0x4000), 0x2000);
        assert_eq!(sqrt(0x1000), 0x1000);
        assert_eq!(sqrt(0), 0);
    }

    #[test]
    fn atan2_covers_the_axes_and_octants() {
        assert_eq!(atan2(0, 0x1000), 0);
        assert_eq!(atan2(0, -0x1000), PI);
        assert_eq!(atan2(0x1000, 0), 0x1922);
        assert_eq!(atan2(-0x1000, 0), -0x1922);
        assert_eq!(atan2(0x1000, 0x1000), 0xc91);
        assert_eq!(atan2(0x1000, -0x1000), 0x25b3);
        assert_eq!(atan2(-0x1000, -0x1000), -0x25b3);
        assert_eq!(atan2(-0x1000, 0x1000), -0xc91);
        // Every direction against floating point, within the table's resolution.
        for degrees in (0..360).step_by(7) {
            let t = f64::from(degrees).to_radians();
            let (x, z) = ((t.sin() * 4096.0) as i32, (t.cos() * 4096.0) as i32);
            let want = t.sin().atan2(t.cos()) * 4096.0;
            let got = f64::from(atan2(x, z));
            assert!((got - want).abs() < 40.0, "{degrees}: {got} vs {want}");
        }
    }
}
