//! Web entry point. The player picks the ROM with the page's `<input type=file id=rom>`: it is
//! read in the browser and never uploaded or fetched. The map comes from the URL query
//! (`?map=M01&time=0&layout=side`), the canvas is `#dq9`, status lines go to `#status`.

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::WindowAttributes;

use crate::app::{App, Options, SessionSpec, UserEvent};
use crate::screens::{Layout, ScreenMode};
use dq9_core::talk::TextSpeed;
use dq9_render::EnhancedOptions;

thread_local! {
    /// Whether the page has queued the ROM (`#rom`): a record picked afterwards replays at
    /// once instead of waiting for a ROM (`web.rs`, `watch_record_input`).
    static ROM_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether this page records its session (`?record=1`): a record picked then would start
    /// the page's session over under another file's options, so it is refused.
    static RECORDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

fn canvas() -> Option<web_sys::HtmlCanvasElement> {
    document()?
        .get_element_by_id("dq9")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()
}

/// Gives the keyboard to the canvas: winit only listens for keys on a focused canvas, so a
/// click on a page control would otherwise leave the game unresponsive.
fn focus_canvas() {
    if let Some(canvas) = canvas() {
        let _ = canvas.focus();
    }
}

fn set_status(text: &str, kind: &str) {
    if let Some(el) = document().and_then(|d| d.get_element_by_id("status")) {
        el.set_text_content(Some(text));
        let _ = el.set_attribute("data-kind", kind);
    }
}

/// Enables or disables a page control without pulling in `HtmlButtonElement`.
fn set_disabled(id: &str, disabled: bool) {
    if let Some(el) = document().and_then(|d| d.get_element_by_id(id)) {
        if disabled {
            let _ = el.set_attribute("disabled", "");
        } else {
            let _ = el.remove_attribute("disabled");
        }
    }
}

/// Engine errors that the page presents itself.
enum Notice {
    Rom,
    Gpu,
    Window,
}

/// The player-facing form of an engine message: the page is French, the console keeps the
/// engine's English text. `None` leaves the message unchanged.
fn player_text(message: &str) -> Option<(Notice, String)> {
    if message.starts_with("not a Dragon Quest IX ROM: ") {
        // The parser's detail (offsets, lengths) cannot help the player; the console keeps it.
        return Some((
            Notice::Rom,
            "This file is not a European Dragon Quest IX ROM (YDQP01).".to_owned(),
        ));
    }
    if let Some(detail) = message.strip_prefix("GPU: ") {
        return Some((
            Notice::Gpu,
            format!("The graphics engine could not start (WebGPU or WebGL 2 required): {detail}"),
        ));
    }
    if let Some(detail) = message.strip_prefix("cannot create the window: ") {
        return Some((
            Notice::Window,
            format!("The game window could not be created: {detail}"),
        ));
    }
    None
}

/// Shows a status line; engine errors that the player can act on are shown in plain words and
/// in the error style, and an unusable ROM disables "New game" again. Engine progress
/// lines do not replace an error the player has not acknowledged by picking a file or
/// starting a game.
pub fn status(message: &str) {
    web_sys::console::log_1(&message.into());
    match player_text(message) {
        Some((notice, text)) => {
            set_status(&text, "error");
            match notice {
                Notice::Rom => set_disabled("newgame", true),
                Notice::Gpu | Notice::Window => {
                    for id in ["rom", "save", "time", "layout", "screens", "fx", "newgame"] {
                        set_disabled(id, true);
                    }
                }
            }
        }
        None => {
            let showing_error = document()
                .and_then(|d| d.get_element_by_id("status"))
                .is_some_and(|el| el.get_attribute("data-kind").as_deref() == Some("error"));
            if !showing_error {
                set_status(message, "info");
            }
        }
    }
}

/// A page message about the player's own action: it replaces a previous error.
fn announce(message: &str) {
    web_sys::console::log_1(&message.into());
    set_status(message, "info");
}

/// Shows a web-layer error (file read, missing control, engine import) in the error style.
pub fn error(message: &str) {
    web_sys::console::error_1(&message.into());
    set_status(message, "error");
}

/// The pilot session's progress line: its own `#session` element when the page has one (the
/// engine's status lines keep replacing `#status`), else `#status`.
pub fn session_line(text: &str) {
    match document().and_then(|d| d.get_element_by_id("session")) {
        Some(el) => {
            el.set_text_content(Some(text));
            let _ = el.remove_attribute("hidden");
        }
        None => set_status(text, "info"),
    }
}

/// Draws into the page's `#dq9` canvas (a new one is appended when absent).
pub fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    let canvas = canvas();
    let append = canvas.is_none();
    attributes
        .with_canvas(canvas)
        .with_append(append)
        .with_prevent_default(true)
}

