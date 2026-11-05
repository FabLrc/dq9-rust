//! Shared sampling logic of the NITRO animation curves (`JNT0`, `SRT0`, `MAT0`).
//!
//! A sampled curve stores one value every `2^rate` frames up to its `end` frame, then one per
//! frame after it; the library reads intermediate frames as the average of two samples (one
//! frame between) or a 3:1 blend (three frames between). The index computation is the same in
//! every sampler of the library: `CalculateTranslationAmountFrameAligned` (EU `0x020b7f64`),
//! `SampleScalarFromMATTrack` (EU `0x020b9288`), `GetColorFromMaterialAnimation`
//! (EU `0x020b8e70`) and their siblings.

use super::at;
use crate::{Error, Result};

/// Which samples a frame reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    /// One sample.
    Direct(usize),
    /// `(s[i] + s[i + 1]) / 2`.
    Average(usize),
    /// `(3 s[high] + s[low]) / 4`.
    Weighted { high: usize, low: usize },
}

/// The samples read for an integer `frame` of a curve with metadata `rate` (bits 30-31 of the
/// curve word) and `end` (last frame sampled at the reduced rate).
pub(crate) fn pick(frame: u32, rate: u32, end: u32) -> Pick {
    let f = frame as usize;
    let end = end as usize;
    if rate == 0 {
        return Pick::Direct(f);
    }
    if rate & 1 != 0 {
        // Every second frame (rate 3 is read as 1).
        if f & 1 == 0 {
            Pick::Direct(f / 2)
        } else if f > end {
            Pick::Direct(end / 2 + 1)
        } else {
            Pick::Average(f / 2)
        }
    } else {
        // Every fourth frame.
        let m = f & 3;
        if m == 0 {
            Pick::Direct(f / 4)
        } else if f > end {
            Pick::Direct(end / 4 + m)
        } else if m == 2 {
            Pick::Average(f / 4)
        } else if m == 3 {
            Pick::Weighted {
                high: f / 4 + 1,
                low: f / 4,
            }
        } else {
            Pick::Weighted {
                high: f / 4,
                low: f / 4 + 1,
            }
        }
    }
}

/// Interpolation between two samples for a fractional time (`JNT0` only):
/// `(s[a] * premul + ((t * (s[b] - s[a])) >> 12)) >> shift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Smooth {
    Direct(usize),
    Lerp {
        a: usize,
        b: usize,
        t: i32,
        premul: i32,
        shift: u32,
    },
}

/// The interpolation of `CalculateTranslationAmountSmooth` (EU `0x020b80c4`) and its siblings
/// for `time` (1.19.12 frames) in an animation of `frame_count` frames that loops (last frame
/// blends into frame 0) or not.
pub(crate) fn smooth(time: i32, rate: u32, end: u32, frame_count: u16, looping: bool) -> Smooth {
    let f = (time >> 12) as u32 as usize;
    let end = end as usize;
    if f + 1 == usize::from(frame_count) {
        let idx = match rate {
            0 => f,
            r if r & 1 != 0 => (f & 1) + (f >> 1),
            _ => (f & 3) + (f >> 2),
        };
        return if looping {
            Smooth::Lerp {
                a: idx,
                b: 0,
                t: time & 0xfff,
                premul: 1,
                shift: 0,
            }
        } else {
            Smooth::Direct(idx)
        };
    }
    let lerp = |a: usize, t: i32, premul: i32, shift: u32| Smooth::Lerp {
        a,
        b: a + 1,
        t,
        premul,
        shift,
    };
    match rate {
        0 => lerp(f, time & 0xfff, 1, 0),
        r if r & 1 != 0 => {
            if f >= end {
                lerp(end / 2, time & 0xfff, 1, 0)
            } else {
                lerp(f / 2, time & 0x1fff, 2, 1)
            }
        }
        _ => {
            if f >= end {
                lerp((f & 3) + f / 4, time & 0xfff, 1, 0)
            } else {
                lerp(f / 4, time & 0x3fff, 4, 2)
            }
        }
    }
}

/// `(a * premul + ((t * (b - a)) >> 12)) >> shift` in 32-bit arithmetic.
pub(crate) fn lerp(a: i32, b: i32, t: i32, premul: i32, shift: u32) -> i32 {
    a.wrapping_mul(premul)
        .wrapping_add(t.wrapping_mul(b.wrapping_sub(a)) >> 12)
        >> shift
}

