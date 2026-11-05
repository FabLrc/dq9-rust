//! Conversions of the DS sound driver's units: volumes in 1/10 dB "square" steps, envelope
//! rates, the modulation sine. They reproduce what the SDK computes (as kode54's SSEQPlayer
//! and fincs's FSS document it, both WTFPL); the driver itself is platform code and is not
//! translated.

use std::sync::LazyLock;

/// The lowest attenuation of an envelope, in the 1/128 steps the envelope counts in.
pub const AMPL_MIN: i32 = -723 << 7;

/// `0` to `127` linear levels as 1/10 dB of the amplitude *squared* (the SDK's
/// `SNDi_DecibelSquareTable`): `400 * log10(level / 127)`, with the two quietest levels
/// pinned just above the floor and `0` silent.
static DECIBEL_SQUARE: LazyLock<[i16; 128]> = LazyLock::new(|| {
    let mut table = [0i16; 128];
    table[0] = -32768;
    for (level, slot) in table.iter_mut().enumerate().skip(1) {
        let db = 400.0 * (level as f64 / 127.0).log10();
        *slot = db.round().max(-722.0) as i16;
    }
    table
});

/// Attenuation of a 0 to 127 level, in 1/10 dB (0 for 127, -32768 for 0).
pub fn decibel_square(level: u8) -> i32 {
    i32::from(DECIBEL_SQUARE[usize::from(level.min(127))])
}

/// Linear amplitude of an attenuation in 1/10 dB (`10^(db/200)`).
pub fn gain(db: i32) -> f32 {
    if db <= -1000 {
        0.0
    } else {
        10f32.powf(db as f32 / 200.0)
    }
}

/// Attack coefficient of an instrument's attack value (0 slowest to 127 instant): the
/// envelope is multiplied by `rate / 256` every update.
pub fn attack_rate(attack: u8) -> i32 {
    const FAST: [i32; 19] = [
        0x00, 0x01, 0x05, 0x0e, 0x1a, 0x26, 0x33, 0x3f, 0x49, 0x54, 0x5c, 0x64, 0x6d, 0x74, 0x7b,
        0x7f, 0x84, 0x89, 0x8f,
    ];
    let attack = i32::from(attack.min(127));
    if attack >= 0x6d {
        FAST[(0x7f - attack) as usize]
    } else {
        0xff - attack
    }
}

/// Fall rate of a decay or release value, in envelope steps per update.
pub fn fall_rate(fall: u8) -> i32 {
    let fall = i32::from(fall.min(127));
    match fall {
        0x7f => 0xffff,
        0x7e => 0x3c00,
        f if f < 0x32 => (f << 1) + 1,
        f => 0x1e00 / (0x7e - f),
    }
}

/// One cycle of a sine over 128 phases, -127 to 127 (the modulation's oscillator).
pub fn sine(phase: u32) -> i32 {
    let angle = f64::from(phase & 0x7f) / 128.0 * std::f64::consts::TAU;
    (angle.sin() * 127.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibel_square_matches_the_sdk_anchors() {
        assert_eq!(decibel_square(127), 0);
        assert_eq!(decibel_square(0), -32768);
        assert_eq!(decibel_square(1), -722);
        assert_eq!(decibel_square(2), -721);
        assert_eq!(decibel_square(3), -651);
        assert_eq!(decibel_square(4), -601);
        assert_eq!(decibel_square(5), -562);
    }

    #[test]
    fn velocity_is_a_square_law() {
        let half = gain(decibel_square(64));
        assert!((half - 0.254).abs() < 0.01, "{half}");
        assert!((gain(0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn envelope_rates() {
        assert_eq!(attack_rate(127), 0);
        assert_eq!(attack_rate(0), 255);
        assert_eq!(attack_rate(108), 147);
        assert_eq!(attack_rate(109), 143);
        assert_eq!(fall_rate(127), 0xffff);
        assert_eq!(fall_rate(126), 0x3c00);
        assert_eq!(fall_rate(0), 1);
        assert_eq!(fall_rate(0x32), 0x1e00 / 0x4c);
    }
}