/// Canvas backing size in physical pixels, independent of CSS layout.
pub fn canvas_size() -> Option<(u32, u32)> {
    let window = web_sys::window()?;
    let canvas = canvas()?;
    let dpr = window.device_pixel_ratio();
    Some((
        (f64::from(canvas.client_width()) * dpr).round() as u32,
        (f64::from(canvas.client_height()) * dpr).round() as u32,
    ))
}

/// What the page's `record` and `play-record` parameters asked for (`docs/app.md`,
/// § Recording a human session).
#[derive(Debug, Clone, Copy, Default)]
struct PageMode {
    /// `?record=1`: the session is recorded from its first step (`UserEvent::Record`).
    record: bool,
    /// `?play-record=1`: the page expects a `.dq9rec` to replay (`UserEvent::PlayRecord`).
    play_record: bool,
}

type PageOptions = (
    Options,
    Option<SessionSpec>,
    ScreenMode,
    EnhancedOptions,
    TextSpeed,
    PageMode,
);

/// `map`, `time` (`night`, `morning`, `day`, `evening` or 0-3), `clock` (second of the day),
/// `layout` (`enhanced`, `enhanced-reactive`, `stacked`, `side` or `top`), `fx`/`nofx`
/// (comma-separated effects to switch on/off: `trilinear`, `fxaa`, `contact-shadows`,
/// `bloom`, `tonemapping`, `grading`, `vignette`, `dithering`, `edge-accent`, `glass`, or
/// `all`/`none`), `screens` (`port` or `original`: where
/// the field and its message window are drawn), `textspeed` (`slow`, `normal` or `fast`: how
/// fast dialogue text types, `normal` being the original's rate), `mute`, `battle` (an
/// `eventbattle.bin` index),
/// `battle-stage` and `monsters` (`1x3`), `seed` (the battle generators' seed) from the page's
/// query string. The pilot session adds `pilot` (the autopilot plays the new game), `route`
/// (an event number: the route mode of `--route`, which implies `pilot` and `fight`), `fight`
/// (the engine plays the battles the native headless run settles), `speed` (fixed steps per
/// animation frame; real time without it), `budget` (milliseconds of steps per frame, 40 by
/// default), `render=0` (no frame is drawn) and `battles=show` (battles are presented instead
/// of settled in the step that starts them). Recording adds `record=1` (the session is
/// recorded from its first step, with seed 7 when `seed` is absent: a record must be
/// replayable) and `play-record=1` (a `.dq9rec` picked in the page is replayed instead of a
/// session being played).
fn options() -> PageOptions {
    let mut fx = EnhancedOptions::DEFAULT;
    let mut mode = PageMode::default();
    let mut options = Options {
        map: None,
        save: None,
        time: None,
        clock: None,
        mute: false,
        record_sound: false,
        trace_inputs: false,
        event: None,
        layout: Layout::default(),
        new_game: false,
        battle: None,
        play: Default::default(),
        story_override: [None; 3],
        story: dq9_formats::cast::Story {
            chapter: 1,
            step: 1,
            substep: 1,
            daytime: true,
            ..dq9_formats::cast::Story::default()
        },
    };
    let mut session = SessionSpec {
        route: None,
        speed: None,
        budget_ms: 40,
        render: true,
        show_battles: false,
    };
    let mut pilot = false;
    let mut screens = ScreenMode::default();
    let mut text_speed = TextSpeed::default();
    let (mut fx_on, mut fx_off) = (String::new(), String::new());
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    for pair in search.trim_start_matches('?').split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "map" if !value.is_empty() => options.map = Some(value.to_owned()),
            "time" => options.time = crate::lighting::parse_time(value).ok(),
            "clock" => options.clock = value.parse().ok(),
            "mute" => options.mute = value != "0",
            "layout" => options.layout = Layout::parse(value).unwrap_or(options.layout),
            "fx" => fx_on = value.to_owned(),
            "nofx" => fx_off = value.to_owned(),
            "screens" => screens = ScreenMode::parse(value).unwrap_or(screens),
            "textspeed" => text_speed = TextSpeed::parse(value).unwrap_or(text_speed),
            "chapter" => {
                if let Ok(n) = value.parse() {
                    options.story.chapter = n;
                    options.story_override[0] = Some(n);
                }
            }
            "step" => {
                if let Ok(n) = value.parse() {
                    options.story.step = n;
                    options.story_override[1] = Some(n);
                }
            }
            "substep" => {
                if let Ok(n) = value.parse() {
                    options.story.substep = n;
                    options.story_override[2] = Some(n);
                }
            }
            "night" => options.story.daytime = value == "0",
            "seed" => options.play.seed = value.parse().ok(),
            "pilot" => pilot = value != "0",
            "fight" => options.play.fight = value != "0",
            "route" => {
                session.route = value.parse().ok();
                pilot |= session.route.is_some();
            }
            "speed" => session.speed = value.parse().ok().filter(|&n| n > 0),
            "budget" => session.budget_ms = value.parse().unwrap_or(session.budget_ms),
            "render" => session.render = value != "0",
            "battles" => session.show_battles = value == "show",
            "event" => options.event = value.parse().ok(),
            "newgame" => options.new_game = value != "0",
            "battle" => {
                if let Ok(record) = value.parse() {
                    options.battle = Some(crate::battle::Request {
                        record: Some(record),
                        ..Default::default()
                    });
                }
            }
            "battle-stage" => {
                if let Ok(stage) = value.parse() {
                    let mut request = options.battle.take().unwrap_or_default();
                    request.record = None;
                    request.stage = stage;
                    options.battle = Some(request);
                }
            }
            "monsters" => {
                if let (Some(request), Some(monsters)) =
                    (&mut options.battle, parse_monsters(value))
                {
                    request.monsters = monsters;
                }
            }
            "record" => mode.record = value != "0",
            "play-record" => mode.play_record = value != "0",
            _ => {}
        }
    }
    if mode.record && mode.play_record {
        error("record=1 and play-record=1 cannot be combined; recording this session");
        mode.play_record = false;
    }
    // A record must replay: with no `seed` the bots' generators would differ on every play.
    if mode.record && options.play.seed.is_none() {
        options.play.seed = Some(7);
    }
    // Same order as `--fx` then `--no-fx`: the off list wins whatever the query order.
    if let Err(e) = fx.apply_list(&fx_on, true) {
        error(&format!("fx=: {e}"));
    }
    if let Err(e) = fx.apply_list(&fx_off, false) {
        error(&format!("nofx=: {e}"));
    }
    // The route mode needs the engine to play the battles, like `--route` needs `--fight`.
    options.play.fight |= session.route.is_some();
    (
        options,
        pilot.then_some(session),
        screens,
        fx,
        text_speed,
        mode,
    )
}

