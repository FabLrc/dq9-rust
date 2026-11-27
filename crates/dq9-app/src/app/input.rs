//! Keyboard and pointer input of the window (and of a replayed record): the key map, the held
//! keys, the d-pad bits a step reads.

use super::App;
use super::fight;
use crate::record::Input as Logged;
use crate::screens;
use crate::screens::ScreenMode;
use crate::talk::Button;
use dq9_core::Input;
use dq9_core::state::TimeOfDay;
use dq9_render::Camera;
use dq9_render::CameraMode;
use glam::Vec2;
use glam::Vec3;
use std::collections::HashSet;
use winit::keyboard::KeyCode;

/// The d-pad bits of an [`Input`] (bit 0 up, 1 down, 2 left, 3 right).
pub(super) fn input_bits(input: Input) -> u8 {
    u8::from(input.up)
        | u8::from(input.down) << 1
        | u8::from(input.left) << 2
        | u8::from(input.right) << 3
}

/// The d-pad keys of a replay frame (bit 0 up, 1 down, 2 left, 3 right).
pub(super) fn pad_input(bits: u8) -> Input {
    Input {
        up: bits & 1 != 0,
        down: bits & 2 != 0,
        left: bits & 4 != 0,
        right: bits & 8 != 0,
    }
}

/// The world direction the d-pad asks for with the game's camera at yaw 0, diagonal first as
/// `Hero_ReadDirectionInput` does.
pub(super) fn pad_direction(bits: u8) -> Vec3 {
    let (up, down, left, right) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
    let direction = match (up, down, left, right) {
        (true, _, true, _) => Vec3::new(-1.0, 0.0, -1.0),
        (true, _, _, true) => Vec3::new(1.0, 0.0, -1.0),
        (_, true, true, _) => Vec3::new(-1.0, 0.0, 1.0),
        (_, true, _, true) => Vec3::new(1.0, 0.0, 1.0),
        (true, _, _, _) => Vec3::NEG_Z,
        (_, true, _, _) => Vec3::Z,
        (_, _, true, _) => Vec3::NEG_X,
        (_, _, _, true) => Vec3::X,
        _ => Vec3::ZERO,
    };
    direction.normalize_or_zero()
}

/// The message-window button a key presses, if any. The window's keys are the DS buttons:
/// `Enter`/`Space`/`Z` (A), `Backspace`/`X` (B) and the arrows or `W`/`S` (the d-pad's
/// up/down, the prompt's cursor).
fn talk_button(code: KeyCode) -> Option<Button> {
    Some(match code {
        KeyCode::Enter | KeyCode::Space | KeyCode::KeyZ => Button::Confirm,
        KeyCode::Backspace | KeyCode::KeyX => Button::Cancel,
        KeyCode::ArrowUp | KeyCode::KeyW => Button::Up,
        KeyCode::ArrowDown | KeyCode::KeyS => Button::Down,
        _ => return None,
    })
}

/// Whether a key only leaves a message wait: DS X (`C`), L (`Q`), R (`E`) and the left/right
/// d-pad. A wait-for-key state takes A, B, X, L, R and all four d-pad keys
/// (`MsgWin_PollWaitKey` `0x020440dc`: mask `0x403`, L/R `0x200`/`0x100` set by
/// `MsgWin_Show` `0x0204501c`, d-pad `0xf0`); Y, Start and Select do nothing. The DS buttons
/// absent from the port (Y, Start, Select) map to no key.
fn talk_advance(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::KeyC
            | KeyCode::KeyQ
            | KeyCode::KeyE
            | KeyCode::ArrowLeft
            | KeyCode::KeyA
            | KeyCode::ArrowRight
            | KeyCode::KeyD
    )
}

/// The window's held keys and pointer.
pub(super) struct Controls {
    pub(super) held: HashSet<KeyCode>,
    pub(super) cursor: Option<(f64, f64)>,
    pub(super) dragging: bool,
    /// Where the left button went down, while it is held: the tap's start.
    press: Option<(f64, f64)>,
    /// How far the pointer travelled since the press, in window pixels: past [`TAP_SLOP`] the
    /// release is the camera gesture, not a touch.
    travel: f64,
    pub(super) rotate: Vec2,
    pub(super) zoom: f32,
    /// The type-1 door task holds the field: the pad and the camera axes ignore the keys.
    pub(super) suppress: bool,
}

