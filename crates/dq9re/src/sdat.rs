//! `dq9re sdat`: what a sound archive (`bgm.sdat`, `se_norm.sdat`, `se_btl.sdat`) holds: the
//! record lists with their names, players, groups and sequence archives (`docs/re/sdat.md`).

use anyhow::{Context as _, Result, bail};
use dq9_formats::sdat::{
    KIND_BANK, KIND_GROUP, KIND_PLAYER, KIND_SEQUENCE, KIND_SEQUENCE_ARCHIVE, KIND_STREAM,
    KIND_STREAM_PLAYER, KIND_WAVE_ARCHIVE, Sdat,
};
use nds_rom::Rom;

#[derive(clap::Args)]
pub struct SdatArgs {
    /// Archive: `bgm`, `se_norm`, `se_btl`, or a ROM path.
    archive: String,
    /// List of records: `seq`, `ssar`, `bank`, `swar`, `player`, `group`, `player2`, `strm`;
    /// omitted: the size of each list.
    kind: Option<String>,
    /// One record of the list, in detail (a sequence archive lists its entries).
    index: Option<usize>,
}

/// ROM path of an archive given by its short name.
pub fn path(name: &str) -> String {
    match name {
        "bgm" | "se_norm" | "se_btl" => format!("data/sound/{name}.sdat"),
        other => other.to_owned(),
    }
}

fn kind_of(name: &str) -> Result<usize> {
    Ok(match name {
        "seq" => KIND_SEQUENCE,
        "ssar" => KIND_SEQUENCE_ARCHIVE,
        "bank" => KIND_BANK,
        "swar" => KIND_WAVE_ARCHIVE,
        "player" => KIND_PLAYER,
        "group" => KIND_GROUP,
        "player2" => KIND_STREAM_PLAYER,
        "strm" => KIND_STREAM,
        other => bail!("unknown list `{other}`"),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run(rom: &Rom, args: &SdatArgs) -> Result<()> {
    let path = path(&args.archive);
    let sdat = Sdat::parse(rom.file_by_path(&path)?).with_context(|| format!("parsing {path}"))?;
    let Some(kind) = args.kind.as_deref() else {
        for (name, kind) in [
            ("seq", KIND_SEQUENCE),
            ("ssar", KIND_SEQUENCE_ARCHIVE),
            ("bank", KIND_BANK),
            ("swar", KIND_WAVE_ARCHIVE),
            ("player", KIND_PLAYER),
            ("group", KIND_GROUP),
            ("player2", KIND_STREAM_PLAYER),
            ("strm", KIND_STREAM),
        ] {
            let filled = (0..sdat.list_len(kind))
                .filter(|&i| sdat.record(kind, i).is_some())
                .count();
            println!(
                "{name:8} {:4} slots, {filled:4} filled",
                sdat.list_len(kind)
            );
        }
        return Ok(());
    };
    let kind = kind_of(kind)?;
    match args.index {
        None => {
            for i in 0..sdat.list_len(kind) {
                let Some(record) = sdat.record(kind, i) else {
                    continue;
                };
                println!(
                    "{i:4} {:16} {}",
                    sdat.name(kind, i).unwrap_or("-"),
                    hex(&record[..record.len().min(16)])
                );
            }
        }
        Some(index) => {
            let Some(record) = sdat.record(kind, index) else {
                bail!("no record {index}");
            };
            println!(
                "{} {}: {}",
                args.kind.as_deref().unwrap_or(""),
                sdat.name(kind, index).unwrap_or("-"),
                hex(&record[..record.len().min(32)])
            );
            if kind == KIND_GROUP {
                for e in sdat.group(index).unwrap_or_default() {
                    println!("  type {:#x} index {}", e.kind, e.index);
                }
            }
            if kind == KIND_SEQUENCE_ARCHIVE {
                let (info, ssar) = sdat.sequence_archive(index)?;
                println!(
                    "  file {}, stream {} bytes, {} entries",
                    info.file,
                    ssar.stream.len(),
                    ssar.entries.len()
                );
                for (i, e) in ssar.entries.iter().enumerate() {
                    let Some(offset) = e.offset else { continue };
                    println!(
                        "  {i:3}: offset {offset:#06x} bank {} volume {} channel priority {} player priority {} player {}",
                        e.bank, e.volume, e.channel_priority, e.player_priority, e.player
                    );
                }
            }
        }
    }
    Ok(())
}
