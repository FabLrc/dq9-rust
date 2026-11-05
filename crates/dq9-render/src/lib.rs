//! Map renderer: turns an assembled map ([`dq9_formats::map::Map`]) into GPU resources and
//! draws it with wgpu, close to the DS 3D engine. No I/O: the caller passes ROM files by path,
//! and the crate builds for `wasm32-unknown-unknown`.
//!
//! - [`scene`]: CPU side ([`Scene::build`]): decoded models, textures and placed instances.
//! - [`renderer`]: GPU side ([`Renderer`]): buffers, textures, pipelines, opaque and sorted
//!   translucent passes; vertex buffers and material bindings can be updated per frame.
//! - [`camera`]: free orbit/fly camera with the game's default projection.
//!
//! What is approximated: `docs/app.md`.

pub mod animation;
pub mod camera;
mod pose;
pub mod renderer;
pub mod scene;

pub use animation::AnimatedScene;
pub use camera::{Camera, CameraInput, CameraMode};
pub use renderer::{Renderer, Viewport};
pub use scene::{Scene, SceneInstance, SceneMaterial, SceneModel, SceneTexture, SceneVertex};