/// `number[xcount]` comma-separated of the page's `monsters` parameter.
fn parse_monsters(text: &str) -> Option<Vec<(u16, u16)>> {
    let mut out = Vec::new();
    for part in text.split(',').filter(|p| !p.is_empty()) {
        let (number, count) = part.split_once('x').unwrap_or((part, "1"));
        out.push((number.parse().ok()?, count.parse().ok()?));
    }
    Some(out)
}

/// Sends the bytes of each file picked in `#rom` to the event loop. The input and the new game
/// button are disabled while the file is read, so a new game always has a ROM queued before it;
/// the bytes stay in the browser. With `?record=1` (`record_seed`) the recording starts on that
/// ROM's own session: `UserEvent::Record` goes first, so the record's first step is the
/// session's first step, and the fields that would replace the session stay disabled.
fn watch_rom_input(proxy: EventLoopProxy<UserEvent>, record_seed: Option<u64>) {
    let Some(input) = document()
        .and_then(|d| d.get_element_by_id("rom"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        error("The page has no <input type=file id=rom> field.");
        return;
    };
    let target = input.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let Some(file) = target.files().and_then(|f| f.get(0)) else {
            return;
        };
        let (name, size) = (file.name(), file.size());
        // A file input fires `change` only when its value differs: clear the selection so
        // picking the same ROM again (after a new game or a refused file) is read again.
        target.set_value("");
        target.set_disabled(true);
        set_disabled("newgame", true);
        announce(&format!(
            "Reading {name} ({:.0} MiB)… the ROM does not leave this browser.",
            size / 1048576.0
        ));
        let proxy = proxy.clone();
        let target = target.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => {
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    ROM_READY.set(true);
                    if let Some(seed) = record_seed {
                        recording_ui(seed, 0);
                        let _ = proxy.send_event(UserEvent::Record);
                        let _ = proxy.send_event(UserEvent::Rom(bytes));
                        announce(&format!(
                            "Recording this session (seed {seed}, {:.0} MiB ROM read locally). \
                             “Save recording” downloads it at any moment; nothing is uploaded.",
                            size / 1048576.0
                        ));
                    } else {
                        let _ = proxy.send_event(UserEvent::Rom(bytes));
                        target.set_disabled(false);
                        set_disabled("newgame", false);
                        announce(&format!(
                            "ROM read locally ({:.0} MiB); it does not leave this browser.",
                            size / 1048576.0
                        ));
                    }
                    focus_canvas();
                }
                Err(e) => {
                    target.set_disabled(false);
                    error(&format!("Cannot read the ROM: {e:?}"));
                }
            }
        });
    });
    if input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the ROM field.");
    }
    // The listener lives as long as the page.
    on_change.forget();
}

