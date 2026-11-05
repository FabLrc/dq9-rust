//! Fixed-point numbers and the small matrix type used by the G3D readers.
//!
//! The DS geometry engine works in signed fixed point with 12 fractional bits. Every G3D value
//! with that resolution (`fx32` 1.19.12, `fx16` 1.3.12, vertex coordinates, matrix entries) is
//! kept exact as an [`Fx12`]; conversion to floating point only happens when a caller asks.

/// Signed fixed-point number with 12 fractional bits (`raw / 4096`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fx12(pub i32);

impl Fx12 {
    pub const ONE: Fx12 = Fx12(1 << 12);
    pub const ZERO: Fx12 = Fx12(0);

    /// From a 1.3.12 `fx16` stored as a raw 16-bit word.
    pub fn from_fx16(raw: u16) -> Fx12 {
        Fx12(i32::from(raw as i16))
    }

    /// From a 1.19.12 `fx32` stored as a raw 32-bit word.
    pub fn from_fx32(raw: u32) -> Fx12 {
        Fx12(raw as i32)
    }

    pub fn to_f32(self) -> f32 {
        self.0 as f32 / 4096.0
    }

    pub fn wrapping_neg(self) -> Fx12 {
        Fx12(self.0.wrapping_neg())
    }
}

/// Sign-extends the low `bits` bits of `value`.
pub(crate) fn sext(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

/// Bit field `[lo, hi)` of `value`.
pub(crate) fn bits(value: u32, lo: u32, hi: u32) -> u32 {
    (value >> lo) & ((1u32 << (hi - lo)) - 1)
}

/// `VEC_Normalize` (EU `0x020c2f28`): normalises a 1.19.12 vector exactly as the NITRO library
/// does with the hardware divider (`2^56 / |v|²`) and square root (`sqrt(4 |v|²)`). A zero
/// vector stays zero.
pub(crate) fn normalize(v: [i32; 3]) -> [i32; 3] {
    let t = v
        .iter()
        .fold(0i64, |t, &c| t.wrapping_add(i64::from(c) * i64::from(c)));
    let sqrt = isqrt64((t as u64).wrapping_mul(4)) as i32;
    let inv = hw_div64(1 << 56, t);
    let prod = inv.wrapping_mul(i64::from(sqrt));
    v.map(|c| (((i128::from(prod) * i128::from(c)) >> 32) as i32).wrapping_add(0x1000) >> 13)
}

/// The hardware square root unit in 64-bit mode: `floor(sqrt(x))`.
fn isqrt64(x: u64) -> u32 {
    let x = u128::from(x);
    let mut r = (x as f64).sqrt() as u128;
    while r * r > x {
        r -= 1;
    }
    while (r + 1) * (r + 1) <= x {
        r += 1;
    }
    r as u32
}

/// The hardware divider in 64/64 or 64/32 mode: truncating signed division; a zero
/// denominator gives `-1` for a non-negative numerator and `1` otherwise (GBATEK).
fn hw_div64(num: i64, den: i64) -> i64 {
    if den == 0 {
        if num >= 0 { -1 } else { 1 }
    } else {
        num.wrapping_div(den)
    }
}

/// `FX_DivAsync` + `FX_GetDivResult` (EU `0x020c2d00`, `0x020c2c6c`): `num / den` in 1.19.12,
/// rounded to nearest, through the hardware divider (`(num << 32) / den`).
pub(crate) fn fx_div(num: i32, den: i32) -> i32 {
    let q = hw_div64(i64::from(num) << 32, i64::from(den));
    (q.wrapping_add(0x80000) >> 20) as i32
}

/// `(a * b) >> 12` on 64 bits, truncated to 32 like the NITRO library's `FX_Mul`.
pub(crate) fn mul64(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 12) as i32
}