/// How far, in window pixels, the pointer may travel from the press and still count as a tap on
/// the touch screen (the path length): the camera gesture is a drag, and a tap must not fire
/// after it (`docs/app/screens.md`, § Touch).
const TAP_SLOP: f64 = 8.0;

impl Controls {
    pub(super) fn new() -> Self {
        Self {
            held: HashSet::new(),
            cursor: None,
            dragging: false,
            press: None,
            travel: 0.0,
            rotate: Vec2::ZERO,
            zoom: 0.0,
            suppress: false,
        }
    }

    /// The left button went down at the pointer: the potential tap starts here (the last
    /// position the window reported; `winit`'s button events carry none, so a click before any
    /// pointer motion has no tap).
    pub(super) fn begin_press(&mut self) {
        self.press = self.cursor;
        self.travel = 0.0;
    }

    /// The pointer moved while the button is held: the distance of the camera gesture.
    pub(super) fn travel_to(&mut self, from: (f64, f64), to: (f64, f64)) {
        self.travel += ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
    }

    /// The left button went up: the press's start when the pointer travelled at most
    /// [`TAP_SLOP`] (a tap), `None` after a drag, without a press, or before the window reported
    /// any pointer position.
    pub(super) fn finish_press(&mut self) -> Option<(f64, f64)> {
        let press = self.press.take();
        let travel = std::mem::take(&mut self.travel);
        press.filter(|_| travel <= TAP_SLOP)
    }
}