/// The page's recorded step count and recording indicator (`App::step_begin`, once a second).
pub fn recording_progress(seed: u64, steps: u64) {
    recording_ui(seed, steps);
}

/// Shows the recording indicator with `steps` recorded so far, and the seed the record names.
fn recording_ui(seed: u64, steps: u64) {
    RECORDING.set(true);
    let Some(el) = document().and_then(|d| d.get_element_by_id("recording")) else {
        return;
    };
    el.set_text_content(Some(&format!("● recording · seed {seed} · {steps} steps")));
    let _ = el.remove_attribute("hidden");
    let _ = el.set_attribute("data-kind", "recording");
    set_disabled("recordsave", false);
    // A recorded session is the one that started with the ROM: nothing may replace it. The
    // layout, the screens and the effects never reach the logic and stay live.
    for id in ["rom", "save", "time", "newgame"] {
        set_disabled(id, true);
    }
}

/// Hands the record so far to the page as a download (`index.html`'s `dq9Download`): a `Blob`
/// the browser saves, `dq9-<seed>-<steps>.dq9rec`. The file never leaves the machine.
pub fn offer_record(name: &str, text: &str) {
    let download = web_sys::window()
        .and_then(|window| js_sys::Reflect::get(&window, &"dq9Download".into()).ok())
        .and_then(|helper| helper.dyn_into::<js_sys::Function>().ok());
    let Some(download) = download else {
        error(
            "The page has no download helper (index.html is out of date): the record was not saved.",
        );
        return;
    };
    if let Err(e) = download.call2(
        &wasm_bindgen::JsValue::UNDEFINED,
        &name.into(),
        &text.into(),
    ) {
        error(&format!("Cannot hand the record to the page: {e:?}"));
        return;
    }
    announce(&format!(
        "Record saved as {name} ({} KiB) in your downloads folder; it stays on this machine.",
        text.len() / 1024
    ));
}

/// Watches the page's "Save recording" button (`#recordsave`): the app answers with
/// `offer_record`.
fn watch_record_save(proxy: EventLoopProxy<UserEvent>) {
    let Some(button) = document().and_then(|d| d.get_element_by_id("recordsave")) else {
        error("The page has no <button id=recordsave> button.");
        return;
    };
    let on_click = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let _ = proxy.send_event(UserEvent::RecordSave);
        focus_canvas();
    });
    if button
        .add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the \"Save recording\" button.");
    }
    on_click.forget();
}

