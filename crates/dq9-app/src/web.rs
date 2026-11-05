//! Web entry point. The player picks the ROM with the page's `<input type=file id=rom>`: it is
//! read in the browser and never uploaded or fetched. The map comes from the URL query
//! (`?map=M01&time=0&ds`), the canvas is `#dq9`, status lines go to `#status`.

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::WindowAttributes;

use crate::app::{App, Options, UserEvent};

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

pub fn status(message: &str) {
    web_sys::console::log_1(&message.into());
    if let Some(el) = document().and_then(|d| d.get_element_by_id("status")) {
        el.set_text_content(Some(message));
    }
}

/// Draws into the page's `#dq9` canvas (a new one is appended when absent).
pub fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    let canvas = document()
        .and_then(|d| d.get_element_by_id("dq9"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    let append = canvas.is_none();
    attributes
        .with_canvas(canvas)
        .with_append(append)
        .with_prevent_default(true)
}

/// `map`, `time` (`night`, `morning`, `day`, `evening` or 0-3), `clock` (second of the day), `ds` and `mute` from the page's query string.
fn options() -> Options {
    let mut options = Options {
        map: None,
        save: None,
        time: None,
        clock: None,
        ds_aspect: false,
        mute: false,
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
            "ds" => options.ds_aspect = value != "0",
            "mute" => options.mute = value != "0",
            "chapter" => {
                if let Ok(n) = value.parse() {
                    options.story.chapter = n;
                }
            }
            "step" => {
                if let Ok(n) = value.parse() {
                    options.story.step = n;
                }
            }
            "substep" => {
                if let Ok(n) = value.parse() {
                    options.story.substep = n;
                }
            }
            "night" => options.story.daytime = value == "0",
            _ => {}
        }
    }
    options
}

/// Sends the bytes of each file picked in `#rom` to the event loop.
fn watch_rom_input(proxy: EventLoopProxy<UserEvent>) {
    let Some(input) = document()
        .and_then(|d| d.get_element_by_id("rom"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        status("the page has no <input type=file id=rom>");
        return;
    };
    let target = input.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let Some(file) = target.files().and_then(|f| f.get(0)) else {
            return;
        };
        status(&format!(
            "reading {} ({:.0} MiB)...",
            file.name(),
            file.size() / 1048576.0
        ));
        let proxy = proxy.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => {
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    let _ = proxy.send_event(UserEvent::Rom(bytes));
                }
                Err(e) => status(&format!("cannot read the file: {e:?}")),
            }
        });
    });
    if input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        status("cannot watch the ROM input");
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
        status("the page has no <input type=file id=save>");
        return;
    };
    let target = input.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let Some(file) = target.files().and_then(|f| f.get(0)) else {
            return;
        };
        let proxy = proxy.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => {
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    let _ = proxy.send_event(UserEvent::Save(bytes));
                }
                Err(e) => status(&format!("cannot read the save: {e:?}")),
            }
        });
    });
    if input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        status("cannot watch the save input");
    }
    on_change.forget();
}

/// Sends the time of day picked in the page's `<select id=time>` (also set from `?time=`).
fn watch_time_select(proxy: EventLoopProxy<UserEvent>, initial: u32) {
    let Some(select) = document()
        .and_then(|d| d.get_element_by_id("time"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        return;
    };
    select.set_value(&initial.to_string());
    let target = select.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        if let Ok(time) = target.value().parse::<u32>() {
            let _ = proxy.send_event(UserEvent::Time(time));
        }
    });
    if select
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
        .is_err()
    {
        status("cannot watch the time select");
    }
    on_change.forget();
}

pub fn main() {
    console_error_panic_hook::set_once();
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            status(&format!("cannot start: {e}"));
            return;
        }
    };
    let proxy = event_loop.create_proxy();
    watch_rom_input(proxy.clone());
    watch_save_input(proxy.clone());
    let options = options();
    watch_time_select(proxy.clone(), options.time.unwrap_or(0));
    status("pick your Dragon Quest IX ROM (.nds); optionally a .sav, both stay in this browser");
    event_loop.spawn_app(App::new(proxy, options, None));
}
