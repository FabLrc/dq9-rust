//! Time of day and the lighting a map shows at that time (`LightingManager`, EU `0x02107930`).
//!
//! The game keeps one clock of 420 seconds (`GameState+0x3cc`, [`DayClock`] of `state`) and copies its
//! value and phase into `LightingManager+0x94` / `+0x98` every frame (`ov_017::0218d0ec`).
//! [`Lighting::select`] turns a parsed `.bats` script ([`LightingInfo`]) and that state into
//! the values the game sends to the 3D engine: colours, the two lights, the fog registers and
//! the basic-mode vertex colour transform. Everything is a pure function of its inputs; the
//! renderer applies the result. Evidence: `docs/re/lighting.md`.

use dq9_formats::lighting::{
    AdvancedLighting, BasicLighting, FogInfo, Light3DConfig, LightingInfo, LightingMode, SLOTS,
};
use nds_rom::g3d::fx::normalize;

use crate::state::{DayClock, TimeOfDay};

/// End of each phase in seconds (`endTimes` of `LightingManager`; phase starts are
/// [`TimeOfDay::start`]).
pub const PHASE_END: [f32; 4] = [180.0, 210.0, 390.0, 420.0];
/// Seconds over which a phase fades into the next one (`s_dayTransitionLength`).
pub const TRANSITION: f32 = 2.0;

/// Whether the day clock runs on a map of this kind (map list value 3, low nibble of the
/// record's `+0xc`): fields (0) and the ocean and sky (7), `ov_017::0218d0ec`.
pub fn clock_runs_on_map_kind(kind: u8) -> bool {
    matches!(kind & 0xf, 0 | 7)
}

/// `LightingManager+0x94` / `+0x98`: the clock's value and phase as the manager holds them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayState {
    pub index: usize,
    pub timer: f32,
}

impl DayState {
    /// The copy made every frame (`ov_017::0218d0ec`).
    pub fn from_clock(clock: &DayClock) -> Self {
        Self {
            index: clock.phase().index() as usize,
            timer: clock.seconds(),
        }
    }

    /// The start of `phase`, where nothing fades yet.
    pub fn at(phase: TimeOfDay) -> Self {
        Self {
            index: phase.index() as usize,
            timer: phase.start(),
        }
    }
}

/// A colour transform on 5-bit channels applied by basic lighting to vertex colours
/// (`ColorTransformTintBrightnessContrast`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    /// Per-channel multiplier (`redMix_2c_`...): `BasicLighting::positions[index]`
    /// (`func_0207ba28`, EU `0x0207ba38`, copies the three floats).
    pub mix: [f32; 3],
    /// `unk_70[index]`, added to the normalised channel.
    pub brightness: f32,
    /// `unk_54[index]` cubed, added above 0.5 and subtracted below.
    pub contrast: f32,
}

impl Tint {
    /// Transforms a BGR555 colour.
    ///
    /// port: 0x02050980
    pub fn apply(&self, color: u16) -> u16 {
        let mut out = 0u16;
        for channel in 0..3 {
            let value = i32::from(color >> (5 * channel) & 0x1f);
            let scaled = ((value as f32 * self.mix[channel]) as i32).min(31);
            let mut v = (scaled as f32 / 31.0 + self.brightness).clamp(0.0, 1.0);
            if v > 0.5 {
                v += self.contrast;
                if v < 0.5 {
                    v = 0.5;
                }
            } else {
                v -= self.contrast;
                if v > 0.5 {
                    v = 0.5;
                }
            }
            let v = v.clamp(0.0, 1.0);
            out |= ((31.0 * v) as u16) << (5 * channel);
        }
        out
    }
}

/// One of the two DS lights as the game sends it (`RenderConfig::SetLightVector` /
/// `SetLightColor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Light {
    /// `LIGHT_VECTOR` components, signed 1.0.9 fixed point (10 bits).
    pub vector: [i16; 3],
    /// BGR555.
    pub color: u16,
}

impl Light {
    /// The direction the light travels, components in `-1.0..1.0`.
    pub fn direction(&self) -> [f32; 3] {
        self.vector.map(|v| f32::from(v) / 512.0)
    }
}

