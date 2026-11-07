//! Web entry point. The player picks the ROM with the page's `<input type=file id=rom>`: it is
//! read in the browser and never uploaded or fetched. The map comes from the URL query
//! (`?map=M01&time=0&layout=side`), the canvas is `#dq9`, status lines go to `#status`.

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::WindowAttributes;

use crate::app::{App, Options, UserEvent};
use crate::screens::Layout;

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
            "Ce fichier n'est pas une ROM Dragon Quest IX européenne (YDQP01).".to_owned(),
        ));
    }
    if let Some(detail) = message.strip_prefix("GPU: ") {
        return Some((
            Notice::Gpu,
            format!(
                "Le moteur graphique n'a pas pu démarrer (WebGPU ou WebGL 2 requis) : {detail}"
            ),
        ));
    }
    if let Some(detail) = message.strip_prefix("cannot create the window: ") {
        return Some((
            Notice::Window,
            format!("La fenêtre du jeu n'a pas pu être créée : {detail}"),
        ));
    }
    None
}

/// Shows a status line; engine errors that the player can act on are shown in French and in
/// the error style, and an unusable ROM disables "Nouvelle partie" again. Engine progress
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
                    for id in ["rom", "save", "time", "layout", "newgame"] {
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

/// `map`, `time` (`night`, `morning`, `day`, `evening` or 0-3), `clock` (second of the day),
/// `layout` (`stacked`, `side` or `top`) and `mute` from the page's query string.
fn options() -> Options {
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
        story_override: [None; 3],
        story: dq9_formats::cast::Story {
            chapter: 1,
            step: 1,
            substep: 1,
            daytime: true,
            ..dq9_formats::cast::Story::default()
        },
    };
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
            "event" => options.event = value.parse().ok(),
            "newgame" => options.new_game = value != "0",
            _ => {}
        }
    }
    options
}

/// Sends the bytes of each file picked in `#rom` to the event loop. The input and the new game
/// button are disabled while the file is read, so a new game always has a ROM queued before it;
/// the bytes stay in the browser.
fn watch_rom_input(proxy: EventLoopProxy<UserEvent>) {
    let Some(input) = document()
        .and_then(|d| d.get_element_by_id("rom"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        error("La page n'a pas de champ <input type=file id=rom>.");
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
            "Lecture de {name} ({:.0} Mio)… la ROM ne quitte pas ce navigateur.",
            size / 1048576.0
        ));
        let proxy = proxy.clone();
        let target = target.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => {
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    let _ = proxy.send_event(UserEvent::Rom(bytes));
                    target.set_disabled(false);
                    set_disabled("newgame", false);
                    announce(&format!(
                        "ROM lue localement ({:.0} Mio) ; elle ne quitte pas ce navigateur.",
                        size / 1048576.0
                    ));
                    focus_canvas();
                }
                Err(e) => {
                    target.set_disabled(false);
                    error(&format!("Lecture de la ROM impossible : {e:?}"));
                }
            }
        });
    });
    if input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Impossible de surveiller le champ ROM.");
    }
    // The listener lives as long as the page.
    on_change.forget();
}
/// Sends the bytes of the optional save picked in `#save` to the event loop.
fn watch_save_input(proxy: EventLoopProxy<UserEvent>) {
    let Some(input) = document()
        .and_then(|d| d.get_element_by_id("save"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        error("La page n'a pas de champ <input type=file id=save>.");
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
                        "Sauvegarde lue localement ({:.1} Mio) ; elle ne quitte pas ce navigateur.",
                        size / 1048576.0
                    ));
                    focus_canvas();
                }
                Err(e) => {
                    target.set_disabled(false);
                    error(&format!("Lecture de la sauvegarde impossible : {e:?}"));
                }
            }
        });
    });
    if input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        error("Impossible de surveiller le champ sauvegarde.");
    }
    on_change.forget();
}

/// Sends the time of day picked in the page's `<select id=time>` (also set from `?time=`).
fn watch_time_select(proxy: EventLoopProxy<UserEvent>, initial: u32) {
    let Some(select) = document()
        .and_then(|d| d.get_element_by_id("time"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        error("La page n'a pas de sélecteur <select id=time>.");
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
        error("Impossible de surveiller le sélecteur de moment.");
    }
    on_change.forget();
}

/// Sends the layout picked in the page's `<select id=layout>` (also set from `?layout=`).
fn watch_layout_select(proxy: EventLoopProxy<UserEvent>, initial: Layout) {
    let Some(select) = document()
        .and_then(|d| d.get_element_by_id("layout"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        error("La page n'a pas de sélecteur <select id=layout>.");
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
        error("Impossible de surveiller le sélecteur d'écrans.");
    }
    on_change.forget();
}

/// Starts a new game when the page's `<button id=newgame>` is clicked. The button stays disabled
/// until a ROM is queued, so the event loop never sees a new game without its ROM.
fn watch_new_game_button(proxy: EventLoopProxy<UserEvent>) {
    let Some(button) = document().and_then(|d| d.get_element_by_id("newgame")) else {
        error("La page n'a pas de bouton <button id=newgame>.");
        return;
    };
    let on_click = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let _ = proxy.send_event(UserEvent::NewGame);
        announce("Nouvelle partie : chargement du prologue…");
        focus_canvas();
    });
    if button
        .add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())
        .is_err()
    {
        error("Impossible de surveiller le bouton « Nouvelle partie ».");
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
        error("Impossible de surveiller la perte de focus.");
    }
    on_blur.forget();
}

pub fn main() {
    console_error_panic_hook::set_once();
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            error(&format!("Le moteur n'a pas pu démarrer : {e}"));
            return;
        }
    };
    let proxy = event_loop.create_proxy();
    watch_rom_input(proxy.clone());
    watch_save_input(proxy.clone());
    watch_new_game_button(proxy.clone());
    let options = options();
    watch_time_select(proxy.clone(), options.time.unwrap_or(0));
    watch_layout_select(proxy.clone(), options.layout);
    release_keys_on_blur();
    announce(
        "Choisissez votre ROM Dragon Quest IX (.nds) ; une sauvegarde .sav est facultative. \
         Ces fichiers restent dans ce navigateur.",
    );
    event_loop.spawn_app(App::new(proxy, options, None));
}