/// Reads the `.dq9rec` picked in `#recordfile` (a native- or Web-made record) and replays it:
/// its start options replace the page's, and the ROM is chosen as for a normal run.
fn watch_record_input(proxy: EventLoopProxy<UserEvent>) {
    let Some(input) = document()
        .and_then(|d| d.get_element_by_id("recordfile"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        error("The page has no <input type=file id=recordfile> field.");
        return;
    };
    let target = input.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        if RECORDING.get() {
            // A replay restarts the session under the record's own start options (`App`): the
            // page's recording would then hold two sessions' steps.
            error("This page is recording its session: reload it to replay a record.");
            return;
        }
        let Some(file) = target.files().and_then(|f| f.get(0)) else {
            return;
        };
        let name = file.name();
        // Same as the ROM field: choosing the same record twice replays it twice.
        target.set_value("");
        let proxy = proxy.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let buffer = match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => buffer,
                Err(e) => {
                    error(&format!("Cannot read {name}: {e:?}"));
                    return;
                }
            };
            let Ok(text) = String::from_utf8(js_sys::Uint8Array::new(&buffer).to_vec()) else {
                error(&format!("{name} is not a .dq9rec file (not UTF-8 text)"));
                return;
            };
            let record = match crate::record::Record::parse(&text) {
                Ok(record) => record,
                Err(e) => {
                    error(&format!("{name}: {e}"));
                    return;
                }
            };
            let (end, seed) = (record.end, record.header.seed);
            let _ = proxy.send_event(UserEvent::PlayRecord(Box::new(record)));
            if let Some(el) = document().and_then(|d| d.get_element_by_id("recording")) {
                el.set_text_content(Some(&format!("▶ replay · seed {seed} · {end} steps")));
                let _ = el.remove_attribute("hidden");
            }
            if ROM_READY.get() {
                announce(&format!(
                    "{name}: replaying {end} recorded steps (seed {seed})…"
                ));
            } else {
                announce(&format!(
                    "{name}: {end} recorded steps (seed {seed}); choose the ROM it was played on."
                ));
            }
            focus_canvas();
        });
    });
    if input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the recording field.");
    }
    on_change.forget();
}

/// Sends the bytes of the optional save picked in `#save` to the event loop.
fn watch_save_input(proxy: EventLoopProxy<UserEvent>) {
    let Some(input) = document()
        .and_then(|d| d.get_element_by_id("save"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        error("The page has no <input type=file id=save> field.");
        return;
    };
    let target = input.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let Some(file) = target.files().and_then(|f| f.get(0)) else {
            return;
        };
        let size = file.size();
        // Same as the ROM field: choosing the same save again must replace the running game,
        // not be ignored because the input still holds that path.
        target.set_value("");
        target.set_disabled(true);
        let proxy = proxy.clone();
        let target = target.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => {
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    let _ = proxy.send_event(UserEvent::Save(bytes));
                    target.set_disabled(false);
                    announce(&format!(
                        "Save read locally ({:.1} MiB); it does not leave this browser.",
                        size / 1048576.0
                    ));
                    focus_canvas();
                }
                Err(e) => {
                    target.set_disabled(false);
                    error(&format!("Cannot read the save: {e:?}"));
                }
            }
        });
    });
    if input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the save field.");
    }
    on_change.forget();
}

/// Sends the time of day picked in the page's `<select id=time>` (also set from `?time=`).
fn watch_time_select(proxy: EventLoopProxy<UserEvent>, initial: u32) {
    let Some(select) = document()
        .and_then(|d| d.get_element_by_id("time"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        error("The page has no <select id=time> selector.");
        return;
    };
    select.set_value(&initial.to_string());
    let target = select.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        if let Ok(time) = target.value().parse::<u32>() {
            let _ = proxy.send_event(UserEvent::Time(time));
        }
        focus_canvas();
    });
    if select
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the time-of-day selector.");
    }
    on_change.forget();
}

/// Sends the layout picked in the page's `<select id=layout>` (also set from `?layout=`).
fn watch_layout_select(proxy: EventLoopProxy<UserEvent>, initial: Layout) {
    let Some(select) = document()
        .and_then(|d| d.get_element_by_id("layout"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        error("The page has no <select id=layout> selector.");
        return;
    };
    select.set_value(initial.name());
    let target = select.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        if let Ok(layout) = Layout::parse(&target.value()) {
            let _ = proxy.send_event(UserEvent::Layout(layout));
        }
        focus_canvas();
    });
    if select
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the screen layout selector.");
    }
    on_change.forget();
}

