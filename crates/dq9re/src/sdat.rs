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

#[derive(clap::Args)]
pub struct SeArgs {
    /// Entry of the sequence archive (`12`).
    entry: usize,
    /// Sequence archive of the sound archive (`100`: the field's base archive).
    #[arg(long, default_value_t = 100)]
    archive: usize,
    /// Sound archive: `se_norm`, `se_btl`, or a ROM path.
    #[arg(long, default_value = "se_norm")]
    file: String,
    /// WAV file to write (16-bit stereo).
    #[arg(long)]
    wav: Option<std::path::PathBuf>,
    /// Sample rate of the render.
    #[arg(long, default_value_t = 32728)]
    rate: u32,
    /// Longest render, in seconds (endless effects are cut and faded here).
    #[arg(long, default_value_t = 10.0)]
    max_seconds: f32,
}

/// `dq9re se`: renders one sound effect of a sequence archive and reports its length and
/// levels.
pub fn run_se(rom: &Rom, args: &SeArgs) -> Result<()> {
    use dq9_audio::{Player, Song};
    let path = path(&args.file);
    let sdat = Sdat::parse(rom.file_by_path(&path)?).with_context(|| format!("parsing {path}"))?;
    let (_, ssar) = sdat.sequence_archive(args.archive)?;
    let entry = ssar
        .entries
        .get(args.entry)
        .filter(|e| e.offset.is_some())
        .with_context(|| format!("archive {} has no entry {}", args.archive, args.entry))?;
    let mut effects = Song::load_effects(&sdat, args.archive)?;
    let song = std::sync::Arc::new(
        effects
            .swap_remove(args.entry)
            .context("entry has no sequence")?,
    );
    println!(
        "{path} archive {} entry {}: bank {}, volume {}, player {} priority {}, starts at {:#x} of {} bytes",
        args.archive,
        args.entry,
        entry.bank,
        entry.volume,
        entry.player,
        entry.player_priority,
        entry.offset.unwrap_or(0),
        song.stream_len()
    );
    let at = entry.offset.unwrap_or(0) as usize;
    println!(
        "commands: {}",
        hex(&ssar.stream[at..(at + 24).min(ssar.stream.len())])
    );
    let mut player = Player::new(song, args.rate);
    let mut samples: Vec<f32> = Vec::new();
    let mut buffer = vec![0f32; 2 * 1024];
    let limit = (args.max_seconds * args.rate as f32) as usize * 2;
    while !player.finished() && samples.len() < limit {
        player.render(&mut buffer);
        samples.extend_from_slice(&buffer);
    }
    let cut = !player.finished();
    let mut end = samples.len() / 2;
    while end > 0 && samples[2 * end - 2..2 * end].iter().all(|s| s.abs() < 1e-5) {
        end -= 1;
    }
    samples.truncate(end * 2);
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    let rms = (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>()
        / samples.len().max(1) as f64)
        .sqrt();
    println!(
        "duration {:.3} s{} ({end} frames at {} Hz), mix peak {:.3} before the limiter, {peak:.3} written, rms {rms:.4}",
        end as f64 / f64::from(args.rate),
        if cut {
            " (cut: the effect does not end)"
        } else {
            ""
        },
        args.rate,
        player.peak()
    );
    if end == 0 {
        bail!("the effect rendered nothing");
    }
    if let Some(path) = &args.wav {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, crate::sseq::wav(&samples, args.rate))
            .with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
