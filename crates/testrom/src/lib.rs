//! Locates the game image for tests that check invariants on real data.
//!
//! The ROM is never committed, so tests call [`load`] and return early when it yields `None`
//! (CI runs without it). Location: `DQ9_ROM`, else the only `*.nds` in `<repo>/rom/` or
//! `<repo>/`.

use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

fn find() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DQ9_ROM") {
        return Some(PathBuf::from(path));
    }
    // `cargo test` sets CARGO_MANIFEST_DIR at run time (the tested crate, also under
    // `crates/`). Prefer it to the compile-time value: a target directory shared between
    // worktrees can reuse a binary compiled in another, possibly removed, checkout.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    let repo = manifest.join("../..");
    [repo.join("rom"), repo].into_iter().find_map(|dir| {
        fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nds")))
    })
}

/// ROM bytes, read once per test binary; `None` (with a notice on stderr) when absent.
pub fn load() -> Option<&'static [u8]> {
    static ROM: LazyLock<Option<Vec<u8>>> =
        LazyLock::new(|| find().map(|path| fs::read(path).expect("reading ROM")));
    let rom = &*ROM;
    if rom.is_none() {
        eprintln!("skipped: no ROM (set DQ9_ROM)");
    }
    rom.as_deref()
}