/// Sample array at `base` in an animation file, read with bounds checks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Samples<'a> {
    pub data: &'a [u8],
    pub base: usize,
}

impl Samples<'_> {
    fn bytes<const N: usize>(&self, idx: usize) -> Result<[u8; N]> {
        let off = idx
            .checked_mul(N)
            .ok_or(Error::Malformed("G3D animation sample index"))?;
        let start = at(self.base, off as u64)?;
        self.data
            .get(start..start.saturating_add(N))
            .and_then(|b| b.try_into().ok())
            .ok_or(Error::Malformed("G3D animation sample out of bounds"))
    }

    pub fn i32(&self, idx: usize) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes::<4>(idx)?))
    }

    pub fn i16(&self, idx: usize) -> Result<i32> {
        Ok(i32::from(i16::from_le_bytes(self.bytes::<2>(idx)?)))
    }

    pub fn u16(&self, idx: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes::<2>(idx)?))
    }

    pub fn u8(&self, idx: usize) -> Result<u8> {
        Ok(self.bytes::<1>(idx)?[0])
    }

    /// Element `idx` of an array of `(i32, i32)` pairs.
    pub fn i32_pair(&self, idx: usize) -> Result<[i32; 2]> {
        let b = self.bytes::<8>(idx)?;
        Ok([0, 4].map(|o| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])))
    }

    /// Element `idx` of an array of `(i16, i16)` pairs.
    pub fn i16_pair(&self, idx: usize) -> Result<[i32; 2]> {
        let b = self.bytes::<4>(idx)?;
        Ok([0, 2].map(|o| i32::from(i16::from_le_bytes([b[o], b[o + 1]]))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_samples_by_rate() {
        assert_eq!(pick(5, 0, 0), Pick::Direct(5));
        // Every second frame, samples at 0, 2, 4, 6, 8 then frame 9 stored on its own.
        assert_eq!(pick(4, 1, 8), Pick::Direct(2));
        assert_eq!(pick(5, 1, 8), Pick::Average(2));
        assert_eq!(pick(9, 1, 8), Pick::Direct(5));
        // Every fourth frame, samples at 0, 4, 8 then frames 9 and 10 on their own.
        assert_eq!(pick(5, 2, 8), Pick::Weighted { high: 1, low: 2 });
        assert_eq!(pick(6, 2, 8), Pick::Average(1));
        assert_eq!(pick(7, 2, 8), Pick::Weighted { high: 2, low: 1 });
        assert_eq!(pick(9, 2, 8), Pick::Direct(3));
        assert_eq!(pick(10, 2, 8), Pick::Direct(4));
    }

    #[test]
    fn smooth_interpolation() {
        let half = 0x800;
        assert_eq!(
            smooth((3 << 12) | half, 0, 0, 10, false),
            Smooth::Lerp {
                a: 3,
                b: 4,
                t: half,
                premul: 1,
                shift: 0
            }
        );
        assert_eq!(smooth(9 << 12 | half, 0, 0, 10, false), Smooth::Direct(9));
        assert!(matches!(
            smooth(9 << 12 | half, 0, 0, 10, true),
            Smooth::Lerp { a: 9, b: 0, .. }
        ));
        assert_eq!(
            smooth((5 << 12) | half, 2, 8, 12, false),
            Smooth::Lerp {
                a: 1,
                b: 2,
                t: (5 << 12 | half) & 0x3fff,
                premul: 4,
                shift: 2
            }
        );
        // Frame 5.5 between samples at 4 and 8: 1.5 frames of 4.
        assert_eq!(lerp(0, 0x800, 0x1800, 4, 2), 0x300);
    }

    #[test]
    fn reads_samples_in_bounds() {
        let s = Samples {
            data: &[1, 0, 0xFF, 0xFF],
            base: 0,
        };
        assert_eq!(s.i16(1).unwrap(), -1);
        assert_eq!(s.i32(0).unwrap(), -65535);
        assert_eq!(s.i16_pair(0).unwrap(), [1, -1]);
        assert!(s.i32(1).is_err());
        assert!(s.u8(usize::MAX).is_err());
    }
}