/// Fog registers: `FOG_COLOR`, `FOG_OFFSET`, `FOG_TABLE` and the `DISP3DCNT` bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fog {
    /// `DISP3DCNT` bit 7 (`func_020c54a4`, EU `0x020c54b4`, first argument).
    pub enabled: bool,
    /// `DISP3DCNT` bit 6: 0 colour and alpha, 1 alpha only.
    pub kind: u8,
    /// `DISP3DCNT` bits 8-11.
    pub depth_shift: u8,
    /// `FOG_OFFSET`, 15 bits.
    pub offset: u16,
    /// `FOG_COLOR` bits 0-14, BGR555.
    pub color: u16,
    /// `FOG_COLOR` bits 16-20.
    pub alpha: u8,
    /// `FOG_TABLE`, 32 densities.
    pub table: [u8; 32],
}

impl Fog {
    /// What the game writes with no lighting (mode 0): `func_020c54a4(0, 0, 1, 0x5800)`.
    pub const OFF: Fog = Fog {
        enabled: false,
        kind: 0,
        depth_shift: 1,
        offset: 0x5800,
        color: 0x7fff,
        alpha: 15,
        table: [0; 32],
    };

    fn from_info(f: &FogInfo, enabled: bool) -> Self {
        Fog {
            enabled,
            kind: f.kind as u8,
            depth_shift: f.depth_shift as u8,
            offset: (f.offset & 0x7fff) as u16,
            color: f.color & 0x7fff,
            alpha: (f.alpha & 0x1f) as u8,
            table: f.density_table.map(|d| d & 0x7f),
        }
    }
}

/// Everything the game derives from a zone's lighting for one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    pub mode: LightingMode,
    /// Slot of the basic entry in use (the phase, or the override).
    pub index: usize,
    /// Outer gradient colour: the clear colour and the sky, BGR555.
    pub background: u16,
    /// Gradient colour at the horizon, BGR555.
    pub horizon: u16,
    /// `LightingInfo::gradientCenterOffset_`, added to the horizon row.
    pub gradient_center_offset: f32,
    /// Ambient reflection colour given to the terrain models (advanced mode).
    pub ambient: u16,
    pub pot_barrel_diffuse: u16,
    pub sprite_diffuse: u16,
    pub model_diffuse: u16,
    /// `EDGE_COLOR` of all eight polygon-id groups.
    pub edge: u16,
    /// The two DS lights (advanced mode; black in basic mode and with no lighting).
    pub lights: [Light; 2],
    /// Vertex colour transform of basic mode.
    pub tint: Option<Tint>,
    pub fog: Fog,
}

/// `InterpolateColors` (`0x02051160`): per channel `a + (b - a) * t`, truncated (the game
/// goes through `char`).
///
/// port: 0x02051160
pub fn interpolate_colors(a: u16, b: u16, t: f32) -> u16 {
    let mut out = 0u16;
    for channel in 0..3 {
        let c0 = i32::from(a >> (5 * channel) & 0x1f);
        let c1 = i32::from(b >> (5 * channel) & 0x1f);
        let lerp = (c0 as f32 + (c1 - c0) as f32 * t) as i32 as u8;
        out |= u16::from(lerp) << (5 * channel);
    }
    out
}

/// Position of the day in the fade to the next phase: 0 before the last [`TRANSITION`]
/// seconds of the phase, 1 at its end.
fn fade(day: DayState, scale: f32) -> f32 {
    let into = scale * (day.timer - (PHASE_END[day.index] - TRANSITION));
    if into < 0.0 { 0.0 } else { into / TRANSITION }
}

/// Copies each invalid entry from the closest valid one before it (cyclically over the first
/// four slots), as `FillMissingEntries` of the three lists does; `valid` tells which entries
/// are set. The game's search gives up after three tries *without* testing the last one, then
/// copies from it anyway; the same is done here. `copy(to, from)` moves one entry.
fn fill_missing(valid: impl Fn(usize) -> bool, mut copy: impl FnMut(usize, usize)) {
    for index in 0..4 {
        if valid(index) {
            continue;
        }
        let mut source = index;
        for _ in 0..3 {
            source = (source + 3) % 4;
            if valid(source) {
                break;
            }
        }
        copy(index, source);
    }
}

