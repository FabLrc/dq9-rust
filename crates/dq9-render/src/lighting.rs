//! The zone's lighting as the renderer applies it: the two DS lights, the basic-mode colour
//! transform, the fog registers and the background gradient. Plain data, filled by the
//! application from `dq9_core::lighting::Lighting`; the maths follows the DS 3D engine
//! (`docs/re/lighting.md`, `docs/app.md`).

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::camera::{Camera, DS_FAR, DS_NEAR};

/// A DS light: the direction it travels in world space (unit) and its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub direction: [f32; 3],
    pub color: [f32; 3],
}

/// `FOG_COLOR`, `FOG_OFFSET`, `FOG_TABLE` and the `DISP3DCNT` fog bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    /// Fog colour, 5-bit channels in `0..=31`.
    pub color: [u8; 3],
    /// `DISP3DCNT` bit 6: the fog changes only alpha, not colour.
    pub alpha_only: bool,
    /// `FOG_OFFSET` register, 15 bits.
    pub offset: u16,
    /// `DISP3DCNT` bits 8-11.
    pub shift: u8,
    /// `FOG_TABLE`, 32 densities of 7 bits.
    pub table: [u8; 32],
}

/// Basic-mode colour transform of vertex colours (see `dq9_core::lighting::Tint`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    pub mix: [f32; 3],
    pub brightness: f32,
    pub contrast: f32,
}

/// The sky gradient drawn behind everything (`LightingManager::DrawBackgroundGradient`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Background {
    /// Colour at the top and, when the horizon is in the lower half, at the bottom.
    pub outer: [u8; 3],
    /// Colour on the horizon row.
    pub horizon: [u8; 3],
    /// `LightingInfo::gradientCenterOffset_`.
    pub center_offset: f32,
}

/// Everything the zone's lighting changes in a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// `LIGHT_VECTOR` / `LIGHT_COLOR` 0 and 1; a black light does nothing. Lights 2 and 3 are
    /// black in the game.
    pub lights: [Light; 2],
    /// Ambient reflection colour of the terrain models of an advanced zone.
    pub ambient: Option<[f32; 3]>,
    /// Multiplies character models.
    pub model_diffuse: [f32; 3],
    /// Multiplies sprites.
    pub sprite_diffuse: [f32; 3],
    pub tint: Option<Tint>,
    pub fog: Option<Fog>,
    pub background: Background,
    /// `EDGE_COLOR` with edge marking on (`DISP3DCNT` bit 5), 5-bit channels.
    pub edge: Option<[u8; 3]>,
    /// Colour and alpha of the translucent polygon natives 102/103 draw over the whole 3D
    /// screen (`ov_001::0215b384`, `0207052c`).
    pub cover: Option<[f32; 4]>,
}

impl Default for Environment {
    /// A zone without a lighting script as far as drawing goes: one white light from above,
    /// a flat blue-grey background, no fog.
    fn default() -> Self {
        let up = Vec3::new(0.3, 1.0, 0.5).normalize();
        Self {
            lights: [
                Light {
                    direction: (-up).to_array(),
                    color: [1.0; 3],
                },
                Light {
                    direction: [0.0, -1.0, 0.0],
                    color: [0.0; 3],
                },
            ],
            ambient: None,
            model_diffuse: [1.0; 3],
            sprite_diffuse: [1.0; 3],
            tint: None,
            fog: None,
            background: Background {
                outer: [8, 11, 16],
                horizon: [8, 11, 16],
                center_offset: 0.0,
            },
            edge: Some([6, 4, 4]),
            cover: None,
        }
    }
}

/// BGR555 colour to 5-bit channels.
pub fn rgb5(color: u16) -> [u8; 3] {
    [0, 5, 10].map(|s| (color >> s & 31) as u8)
}

fn unit(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| f32::from(v) / 31.0)
}

/// GPU copy of [`Environment`] for one camera (`Env` of `shader.wgsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct EnvUniform {
    light_dir: [[f32; 4]; 2],
    light_color: [[f32; 4]; 2],
    ambient: [f32; 4],
    model_diffuse: [f32; 4],
    sprite_diffuse: [f32; 4],
    tint_mix: [f32; 4],
    tint_bc: [f32; 4],
    fog_color: [f32; 4],
    fog_params: [f32; 4],
    fog_table: [[f32; 4]; 9],
    eye_back: [f32; 4],
    zplanes: [f32; 4],
}

