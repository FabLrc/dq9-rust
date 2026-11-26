# Application `dq9-app` and renderer `dq9-render`

The shared native/Web application displays a map, its models, textures, animations, NPCs and the
animated hero (pieces of `chara_pc.gp2` assembled, idle/walk/run depending on speed;
[`re/characters.md`](re/characters.md) § "The hero"). `dq9-core` consumes the `.col2` collisions
([`re/collision.md`](re/collision.md)) for experimental terrain movement;
`dq9_core::field` tests the doors, areas and warps of the `.bmbl` at each step
([`re/map-archive.md`](re/map-archive.md)) and `dq9-app` loads the destination map it
returns; the map's lighting, fog and time of day are applied
([`re/lighting.md`](re/lighting.md)).

- `crates/dq9-core` (no I/O, compiles to wasm): `lighting` (day clock `DayClock`,
  phase `TimeOfDay`, selection and blending of the `.bats` entry, fog registers, light
  vectors); `CollisionWorld` assembles the `.col2` of the
  visible placements; `Hero` advances at the fixed step, snaps to the ground and is blocked by vertical faces.
  Speed and start are provisional ([`re/movement.md`](re/movement.md)).
- `crates/dq9-render` (no I/O, compiles to wasm): `Scene::build(map, file)` remains
  available for a static scene; `AnimatedScene::build(map, id, code, story, file)`
  adds animations and NPCs. `step(camera)` advances by one 60 Hz step;
  `face_camera` and `upload` prepare rendering. `Renderer` holds the GPU resources;
  `Camera` is the free camera.
- `crates/dq9-app` (winit + wgpu): the only crate that touches the system. Reads the ROM (file in
  native, `<input type=file>` on the Web), opens the window, runs the loop: fixed-step 60 Hz
  updates (accumulator), one render per displayed frame.

## Topics

One file per subsystem, each with its commands, options, limits and evidence. The section
anchors moved with the text: a citation of `docs/app.md` § "X" in an older note now names the
file below.

| Topic | File | What it covers |
|---|---|---|
| Running natively | [`app/native.md`](app/native.md) | `cargo run -p dq9-app`, `--map`, `--save`, `--save-out`, `--time`/`--clock`, `--layout`, `--screens`, `--screenshot`, `--walk`, `--input`, conversations, controls, sound. |
| Field menu, shops, inn, church | [`app/field-menu.md`](app/field-menu.md) | The field menu (`I`) and the facility screens, held-item transfer, the equip rule, the core rules and their known limits. |
| Events (`.stb` cutscenes) | [`app/events.md`](app/events.md) | Playing a cutscene (`--stage`, `--event`, `--events-report`), the architecture, the cadence, the camera, captions, fades and the report of the prologue and chapter 2. |
| Battle scene | [`app/battle-scene.md`](app/battle-scene.md) | A stage, the grid and formation, monsters and motions, the battle cameras, the swirl transition and the way back to the field. |
| Battle | [`app/battle.md`](app/battle.md) | The engine (`dq9_core::battle`), the windows and messages, the end of a fight, the wipe-out, encounters, the parity references. |
| New game and automatic run | [`app/new-game.md`](app/new-game.md) | `--new-game`, `--auto`, `--pilot`, `--route`, the checkpoints and input traces, the chain of the story and the driver's limits. |
| Recording a human session | [`app/record.md`](app/record.md) | `--record`, `--play-record`, the `dq9rec` format, the digests, the page's recording and the divergence guard. |
| Running in a browser | [`app/browser.md`](app/browser.md) | The Pages demo, the wasm build and local server, the page's parameters, `pilot=1` route mode and `scripts/web-session.ps1`. |
| Development loop | [`app/dev-loop.md`](app/dev-loop.md) | `verify.ps1` and its steps, the ROM lookup, the per-slot Cargo targets, test times, `nextest` and the README block. |
| Rendering | [`app/rendering.md`](app/rendering.md) | What `dq9-render` does faithfully (instances, textures, materials, passes, camera, hero, NPCs, animations) and what it approximates. |
| DS screens | [`app/screens.md`](app/screens.md) | The two screens, `--screens port\|original`, the layouts, the enhanced 16:9 presentation and its effects. |
| Verification | [`app/verification.md`](app/verification.md) | Native, browser and CI checks with their evidence; what each one does and does not certify. |

## Elsewhere

- [`app-structure.md`](app-structure.md): the module map of `crates/dq9-app`, its extension
  points and the batch that split it.
- [`architecture.md`](architecture.md): the crates, the rules (determinism, traceability) and
  the verification strategy.
- [`build-speed.md`](build-speed.md): the measured Windows build and test times, `rust-lld`
  and the test profile.
- [`status.md`](status.md): the current state of the port; [`re/`](re/): one note per format
  and subsystem.