/// `AdvancedLighting::FillMissingEntries` (`0x020504b4`).
///
/// port: 0x020504b4
pub fn fill_advanced(a: &mut AdvancedLighting) {
    let light0: Vec<bool> = a.light0.iter().map(|l| l.enabled).collect();
    fill_missing(
        |i| light0[i],
        |to, from| {
            a.light0[to] = a.light0[from];
            a.ambient[to] = a.ambient[from];
            a.background[to] = a.background[from];
            a.background_second[to] = a.background_second[from];
            a.unk_142[to] = a.unk_142[from];
            a.sprite_diffuse[to] = a.sprite_diffuse[from];
            a.model_diffuse[to] = a.model_diffuse[from];
            a.edge[to] = a.edge[from];
        },
    );
    let light1: Vec<bool> = a.light1.iter().map(|l| l.enabled).collect();
    fill_missing(
        |i| light1[i],
        |to, from| {
            a.light1[to] = a.light1[from];
        },
    );
}

/// `BasicLighting::FillMissingEntries` (`0x020506ec`).
///
/// port: 0x020506ec
pub fn fill_basic(b: &mut BasicLighting) {
    let set = b.entry_set;
    fill_missing(
        |i| set[i],
        |to, from| {
            b.positions[to] = b.positions[from];
            b.unk_54[to] = b.unk_54[from];
            b.unk_70[to] = b.unk_70[from];
            b.background[to] = b.background[from];
            b.background_second[to] = b.background_second[from];
            b.pot_barrel_diffuse[to] = b.pot_barrel_diffuse[from];
            b.sprite_diffuse[to] = b.sprite_diffuse[from];
            b.model_diffuse[to] = b.model_diffuse[from];
            b.edge[to] = b.edge[from];
        },
    );
}

/// `FogList::FillMissingEntries` (`0x02050840`).
///
/// port: 0x02050840
pub fn fill_fog(fog: &mut [FogInfo; SLOTS]) {
    let set: Vec<bool> = fog.iter().map(|f| f.enabled).collect();
    fill_missing(
        |i| set[i],
        |to, from| {
            let source = fog[from].clone();
            let entry = &mut fog[to];
            entry.kind = source.kind;
            entry.depth_shift = source.depth_shift;
            entry.offset = source.offset;
            entry.color = source.color;
            entry.alpha = source.alpha;
            entry.density_table = source.density_table;
        },
    );
}

/// `ComputeFogInfo` (`0x02051254`): the fog of the current phase faded into the next one.
///
/// port: 0x02051254
fn fogs_lerp(list: &[FogInfo; SLOTS], day: DayState, scale: f32) -> FogInfo {
    let next = (day.index + 1) % 4;
    let t = fade(day, scale);
    let (old, new) = (&list[day.index], &list[next]);
    let mut out = old.clone();
    out.color = interpolate_colors(old.color, new.color, t);
    out.alpha = i32::from((old.alpha + ((new.alpha - old.alpha) as i8 as f32 * t) as i32) as i8);
    out.offset = old.offset + ((new.offset - old.offset) as f32 * t) as i32;
    for i in 0..32 {
        let diff = i32::from(new.density_table[i]) - i32::from(old.density_table[i]);
        out.density_table[i] =
            (f32::from(old.density_table[i]) + f32::from(diff as i8) * t) as i32 as u8;
    }
    out
}

fn scale_channels(color: u16, scale: f32) -> u16 {
    let mut out = 0;
    for channel in 0..3 {
        let c = f32::from(color >> (5 * channel) & 0x1f);
        out |= ((c * scale) as u16) << (5 * channel);
    }
    out
}

/// `RecomputeAdvancedLighting` (`0x02051d38`) light setup: a direction in 1.19.12 rounded
/// half away from zero, normalised with `VEC_Normalize`, each component capped at `0xff7`,
/// then `SetLightVector` keeps bits 3..12 as a signed 1.0.9 vector.
fn light_vector(direction: [f32; 3]) -> [i16; 3] {
    let fix = direction.map(|d| {
        if d > 0.0 {
            (4096.0 * d + 0.5) as i32
        } else {
            (4096.0 * d - 0.5) as i32
        }
    });
    let mut v = normalize(fix);
    for c in &mut v {
        *c = (*c).min(0xff7);
    }
    v.map(|c| {
        let raw = (c >> 3) & 0x3ff;
        ((raw << 22) >> 22) as i16
    })
}

