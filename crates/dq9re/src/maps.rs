//! `dq9re maps`: the map list, optionally checked against a melonDS state.

use std::{fs, path::Path};

use anyhow::{Context, Result};
use dq9_formats::{Table, maplist, maplist::MapList, melonds};
use nds_rom::Rom;

fn text(s: Option<&[u8]>) -> String {
    s.map(|s| String::from_utf8_lossy(s).into_owned())
        .unwrap_or_default()
}

pub fn run(rom: &Rom, state: Option<&Path>) -> Result<()> {
    let table = Table::parse(rom.file_by_path(maplist::PATH)?).context("parsing map list")?;
    let list = MapList::parse(&table)?;
    let Some(state) = state else {
        println!("index    id  area code    kind music space  region / label");
        for (i, r) in list.records.iter().enumerate() {
            println!(
                "{i:5} {:5} {:5} {:7} {:4} {:5} {:5}  {} / {}",
                r.id,
                r.area,
                String::from_utf8_lossy(r.code),
                r.kind(),
                r.music(),
                r.space(),
                text(r.region),
                text(r.label),
            );
        }
        return Ok(());
    };
    let bytes = fs::read(state).with_context(|| format!("reading {}", state.display()))?;
    let state = melonds::State::parse(&bytes).context("parsing state")?;
    let differing = state.compare_map_list(&list)?;
    println!(
        "map list: {} records, {} differ from RAM {differing:?}",
        list.records.len(),
        differing.len()
    );
    let (id, area) = state.current_map()?;
    match list.find(id) {
        Some(r) => println!(
            "current map: {id} ({}, area {area}, list area {}) {} / {}",
            String::from_utf8_lossy(r.code),
            r.area,
            text(r.region),
            text(r.label),
        ),
        None => println!("current map: {id} (area {area}), not in the list"),
    }
    Ok(())
}
