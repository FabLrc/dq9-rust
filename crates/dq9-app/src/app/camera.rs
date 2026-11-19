//! The game's camera turn (`L`/`R`), what a map change gives the camera, and the framing view.

use super::App;
use super::STEP;
use dq9_core::camera::{self, FieldCamera};
use dq9_core::fx::reduce_angle;
use dq9_formats::Table;
use dq9_formats::map::{ZoneInfo, rom_file};
use dq9_formats::maplist;
use dq9_render::Camera;
use winit::keyboard::KeyCode;

/// The field camera and what frames it.
pub(super) struct View {
    pub(super) camera: Camera,
    /// The game's camera turn (`L`/`R`).
    pub(super) field_camera: FieldCamera,
    /// The application's yaw of the direction the camera returns to (`FieldCamera+0x246`):
    /// the map's home yaw, set on every map change.
    pub(super) camera_home: f32,
    /// Milliseconds of fixed steps not yet given to the camera turn.
    pub(super) clock_ms: f32,
    pub(super) framed: Option<[glam::Vec3; 2]>,
}

/// The application's yaw for a game yaw (`fx32` radians): the game's turns the other way
/// round.
pub(super) fn app_yaw(game: i32) -> f32 {
    (-(game as f32) / 4096.0).rem_euclid(std::f32::consts::TAU)
}

/// The game's yaw (`fx32`, `0..=0x6488`) of an application yaw.
fn game_yaw(app: f32) -> i32 {
    reduce_angle((-app * 4096.0).round() as i32)
}

/// `Zone3D+0x40` of map `id`, the world angle the zone settings take from the map list
/// (`0x0201c60c`); 0 when the ROM or its list cannot be read.
fn world_angle(rom: &[u8], id: u16) -> u16 {
    let mut zone = ZoneInfo::default();
    if let Ok(parsed) = nds_rom::Rom::parse(rom)
        && let Some(file) = rom_file(&parsed, maplist::PATH)
        && let Ok(table) = Table::parse(file)
    {
        let _ = zone.load_map_list(&table, id);
    }
    zone.world_angle
}

impl View {
    pub(super) fn new() -> Self {
        Self {
            camera: Camera::default(),
            field_camera: FieldCamera::default(),
            camera_home: 0.0,
            clock_ms: 0.0,
            framed: None,
        }
    }
}

/// The orbit camera of a map just entered (`ov_017::0219e384`, the map load task, run for
/// every warp, door and event change of map but a same-place reload) and its home yaw: the
/// elevation, distance and field of view of the record's framing ([`camera::framing`]), the
/// yaw at the map's home yaw ([`camera::home_yaw`]) at once, not the hero's heading. `L` + `R`
/// return to the home yaw.
pub(super) fn arrival_camera(rom: &[u8], hero: glam::Vec3, byte_c: u8, map: u16) -> (Camera, f32) {
    let home = app_yaw(camera::home_yaw(world_angle(rom, map)));
    let framing = camera::framing(byte_c);
    let mut camera = Camera::field(hero, home);
    camera.pitch = (framing.dy as f32 / framing.distance as f32).asin();
    camera.distance = framing.distance as f32 / 4096.0;
    camera.fovy = 2.0 * (framing.half_fov as f32 / 4096.0).to_radians();
    (camera, home)
}

impl App {
    /// The game's camera turn (`FieldCamera_Update`): `Q`/`E` are its `L`/`R` buttons. The yaw
    /// turns the other way round from the application's, and `L` + `R` return it to the map's
    /// home yaw.
    pub(super) fn turn_camera(&mut self) {
        let left = self.controls.held.contains(&KeyCode::KeyQ);
        let right = self.controls.held.contains(&KeyCode::KeyE);
        if !(left || right || self.view.field_camera.is_returning()) {
            return;
        }
        // Whole milliseconds of the fixed steps, as the game's delta time is.
        self.view.clock_ms += STEP * 1000.0;
        let delta = self.view.clock_ms.floor();
        self.view.clock_ms -= delta;
        let yaw = self.view.field_camera.step(
            game_yaw(self.view.camera.yaw),
            game_yaw(self.view.camera_home),
            left,
            right,
            delta as u32,
        );
        self.view.camera.yaw = app_yaw(yaw);
    }

    /// Replaces the framing view.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn look_at(&mut self, eye: glam::Vec3, target: glam::Vec3) {
        self.view.camera.look_at(eye, target);
    }
}

#[cfg(all(test, windows))]
mod tests;