impl App {
    /// Handles a key of the window or of a replay; `true` asks the window to close.
    pub(super) fn key(&mut self, code: KeyCode, pressed: bool) -> bool {
        if !pressed {
            if self.controls.held.remove(&code) {
                self.log_key(code, false);
            }
            return false;
        }
        if !self.controls.held.insert(code) {
            return false; // key repeat
        }
        self.log_key(code, true);
        #[cfg(not(target_arch = "wasm32"))]
        if self.creation.is_some() {
            // The creation screen takes every key, the name's letters included.
            return self.creation_key(code);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.title.is_some() {
            // The title takes every key: it steps nothing else.
            return self.title_key(code);
        }
        if self.combat.fight.is_some() {
            // The fight reads the pad itself; the conversation's presses must not pile up.
            match code {
                KeyCode::KeyM => self.audio.toggle_mute(),
                KeyCode::KeyR => self.set_layout(self.options.layout.next()),
                _ => fight::pad_press(&mut self.combat.battle_pad, code),
            }
            return false;
        }
        if self.services.is_open() {
            match code {
                KeyCode::KeyM => self.audio.toggle_mute(),
                KeyCode::KeyR => self.set_layout(self.options.layout.next()),
                KeyCode::Enter | KeyCode::Space | KeyCode::KeyZ => {
                    self.service_button(Button::Confirm);
                }
                KeyCode::Backspace | KeyCode::KeyX | KeyCode::Escape => {
                    self.service_button(Button::Cancel);
                }
                KeyCode::ArrowUp | KeyCode::KeyW => self.service_button(Button::Up),
                KeyCode::ArrowDown | KeyCode::KeyS => self.service_button(Button::Down),
                KeyCode::ArrowLeft | KeyCode::KeyA => self.service_side(false),
                KeyCode::ArrowRight | KeyCode::KeyD => self.service_side(true),
                KeyCode::KeyH => self.service_hold(),
                KeyCode::KeyT => self.service_take(),
                _ => {}
            }
            return false;
        }
        if code == KeyCode::KeyI
            && !self.talk.is_active()
            && !self.battle_is_active()
            && !self.events.is_active()
            && self.events.pending_start().is_none()
            && self.open_field_menu()
        {
            return false;
        }
        if self.talk.is_active() {
            if let Some(button) = talk_button(code) {
                self.talk.press(button);
            } else if talk_advance(code) {
                self.talk.advance();
            }
        }
        match code {
            KeyCode::Enter | KeyCode::Space | KeyCode::KeyZ => self.talk.press(Button::Confirm),
            KeyCode::Backspace | KeyCode::KeyX => self.talk.press(Button::Cancel),
            KeyCode::Escape => {
                // From the field Escape ends the session: the native window closes. The page
                // has no window to close, and its loop would stop with the recording
                // (`docs/app/record.md`, § "Recording a human session"): Escape does nothing there.
                return !cfg!(target_arch = "wasm32");
            }
            KeyCode::Tab => {
                self.view.camera.mode = match self.view.camera.mode {
                    CameraMode::Orbit => CameraMode::Fly,
                    CameraMode::Fly => CameraMode::Orbit,
                };
                self.update_title();
            }
            KeyCode::KeyF => {
                if self.view.camera.mode == CameraMode::Orbit {
                    if let Some(hero) = &self.hero {
                        self.view.camera = Camera::field(hero.position(), self.view.camera_home);
                    }
                } else if let Some(bounds) = self.view.framed {
                    self.view.camera.frame(bounds);
                }
            }
            KeyCode::KeyM => self.audio.toggle_mute(),
            KeyCode::KeyR => self.set_layout(self.options.layout.next()),
            KeyCode::KeyT => {
                let next = match self.day.phase() {
                    TimeOfDay::Night => TimeOfDay::Morning,
                    TimeOfDay::Morning => TimeOfDay::Day,
                    TimeOfDay::Day => TimeOfDay::Evening,
                    TimeOfDay::Evening => TimeOfDay::Night,
                };
                self.set_time(next);
            }
            KeyCode::PageDown => self.cycle_map(1),
            KeyCode::PageUp => self.cycle_map(-1),
            _ => {}
        }
        false
    }

    pub(super) fn axis(&self, plus: &[KeyCode], minus: &[KeyCode]) -> f32 {
        if self.controls.suppress {
            return 0.0;
        }
        let any = |keys: &[KeyCode]| keys.iter().any(|k| self.controls.held.contains(k));
        f32::from(i8::from(any(plus)) - i8::from(any(minus)))
    }

    /// A mouse drag (camera yaw, pitch) and wheel, as the window hands them to the camera.
    pub(super) fn mouse(&mut self, rotate: Vec2, zoom: f32) {
        self.controls.rotate += rotate;
        self.controls.zoom += zoom;
        self.log_input(Logged::Mouse {
            rotate: rotate.to_array(),
            zoom,
        });
    }

    /// The window lost focus: the held keys are forgotten.
    pub(super) fn focus_lost(&mut self) {
        self.controls.held.clear();
        self.log_input(Logged::FocusLost);
    }

    /// A tap on the touch screen: a left press released after at most [`TAP_SLOP`] of pointer
    /// travel, at `point` in window pixels, in a `window` (`docs/app/screens.md`, § Touch).
    ///
    /// The point is mapped to the 256×192 DS screen the field draws on for the layout, the
    /// screen mode and the script's swap ([`Screens::field_pointer`]); a press on the other LCD
    /// or the letterbox does nothing. On the field's screen: an open conversation takes the tap
    /// as the action button (the original's bottom-half capture advances on a tap outside the
    /// window's own rectangle too), and otherwise a tap on the "X Menu" rectangle
    /// ([`screens::menu_affordance`], the measured label, **[hypothesis]**) opens the field menu
    /// under the key `I`'s conditions. Nothing else: the original's other touch handlers (walk
    /// and climb by drag, pick up a spot, steer the ship) are not established in this port.
    ///
    /// A recorded session replays keys and camera deltas only (`record::Input`), so a tap would
    /// move the game without appearing in the record: it is ignored while recording or
    /// replaying.
    pub(super) fn tap(&mut self, point: (f64, f64), window: (u32, u32)) {
        if self.recording.replay.is_some() || self.recording.recorder.is_some() {
            return;
        }
        let screens = screens::screens(self.options.layout, window.0, window.1);
        #[cfg(not(target_arch = "wasm32"))]
        if self.title.is_some() {
            // The title maps the pointer to the 256×192 screen it draws on; a tap on a row
            // chooses it.
            if let Some(at) = screens.field_pointer(self.presentation.screens, false, point) {
                self.title_tap(at);
            }
            return;
        }
        // A battle keeps the port's assignment, which is the original's for a battle
        // (`re/bottom-screen.md` § 3): the tap would reach the same rectangle either way.
        let mode = if self.encounter.battle.is_some() {
            ScreenMode::Port
        } else {
            self.presentation.screens
        };
        let swapped = self.events.screen_swapped();
        let Some(on_screen) = screens.field_pointer(mode, swapped, point) else {
            return;
        };
        if self.encounter.battle.is_some() || self.combat.fight.is_some() || self.services.is_open()
        {
            // A battle scene, a played fight and a facility screen route no tap: the command
            // windows they draw are not touch-driven in the port (the original's overlay 23
            // callers are not modelled), and the field menu must not open over them.
            return;
        }
        if self.talk.is_active() {
            // The touch acts as the action button on the message window **[hypothesis]**: the
            // capture `field-church-tap-bottom.png` shows a tap advancing a plain line; which
            // row a tap takes on a prompt is not established, so the touched row is not read.
            self.talk.press(Button::Confirm);
            return;
        }
        if self.battle_is_active()
            || self.events.is_active()
            || self.events.pending_start().is_some()
            || !screens::menu_affordance(on_screen)
        {
            return;
        }
        self.open_field_menu();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys a message wait accepts, measured on the original.
    const WAIT_KEYS: [(KeyCode, &str); 16] = [
        (KeyCode::Enter, "A"),
        (KeyCode::Space, "A"),
        (KeyCode::KeyZ, "A"),
        (KeyCode::Backspace, "B"),
        (KeyCode::KeyX, "B"),
        (KeyCode::KeyC, "X"),
        (KeyCode::KeyQ, "L"),
        (KeyCode::KeyE, "R"),
        (KeyCode::ArrowUp, "d-pad up"),
        (KeyCode::KeyW, "d-pad up"),
        (KeyCode::ArrowDown, "d-pad down"),
        (KeyCode::KeyS, "d-pad down"),
        (KeyCode::ArrowLeft, "d-pad left"),
        (KeyCode::KeyA, "d-pad left"),
        (KeyCode::ArrowRight, "d-pad right"),
        (KeyCode::KeyD, "d-pad right"),
    ];

    /// DS Y, Start and Select have no key in the port; they must reach nothing.
    const INERT_KEYS: [(KeyCode, &str); 3] = [
        (KeyCode::KeyV, "Y"),
        (KeyCode::F1, "Start"),
        (KeyCode::F2, "Select"),
    ];

    /// The keys a message wait accepts: A, B, X, L, R and the four d-pad keys leave a wait;
    /// Y, Start and Select do nothing (`MsgWin_PollWaitKey` `0x020440dc`: mask `0x403` plus
    /// `0x200`/`0x100` for L/R and `0xf0` for the d-pad).
    #[test]
    fn only_the_measured_keys_leave_a_message_wait() {
        for (code, name) in WAIT_KEYS {
            assert!(
                talk_button(code).is_some() || talk_advance(code),
                "{name} ({code:?}) leaves a wait"
            );
        }
        // The closed matches cannot add a key silently.
        for (code, name) in INERT_KEYS {
            assert!(
                talk_button(code).is_none() && !talk_advance(code),
                "{name} ({code:?}) does nothing in a wait"
            );
        }
    }

    /// Prints the key table of `docs/re/dialogue.md` § "Message pacing"
    /// (`cargo test -p dq9-app print_key_table -- --ignored --nocapture`).
    #[test]
    #[ignore = "prints the measured table, asserts nothing"]
    fn print_key_table() {
        for (code, name) in WAIT_KEYS.into_iter().chain(INERT_KEYS) {
            let button = talk_button(code);
            println!(
                "{name:<12} {code:?} -> button {button:?}, leaves a wait {}",
                button.is_some() || talk_advance(code)
            );
        }
    }

    /// The prompt keeps its own keys: A confirms, B answers the second choice and up/down move
    /// the cursor; the d-pad's left/right and X/L/R only leave a wait.
    #[test]
    fn the_prompt_keys_keep_their_meaning() {
        assert_eq!(talk_button(KeyCode::Enter), Some(Button::Confirm));
        assert_eq!(talk_button(KeyCode::Backspace), Some(Button::Cancel));
        assert_eq!(talk_button(KeyCode::ArrowUp), Some(Button::Up));
        assert_eq!(talk_button(KeyCode::ArrowDown), Some(Button::Down));
        for code in [KeyCode::ArrowLeft, KeyCode::ArrowRight, KeyCode::KeyC] {
            assert!(talk_button(code).is_none());
            assert!(talk_advance(code), "{code:?} only leaves a wait");
        }
    }

    /// The camera gesture and the touch screen share the left button: a press released where it
    /// went down is a tap, one released after a drag is not (`docs/app/screens.md`, § Touch).
    #[test]
    fn a_press_released_in_place_is_a_tap_and_a_drag_is_not() {
        let mut controls = Controls::new();
        controls.cursor = Some((100.0, 100.0));
        controls.begin_press();
        controls.travel_to((100.0, 100.0), (102.0, 103.0));
        assert_eq!(
            controls.finish_press(),
            Some((100.0, 100.0)),
            "a small jitter stays a tap"
        );
        assert_eq!(controls.finish_press(), None, "one release per press");

        controls.cursor = Some((400.0, 300.0));
        controls.begin_press();
        controls.travel_to((400.0, 300.0), (430.0, 300.0));
        assert_eq!(
            controls.finish_press(),
            None,
            "the drag rotates the camera and must not also tap"
        );

        // The slop is inclusive, and a release the window never saw go down is not a tap.
        controls.cursor = Some((10.0, 10.0));
        controls.begin_press();
        controls.travel_to((10.0, 10.0), (18.0, 10.0));
        assert_eq!(controls.finish_press(), Some((10.0, 10.0)), "8 px = slop");
        controls.begin_press();
        controls.travel_to((10.0, 10.0), (10.0, 18.5));
        assert_eq!(controls.finish_press(), None, "8.5 px is a drag");
        assert_eq!(controls.finish_press(), None);

        // A press the window saw before any pointer motion has no position to tap: no tap
        // (winit's button events carry none, and guessing one would tap the wrong place).
        let mut cold = Controls::new();
        cold.begin_press();
        assert_eq!(cold.finish_press(), None);
    }

    use crate::app::Options;

    /// The start options of a headless run on an explicit map and story point (`--map`,
    /// `--story`, `--new-game`), in the enhanced layout: the same as `app::test_support`'s
    /// `map_options`, which compiles on Windows only, so that the touch tests also build on
    /// CI's Linux test run (they skip themselves without a ROM).
    fn options(map: Option<&str>, story: (u8, u8, u8, bool), new_game: bool) -> Options {
        Options {
            map: map.map(Into::into),
            save: None,
            time: None,
            clock: None,
            layout: crate::screens::Layout::Enhanced,
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story {
                chapter: story.0,
                step: story.1,
                substep: story.2,
                daytime: story.3,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game,
            battle: None,
            play: Default::default(),
        }
    }

    /// A DS screen point of the enhanced layout, in the 1920×1080 window these tests tap.
    fn at(x: f32, y: f32) -> (f64, f64) {
        (f64::from(x) * 1920.0 / 256.0, f64::from(y) * 1080.0 / 192.0)
    }

    /// The "X Menu" rectangle opens the field menu, under the same conditions as the key `I`
    /// (`open_field_menu`); any other point of the field does nothing.
    #[test]
    fn a_tap_on_the_menu_affordance_opens_the_field_menu() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(
            options(Some("M01"), (1, 1, 1, true), false),
            Some(rom.to_vec()),
        );
        assert!(
            !app.events.is_active() && app.events.pending_start().is_none(),
            "M01 at 1.1.1 must be free for the menu"
        );
        assert!(!app.services.is_open());
        app.tap(at(60.0, 100.0), (1920, 1080));
        assert!(
            !app.services.is_open(),
            "a tap on the 3D view opens nothing"
        );
        app.tap(at(228.0, 31.0), (1920, 1080));
        assert!(
            app.services.is_open(),
            "the X Menu rectangle opened the field menu"
        );
        // While the menu holds the field, a tap on the affordance does not stack another.
        app.tap(at(228.0, 31.0), (1920, 1080));
        assert!(app.services.is_open());
    }

    /// The Observatory's NPC 130 on `X01M07` at 1.1.1, the conversation the message-flow tests
    /// use: a real line from the ROM, opened directly and run to its first wait for the key.
    fn observatory_conversation(rom: &[u8]) -> App {
        let mut app = App::new_headless(
            options(Some("X01M07"), (1, 1, 1, true), false),
            Some(rom.to_vec()),
        );
        app.talk
            .start(rom, "X01M07", &mut app.state, 130, 0)
            .expect("NPC 130 of X01M07 speaks");
        assert!(app.talk.is_active(), "the conversation is open");
        for _ in 0..400 {
            app.run_steps(1);
            if let Some((_, what)) = app.talk.trace.last()
                && what == "window shows: arrow"
            {
                return app;
            }
        }
        panic!(
            "the line reaches no wait for the key: {:?}",
            app.talk.trace.last()
        );
    }

    /// A synthetic tap on the field advances a real conversation line: on the `X01M07` line it
    /// has the action button's effect (the tap and an `Enter` press leave the same trace) and
    /// without it the line keeps waiting (`talk::trace`, the message-flow probe).
    #[test]
    fn a_synthetic_tap_advances_a_real_conversation_line() {
        let Some(rom) = testrom::load() else { return };
        let mut pressed = observatory_conversation(rom);
        let mut tapped = observatory_conversation(rom);
        let waiting = tapped.talk.trace.clone();
        pressed.talk.press(Button::Confirm);
        // The field's screen centre in the 1920×1080 window: DS (128, 96).
        tapped.tap((960.0, 540.0), (1920, 1080));
        for _ in 0..120 {
            pressed.run_steps(1);
            tapped.run_steps(1);
        }
        assert_eq!(
            tapped.talk.trace, pressed.talk.trace,
            "the tap must act as the message window's action button"
        );
        assert_ne!(
            tapped.talk.trace, waiting,
            "an untouched line keeps its wait: the tap must have moved it"
        );
    }

    /// A tap acts as the action button to the end of a line too: repeated taps read the
    /// `X01M07` conversation to its yes/no, answer it and close the window, as the key does.
    #[test]
    fn taps_read_the_line_and_close_the_conversation() {
        let Some(rom) = testrom::load() else { return };
        let mut app = observatory_conversation(rom);
        for _ in 0..600 {
            if !app.talk.is_active() {
                break;
            }
            // DS (128, 96) in the 1920×1080 enhanced window: the field's screen.
            app.tap((960.0, 540.0), (1920, 1080));
            app.run_steps(20);
        }
        assert!(
            !app.talk.is_active(),
            "the taps read the line to its end and closed the window"
        );
        assert!(
            app.talk
                .trace
                .iter()
                .any(|(_, what)| what.starts_with("window closed")),
            "the window closed by the tap's action edge: {:?}",
            app.talk.trace.last()
        );
    }

    /// Prints the trace of the `X01M07` conversation around a synthetic tap, for the batch's
    /// evidence (`cargo test -p dq9-app print_tap_trace -- --ignored --nocapture`).
    #[test]
    #[ignore = "prints the probe, asserts nothing"]
    fn print_tap_trace() {
        let Some(rom) = testrom::load() else { return };
        let mut app = observatory_conversation(rom);
        let before = app.talk.trace.len();
        println!(
            "before the tap (DS 128,96 in a 1920×1080 enhanced window): {:?}",
            &app.talk.trace[before.saturating_sub(3)..]
        );
        app.tap((960.0, 540.0), (1920, 1080));
        for step in 0..120 {
            let len = app.talk.trace.len();
            app.run_steps(1);
            if app.talk.trace.len() > len {
                println!("after {} step(s): {:?}", step + 1, &app.talk.trace[len..]);
            }
        }
        println!("window open: {}", app.talk.is_active());
    }
}
