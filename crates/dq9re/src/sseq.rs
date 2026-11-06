//! `dq9re sseq`: renders a sequence of `bgm.sdat` offline to a WAV file and reports what it
//! contains (`dq9_audio`, `docs/re/sdat.md`). The smoke test of the sound path without a
//! window.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use dq9_audio::{Player, Song};
use dq9_formats::Table;
use dq9_formats::bgm::{self, Overrides};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::sdat::Sdat;
use nds_rom::Rom;

/// Path of the music archive in the ROM filesystem.
const BGM_SDAT: &str = "data/sound/bgm.sdat";

#[derive(clap::Args)]
pub struct SseqArgs {
    /// Sequence index or name (`5`, `BG_005`); omitted: list the archive's sequences.
    id: Option<String>,
    /// Map id (`1100`) or code (`M01`) whose music is rendered instead of a sequence.
    #[arg(long, conflicts_with = "id")]
    map: Option<String>,
    /// Time of day for `--map` (0 night, 1 morning, 2 day, 3 evening).
    #[arg(long, default_value_t = 2)]
    time: u32,
    /// WAV file to write (16-bit stereo).
    #[arg(long)]
    wav: Option<PathBuf>,
    /// Sample rate of the render.
    #[arg(long, default_value_t = 32728)]
    rate: u32,
    /// Passes of the sequence's loop to render after the intro.
    #[arg(long, default_value_t = 1)]
    loops: u32,
    /// Seconds of fade-out added at the end.
    #[arg(long, default_value_t = 2.0)]
    fade: f32,
    /// Longest render, in seconds.
    #[arg(long, default_value_t = 600.0)]
    max_seconds: f32,
}

fn resolve(rom: &Rom, args: &SseqArgs, sdat: &Sdat) -> Result<Option<usize>> {
    if let Some(map) = &args.map {
        let table = Table::parse(rom.file_by_path(maplist::PATH)?)?;
        let list = MapList::parse(&table)?;
        let overrides = Overrides::parse(&Table::parse(rom.file_by_path(bgm::PATH)?)?)?;
        let record = match map.parse::<u16>() {
            Ok(id) => list.find(id),
            Err(_) => list.records.iter().find(|r| r.code == map.as_bytes()),
        }
        .with_context(|| format!("no map {map}"))?;
        // Story flags are not modelled in the tool: none is set.
        let sequence = bgm::sequence_for_map(&list, &overrides, record.id, args.time, |_| false);
        println!(
            "map {} ({}): music slot {}, sequence {:?}",
            record.id,
            String::from_utf8_lossy(record.code),
            record.music(),
            sequence
        );
        return Ok(sequence.map(usize::from));
    }
    let Some(id) = &args.id else { return Ok(None) };
    match id.parse::<usize>() {
        Ok(n) => Ok(Some(n)),
        Err(_) => Ok(Some(
            sdat.find_sequence(id)
                .with_context(|| format!("no sequence named {id}"))?,
        )),
    }
}

pub fn run(rom: &Rom, args: &SseqArgs) -> Result<()> {
    let sdat = Sdat::parse(rom.file_by_path(BGM_SDAT)?).context("parsing bgm.sdat")?;
    let Some(index) = resolve(rom, args, &sdat)? else {
        if args.map.is_some() {
            println!("this map has no music");
            return Ok(());
        }
        for i in 0..sdat.sequence_count() {
            let Some(info) = sdat.sequence_info(i) else {
                continue;
            };
            println!(
                "{i:3} {:8} bank {:3} volume {:3} file {}",
                sdat.sequence_name(i).unwrap_or("-"),
                info.bank,
                info.volume,
                info.file
            );
        }
        return Ok(());
    };
    let song = Arc::new(Song::load(&sdat, index).with_context(|| format!("sequence {index}"))?);
    println!(
        "sequence {index} {}: {} bytes of commands",
        song.name.as_deref().unwrap_or("-"),
        song.stream_len()
    );
    let mut player = Player::new(song, args.rate);
    let mut samples: Vec<f32> = Vec::new();
    let mut tempos: Vec<u16> = Vec::new();
    let mut buffer = vec![0f32; 2 * 1024];
    let mut loop_end = None;
    let limit = (args.max_seconds * args.rate as f32) as usize * 2;
    while !player.finished() && samples.len() < limit {
        player.render(&mut buffer);
        samples.extend_from_slice(&buffer);
        if tempos.last() != Some(&player.tempo()) {
            tempos.push(player.tempo());
        }
        if loop_end.is_none() && player.loops() >= args.loops && player.loops() != u32::MAX {
            loop_end = Some(samples.len() / 2);
            if args.fade > 0.0 {
                player.fade_out(args.fade);
            } else {
                break;
            }
        }
    }
    let cut = loop_end.is_none() && !player.finished();
    let frames = samples.len() / 2;
    // Trim the silence that fills the last block.
    let mut end = frames;
    while end > 0 && samples[2 * end - 2..2 * end].iter().all(|s| s.abs() < 1e-5) {
        end -= 1;
    }
    samples.truncate(end * 2);
    let rate = args.rate as f64;
    let rms = (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>()
        / samples.len().max(1) as f64)
        .sqrt();
    let out_peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    let clipped = samples.iter().filter(|s| s.abs() >= 0.999).count();
    println!("tempo: {tempos:?} BPM; {} tracks", player.track_count());
    match loop_end {
        Some(at) => println!(
            "loop: {} pass(es) of the infinite loop done at {:.2} s (intro included)",
            args.loops,
            at as f64 / rate
        ),
        None if cut => println!("stopped at --max-seconds before the loop was reached"),
        None => println!("the sequence ends without looping"),
    }
    println!(
        "duration {:.2} s ({end} frames at {} Hz), mix peak {:.3} before the limiter, {:.3} written, rms {:.4}, {clipped} samples at full scale",
        end as f64 / rate,
        args.rate,
        player.peak(),
        out_peak,
        rms
    );
    if let Some(path) = &args.wav {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, wav(&samples, args.rate))
            .with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    } else if frames == 0 {
        bail!("the sequence rendered nothing");
    }
    Ok(())
}

/// 16-bit PCM stereo WAV.
pub(crate) fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    for s in samples {
        out.extend(((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    out
}
