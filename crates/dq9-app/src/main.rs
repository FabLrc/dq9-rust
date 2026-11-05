//! `dq9-app`: the Dragon Quest IX port's application. For now it shows a map
//! (`docs/app.md`): `dq9-app --map M01` natively, `index.html?map=M01` on the web.
//!
//! The only crate that touches the system: it reads the ROM (a file natively, an
//! `<input type=file>` on the web), opens the window and drives `dq9-render`.

mod app;
mod gpu;
mod lighting;
mod load;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod walk;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::main();
}