impl EnvUniform {
    pub(crate) fn new(env: &Environment, camera: &Camera) -> Self {
        let pad = |v: [f32; 3], w: f32| [v[0], v[1], v[2], w];
        let view = camera.view();
        let back = Vec3::new(view.x_axis.z, view.y_axis.z, view.z_axis.z);
        let mut out = Self::zeroed();
        for (i, l) in env.lights.iter().enumerate() {
            out.light_dir[i] = pad(l.direction, 0.0);
            out.light_color[i] = pad(l.color, 0.0);
        }
        out.ambient = pad(
            env.ambient.unwrap_or([0.0; 3]),
            f32::from(u8::from(env.ambient.is_some())),
        );
        out.model_diffuse = pad(env.model_diffuse, 1.0);
        out.sprite_diffuse = pad(env.sprite_diffuse, 1.0);
        if let Some(t) = &env.tint {
            out.tint_mix = pad(t.mix, 1.0);
            out.tint_bc = [t.brightness, t.contrast, 0.0, 0.0];
        }
        if let Some(f) = &env.fog {
            // The 3D engine widens 5-bit channels to 6 bits (`x << 1`, plus one unless 0).
            let wide = f.color.map(|c| {
                if c == 0 {
                    0.0
                } else {
                    f32::from(2 * c + 1) / 63.0
                }
            });
            out.fog_color = pad(wide, f32::from(u8::from(f.alpha_only)));
            // Z-buffer values are 24 bits: the 15-bit register counts in steps of 0x200.
            out.fog_params = [f32::from(f.offset) * 512.0, f32::from(f.shift), 1.0, 0.0];
            // Density at index `i` interpolates entries `i - 1` and `i` of the register table
            // (the engine keeps a copy with the first entry doubled and the last one repeated).
            let mut padded = [0u8; 34];
            padded[0] = f.table[0];
            padded[1..33].copy_from_slice(&f.table);
            padded[33] = f.table[31];
            for (i, d) in padded.iter().enumerate() {
                out.fog_table[i / 4][i % 4] = f32::from(*d);
            }
        }
        out.eye_back = pad(back.to_array(), 0.0);
        out.zplanes = [DS_NEAR, DS_FAR, 0.0, 0.0];
        out
    }
}

/// Colours of the background gradient for `camera` on a viewport of aspect `aspect`:
/// top, horizon and bottom (unit RGB), and the horizon row from the top (`0..=1`).
///
/// `MaybeComputeHorizonPosition` (`0x02052360`) finds the row of a point 2048 units away
/// along the camera's horizontal direction; `DrawBackgroundGradient` (`0x02052420`) then
/// blends the outer colour with the horizon colour around that row.
pub(crate) fn gradient(bg: &Background, camera: &Camera, aspect: f32) -> ([[f32; 3]; 3], f32) {
    let eye = camera.eye();
    let forward = camera.forward();
    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let clip = camera.view_proj(aspect) * (eye + flat * 2048.0).extend(1.0);
    let mut norm = if forward.y >= 0.0 { 1.0 } else { 0.0 };
    if flat != Vec3::ZERO && clip.w > 0.0 {
        let row = (1.0 - clip.y / clip.w) * 0.5 * 192.0;
        if row > 0.0 && row < 192.0 {
            norm = row / 191.0;
        }
    }
    let center = (norm + (4096.0 * bg.center_offset).trunc() / 4096.0).clamp(0.0, 1.0);
    let inner = bg.horizon;
    let delta: [f32; 3] =
        std::array::from_fn(|i| (f32::from(bg.outer[i]) - f32::from(inner[i])) / 0.5);
    let (mut top, mut bottom) = (bg.outer, bg.outer);
    let shorter = if center >= 0.5 { 1.0 - center } else { center };
    let blended: [u8; 3] = std::array::from_fn(|i| {
        (f32::from(inner[i]) + (delta[i] * shorter).trunc()).clamp(0.0, 31.0) as u8
    });
    if center >= 0.5 {
        bottom = blended;
    } else {
        top = blended;
    }
    ([unit(top), unit(inner), unit(bottom)], center)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_split_into_five_bit_channels() {
        assert_eq!(rgb5(0x7fff), [31; 3]);
        assert_eq!(rgb5(0x0466), [6, 3, 1]);
    }

    #[test]
    fn fog_table_is_padded_like_the_engine() {
        let mut table = [0u8; 32];
        table[0] = 5;
        table[31] = 99;
        let env = Environment {
            fog: Some(Fog {
                color: [31, 0, 1],
                alpha_only: false,
                offset: 0x5800,
                shift: 1,
                table,
            }),
            ..Environment::default()
        };
        let u = EnvUniform::new(&env, &Camera::default());
        assert_eq!(u.fog_table[0][0], 5.0);
        assert_eq!(u.fog_table[0][1], 5.0);
        assert_eq!(u.fog_table[8][0], 99.0);
        assert_eq!(u.fog_table[8][1], 99.0);
        assert_eq!(u.fog_params[0], f32::from(0x5800u16) * 512.0);
        assert_eq!(&u.fog_color[..3], &[1.0, 0.0, 3.0 / 63.0]);
    }

    #[test]
    fn gradient_blends_toward_the_horizon_colour() {
        let bg = Background {
            outer: [10, 20, 30],
            horizon: [0, 0, 0],
            center_offset: 0.0,
        };
        let mut camera = Camera::default();
        camera.look_at(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 5.0, -10.0));
        let ([top, mid, bottom], center) = gradient(&bg, &camera, 4.0 / 3.0);
        // The horizon of a level camera is near the middle of the screen.
        assert!((center - 0.5).abs() < 0.05, "{center}");
        assert_eq!(mid, [0.0; 3]);
        // On the horizon row both halves reach the horizon colour at a distance of half a
        // screen: the outer colour fades fully only on the far side.
        assert!(top[2] <= 30.0 / 31.0 && bottom[2] <= 30.0 / 31.0);
        assert!(top == unit(bg.outer) || bottom == unit(bg.outer));
    }
}