/// 4x4 `f32` matrix for column vectors, stored `m[column][row]` (OpenGL layout).
///
/// Composition follows apicula: `current = current * object`, points are `M * p`. The DS stores
/// its matrices row-major for row vectors, which is the same memory order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [[f32; 4]; 4]);

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    pub const ZERO: Mat4 = Mat4([[0.0; 4]; 4]);

    pub fn scale(s: [f32; 3]) -> Mat4 {
        let mut m = Self::IDENTITY;
        m.0[0][0] = s[0];
        m.0[1][1] = s[1];
        m.0[2][2] = s[2];
        m
    }

    pub fn translation(t: [f32; 3]) -> Mat4 {
        let mut m = Self::IDENTITY;
        m.0[3][0] = t[0];
        m.0[3][1] = t[1];
        m.0[3][2] = t[2];
        m
    }

    /// From a 3x3 block given column by column (9 entries, the DS memory order).
    pub fn from_mat3(cols: &[Fx12; 9]) -> Mat4 {
        Self::from_columns(cols)
    }

    /// From a 4x3 matrix stored as the DS does for row vectors: four rows of three, the last
    /// one the translation. That is column by column for column vectors.
    pub fn from_mat43(cols: &[Fx12; 12]) -> Mat4 {
        Self::from_columns(cols)
    }

    fn from_columns(cols: &[Fx12]) -> Mat4 {
        let mut m = Self::IDENTITY;
        for (c, col) in cols.as_chunks::<3>().0.iter().enumerate() {
            for (r, v) in col.iter().enumerate() {
                m.0[c][r] = v.to_f32();
            }
        }
        m
    }

    pub fn mul(&self, rhs: &Mat4) -> Mat4 {
        let mut out = Self::ZERO;
        for c in 0..4 {
            for r in 0..4 {
                out.0[c][r] = (0..4).map(|k| self.0[k][r] * rhs.0[c][k]).sum();
            }
        }
        out
    }

    pub fn add_scaled(&mut self, rhs: &Mat4, factor: f32) {
        for c in 0..4 {
            for r in 0..4 {
                self.0[c][r] += rhs.0[c][r] * factor;
            }
        }
    }

    pub fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
    }

    pub fn transform_vector(&self, v: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        std::array::from_fn(|r| m[0][r] * v[0] + m[1][r] * v[1] + m[2][r] * v[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_conversions() {
        assert_eq!(Fx12::from_fx16(0xF000).to_f32(), -1.0);
        assert_eq!(Fx12::from_fx32(0x1800).to_f32(), 1.5);
        assert_eq!(sext(0x3FF, 10), -1);
        assert_eq!(sext(0x1FF, 10), 511);
        assert_eq!(bits(0b1011_0000, 4, 8), 0b1011);
    }

    #[test]
    fn hardware_math() {
        assert_eq!(normalize([0x2000, 0, 0]), [0x1000, 0, 0]);
        assert_eq!(normalize([0, 0, 0]), [0, 0, 0]);
        let n = normalize([0x1000, 0x1000, 0]);
        assert!(
            (n[0] - 2896).abs() <= 1 && n[0] == n[1] && n[2] == 0,
            "{n:?}"
        );
        assert_eq!(fx_div(0x1000, 0x2000), 0x800);
        assert_eq!(fx_div(-0x3000, 0x1000), -0x3000);
        assert_eq!(fx_div(0x1000, 0), ((-1i64 + 0x80000) >> 20) as i32);
        assert_eq!(isqrt64(u64::MAX), u32::MAX);
        assert_eq!(isqrt64(15), 3);
        assert_eq!(mul64(0x1800, -0x2000), -0x3000);
    }

    #[test]
    fn matrix_composition() {
        let m = Mat4::translation([1.0, 2.0, 3.0]).mul(&Mat4::scale([2.0, 2.0, 2.0]));
        assert_eq!(m.transform_point([1.0, 1.0, 1.0]), [3.0, 4.0, 5.0]);
        assert_eq!(m.transform_vector([1.0, 0.0, 0.0]), [2.0, 0.0, 0.0]);
    }
}