/// Sends the effects picked in the page's `#fx` checkboxes: every `input[data-fx]` of the
/// container is one switch, and `?fx=`/`?nofx=` set their initial state.
fn watch_fx_checkboxes(proxy: EventLoopProxy<UserEvent>, initial: EnhancedOptions) {
    let Some(container) = document().and_then(|d| d.get_element_by_id("fx")) else {
        error("The page has no <fieldset id=fx> control.");
        return;
    };
    // The initial state of each switch from the query string (the enhanced default: every
    // effect but the opt-in contact shadows).
    if let Ok(list) = container.query_selector_all("input[data-fx]") {
        for i in 0..list.length() {
            let Some(input) = list
                .get(i)
                .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                continue;
            };
            if let Some(name) = input.get_attribute("data-fx")
                && let Some(on) = initial.is_on(&name)
            {
                input.set_checked(on);
            }
        }
    }
    let target = container.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let mut fx = EnhancedOptions::NONE;
        if let Ok(list) = target.query_selector_all("input[data-fx]") {
            for i in 0..list.length() {
                let Some(input) = list
                    .get(i)
                    .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
                else {
                    continue;
                };
                let Some(name) = input.get_attribute("data-fx") else {
                    continue;
                };
                if input.checked() {
                    let _ = fx.set(&name, true);
                }
            }
        }
        let _ = proxy.send_event(UserEvent::Fx(fx));
        focus_canvas();
    });
    if container
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the effect checkboxes.");
    }
    on_change.forget();
}

/// Sends the field screen assignment picked in the page's `<select id=screens>` (also set from
/// `?screens=`).
fn watch_screens_select(proxy: EventLoopProxy<UserEvent>, initial: ScreenMode) {
    let Some(select) = document()
        .and_then(|d| d.get_element_by_id("screens"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        error("The page has no <select id=screens> selector.");
        return;
    };
    select.set_value(initial.name());
    let target = select.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        if let Ok(screens) = ScreenMode::parse(&target.value()) {
            let _ = proxy.send_event(UserEvent::Screens(screens));
        }
        focus_canvas();
    });
    if select
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the field screens selector.");
    }
    on_change.forget();
}

/// Starts a new game when the page's `<button id=newgame>` is clicked. The button stays disabled
/// until a ROM is queued, so the event loop never sees a new game without its ROM.
fn watch_new_game_button(proxy: EventLoopProxy<UserEvent>) {
    let Some(button) = document().and_then(|d| d.get_element_by_id("newgame")) else {
        error("The page has no <button id=newgame> button.");
        return;
    };
    let on_click = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let _ = proxy.send_event(UserEvent::NewGame);
        announce("New game: loading the prologue…");
        focus_canvas();
    });
    if button
        .add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch the \"New game\" button.");
    }
    on_click.forget();
}

/// Releases the keys held when the page loses focus: winit reports the canvas blur, which makes
/// the app forget its held keys (`WindowEvent::Focused(false)`), so no key stays stuck.
fn release_keys_on_blur() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_blur = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        if let Some(canvas) = canvas() {
            let _ = canvas.blur();
        }
    });
    if window
        .add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref())
        .is_err()
    {
        error("Cannot watch focus loss.");
    }
    on_blur.forget();
}

pub fn main() {
    console_error_panic_hook::set_once();
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            error(&format!("The engine could not start: {e}"));
            return;
        }
    };
    let proxy = event_loop.create_proxy();
    let (options, session, screens, fx, text_speed, mode) = options();
    // The seed of the recording the page asks for (`?record=1`; 7 unless `?seed=` names one).
    let record_seed = mode.record.then_some(options.play.seed.unwrap_or(7));
    watch_rom_input(proxy.clone(), record_seed);
    watch_save_input(proxy.clone());
    watch_new_game_button(proxy.clone());
    watch_record_save(proxy.clone());
    watch_record_input(proxy.clone());
    watch_time_select(proxy.clone(), options.time.unwrap_or(0));
    watch_layout_select(proxy.clone(), options.layout);
    watch_fx_checkboxes(proxy.clone(), fx);
    watch_screens_select(proxy.clone(), screens);
    release_keys_on_blur();
    announce(
        "Choose your Dragon Quest IX ROM (.nds); a .sav save is optional. \
         These files stay in this browser.",
    );
    if mode.play_record {
        announce(
            "Play recording: choose a .dq9rec file (native- or Web-made), then the ROM it \
             was played on; it stays in this browser.",
        );
    }
    let mut app = App::new(proxy, options, None);
    app.set_screens(screens);
    app.set_text_speed(text_speed);
    app.set_fx(fx);
    if let Some(spec) = session {
        app.set_session(spec);
    }
    event_loop.spawn_app(app);
}