fn lerp_direction(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn light_at(entries: &[Light3DConfig; SLOTS], day: DayState, t: f32, scale: f32) -> Light {
    let (a, b) = (&entries[day.index], &entries[(day.index + 1) % 4]);
    let direction = lerp_direction(a.direction, b.direction, t);
    let color = interpolate_colors(a.color, b.color, t);
    Light {
        vector: light_vector(direction),
        color: if scale == 1.0 {
            color
        } else {
            scale_channels(color, scale)
        },
    }
}

impl Lighting {
    /// What the game has for a zone: no lighting, the zone's `.bats` (`info`), at `day`.
    /// `override_index` is `LightingManager+0x90` (0: none); `rgb_scale` is
    /// `lightRGBScale_ / 4096` (1.0 unless the game fades the light).
    ///
    /// Advanced zones blend into the next phase over the last two seconds of each phase
    /// (`GetCurrentAdvancedLightingValues`, `0x02050c30`; fog: `ComputeFogInfo`). Basic
    /// zones read the phase's entry (`ProcessZoneChange`, `0x020519f4`); entering one within
    /// four seconds of a phase's end moves the clock to the next phase, which the caller
    /// does with [`settle_basic_entry`].
    pub fn select(
        info: Option<&LightingInfo>,
        day: DayState,
        override_index: usize,
        rgb_scale: f32,
    ) -> Lighting {
        let mut lighting = Self::select_unscaled_background(info, day, override_index, rgb_scale);
        if rgb_scale != 1.0 {
            // `DrawBackgroundGradient` (`0x02052420`) multiplies the gradient colours by
            // `lightRGBScale_` too (USA `LightingManager.cpp` l.1087).
            lighting.background = scale_channels(lighting.background, rgb_scale);
            lighting.horizon = scale_channels(lighting.horizon, rgb_scale);
        }
        lighting
    }

    fn select_unscaled_background(
        info: Option<&LightingInfo>,
        day: DayState,
        override_index: usize,
        rgb_scale: f32,
    ) -> Lighting {
        let none = Lighting {
            mode: LightingMode::None,
            index: 0,
            background: 0,
            horizon: 0,
            gradient_center_offset: 0.0,
            ambient: 0,
            pot_barrel_diffuse: 0x7fff,
            sprite_diffuse: 0x7fff,
            model_diffuse: 0x7fff,
            edge: (4 << 10) | (4 << 5) | 6,
            lights: [Light::default(); 2],
            tint: None,
            fog: Fog::OFF,
        };
        let Some(info) = info else { return none };
        match info.mode {
            LightingMode::Advanced => {
                let mut a = info.advanced.clone();
                let mut fog = info.fog.clone();
                fill_advanced(&mut a);
                fill_fog(&mut fog);
                let i = override_index.min(SLOTS - 1);
                if override_index != 0 {
                    Lighting {
                        mode: LightingMode::Advanced,
                        index: i,
                        background: a.background[i],
                        horizon: a.background_second[i],
                        gradient_center_offset: info.gradient_center_offset,
                        ambient: a.ambient[i],
                        pot_barrel_diffuse: a.unk_142[i],
                        sprite_diffuse: a.sprite_diffuse[i],
                        model_diffuse: a.model_diffuse[i],
                        edge: a.edge[i],
                        lights: [&a.light0[i], &a.light1[i]].map(|l| Light {
                            vector: light_vector(l.direction),
                            color: scale_channels(l.color, rgb_scale),
                        }),
                        tint: None,
                        fog: Fog::from_info(&fog[i], true),
                    }
                } else {
                    let day = DayState {
                        index: day.index % 4,
                        ..day
                    };
                    let t = fade(day, 1.0);
                    let next = (day.index + 1) % 4;
                    let lerp =
                        |arr: &[u16; SLOTS]| interpolate_colors(arr[day.index], arr[next], t);
                    Lighting {
                        mode: LightingMode::Advanced,
                        index: day.index,
                        background: lerp(&a.background),
                        horizon: lerp(&a.background_second),
                        gradient_center_offset: info.gradient_center_offset,
                        ambient: lerp(&a.ambient),
                        pot_barrel_diffuse: lerp(&a.unk_142),
                        sprite_diffuse: lerp(&a.sprite_diffuse),
                        model_diffuse: lerp(&a.model_diffuse),
                        edge: lerp(&a.edge),
                        lights: [
                            light_at(&a.light0, day, t, rgb_scale),
                            light_at(&a.light1, day, t, rgb_scale),
                        ],
                        tint: None,
                        fog: Fog::from_info(&fogs_lerp(&fog, day, rgb_scale), true),
                    }
                }
            }
            LightingMode::Basic => {
                let mut b = info.basic.clone();
                let mut fog = info.fog.clone();
                fill_basic(&mut b);
                fill_fog(&mut fog);
                let i = if override_index != 0 {
                    override_index
                } else {
                    day.index
                }
                .min(SLOTS - 1);
                let p = b.positions[i];
                let contrast = b.unk_54[i];
                Lighting {
                    mode: LightingMode::Basic,
                    index: i,
                    background: b.background[i],
                    horizon: b.background_second[i],
                    gradient_center_offset: info.gradient_center_offset,
                    ambient: 0,
                    pot_barrel_diffuse: b.pot_barrel_diffuse[i],
                    sprite_diffuse: b.sprite_diffuse[i],
                    model_diffuse: b.model_diffuse[i],
                    edge: b.edge[i],
                    lights: [Light::default(); 2],
                    tint: Some(Tint {
                        mix: [p.x, p.y, p.z],
                        brightness: b.unk_70[i],
                        contrast: contrast * (contrast * contrast),
                    }),
                    fog: Fog::from_info(&fog[i], true),
                }
            }
            LightingMode::None => none,
        }
    }
}

/// `ProcessZoneChange` for a basic zone (`0x020519f4`): entering within `2 * TRANSITION`
/// seconds of the end of the phase moves the manager's phase to the next one and sets the
/// game's day clock to that phase's start.
pub fn settle_basic_entry(clock: &mut DayClock) {
    let day = DayState::from_clock(clock);
    if day.timer > PHASE_END[day.index] - 2.0 * TRANSITION {
        let next = [
            TimeOfDay::Night,
            TimeOfDay::Morning,
            TimeOfDay::Day,
            TimeOfDay::Evening,
        ][(day.index + 1) % 4];
        clock.set_phase(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::lighting::Vec3;

    #[test]
    fn colours_interpolate_per_channel() {
        let a = 0x7fff;
        let b = 0;
        assert_eq!(interpolate_colors(a, b, 0.0), a);
        assert_eq!(interpolate_colors(a, b, 1.0), b);
        // 31 + (0 - 31) * 0.5 = 15.5, truncated.
        assert_eq!(interpolate_colors(a, b, 0.5), 15 | 15 << 5 | 15 << 10);
        assert_eq!(interpolate_colors(0x0421, 0x0c63, 0.5), 0x0842);
    }

    fn advanced() -> LightingInfo {
        let mut info = LightingInfo {
            mode: LightingMode::Advanced,
            ..LightingInfo::default()
        };
        for i in 0..4 {
            info.advanced.light0[i] = Light3DConfig {
                color: 0x7fff,
                direction: [0.0, -1.0, 0.0],
                enabled: true,
            };
            info.advanced.light1[i] = info.advanced.light0[i];
            info.advanced.background[i] = (i as u16 + 1) * 4;
        }
        info.advanced.light0[1].direction = [0.0, 0.0, -1.0];
        info
    }

    #[test]
    fn advanced_zone_fades_only_in_the_last_two_seconds() {
        let info = advanced();
        let at = |timer| Lighting::select(Some(&info), DayState { index: 0, timer }, 0, 1.0);
        assert_eq!(at(100.0).background, 4);
        assert_eq!(at(177.9).background, 4);
        // Halfway through the fade: 4 + (8 - 4) / 2.
        assert_eq!(at(179.0).background, 6);
        // The light turns half way between its two directions.
        let l = at(179.0).lights[0];
        assert_eq!(l.vector[0], 0);
        assert!(l.vector[1] < 0 && l.vector[2] < 0);
        assert_eq!(at(100.0).lights[0].vector, [0, -512, 0]);
    }

    #[test]
    fn light_vector_is_capped_below_one() {
        assert_eq!(light_vector([0.0, 1.0, 0.0]), [0, 510, 0]);
        assert_eq!(light_vector([0.0, -1.0, 0.0]), [0, -512, 0]);
        assert_eq!(light_vector([0.0; 3]), [0, 0, 0]);
    }

    #[test]
    fn missing_entries_copy_from_the_previous_valid_one() {
        let mut b = BasicLighting::default();
        b.entry_set[2] = true;
        b.background[2] = 0x1234;
        b.background[3] = 0x4321;
        fill_basic(&mut b);
        // 0 and 1 take the cyclic predecessor search result (slot 2 after three steps back);
        // 3 is copied from 2.
        assert_eq!(b.background[3], 0x1234);
        assert_eq!(b.background[0], 0x1234);
        assert_eq!(b.background[1], 0x1234);
    }

    #[test]
    fn basic_zone_takes_the_phase_entry_and_the_tint_numbers_of_the_game() {
        // C01M09 at night as the game held it in RAM (melonDS state): mix 0.81, 1.16, 2.0,
        // brightness 0.03, contrast 0.26 cubed.
        let mut info = LightingInfo {
            mode: LightingMode::Basic,
            ..LightingInfo::default()
        };
        for i in 0..4 {
            info.basic.entry_set[i] = true;
        }
        info.basic.positions[0] = Vec3 {
            x: 0.81,
            y: 1.16,
            z: 2.0,
        };
        info.basic.unk_70[0] = 0.03;
        info.basic.unk_54[0] = 0.26;
        info.basic.edge[0] = 0x0466;
        let l = Lighting::select(Some(&info), DayState::at(TimeOfDay::Night), 0, 1.0);
        let tint = l.tint.unwrap();
        assert_eq!(tint.mix, [0.81, 1.16, 2.0]);
        assert!((tint.contrast - 0.017576).abs() < 1e-6);
        assert_eq!(l.edge, 0x0466);
        assert_eq!(l.lights, [Light::default(); 2]);
        // White: red 31 * 0.81 = 25 -> 0.806 + 0.03 + 0.0176, blue saturates.
        let out = tint.apply(0x7fff);
        assert_eq!(out >> 10, 31);
        assert!((out & 31) < 31 && (out & 31) > 24);
    }

    #[test]
    fn no_lighting_is_what_the_game_writes_for_mode_zero() {
        let l = Lighting::select(None, DayState::at(TimeOfDay::Day), 0, 1.0);
        assert_eq!(l.mode, LightingMode::None);
        assert_eq!((l.model_diffuse, l.edge), (0x7fff, 0x1086));
        assert!(!l.fog.enabled && l.fog.offset == 0x5800 && l.fog.depth_shift == 1);
    }

    #[test]
    fn entering_a_basic_zone_near_the_end_of_a_phase_moves_the_clock_on() {
        let mut c = DayClock::new(385.0, true);
        settle_basic_entry(&mut c);
        assert_eq!((c.phase(), c.seconds()), (TimeOfDay::Day, 385.0));
        let mut c = DayClock::new(386.5, true);
        settle_basic_entry(&mut c);
        assert_eq!((c.phase(), c.seconds()), (TimeOfDay::Evening, 390.0));
    }

    #[test]
    fn advanced_fog_fades_with_the_colour() {
        let mut info = advanced();
        for i in 0..4 {
            info.fog[i].enabled = true;
            info.fog[i].offset = 0x5800;
            info.fog[i].density_table = [if i == 1 { 100 } else { 0 }; 32];
        }
        let l = Lighting::select(
            Some(&info),
            DayState {
                index: 0,
                timer: 179.0,
            },
            0,
            1.0,
        );
        assert_eq!(l.fog.table[0], 50);
        assert!(l.fog.enabled);
    }
}
