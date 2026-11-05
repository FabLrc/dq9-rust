//! `dq9re`: command-line tooling for reverse engineering the Dragon Quest IX ROM.

mod coverage;
mod extract;
mod font;
mod maps;
mod msg;
mod save;
mod state;
mod stb;
mod talk;
mod text;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use dq9_formats::Gpc2;
use nds_rom::rom::BANNER_LANGUAGES;
use nds_rom::{Narc, Rom, compress};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// ROM image. Defaults to the only `*.nds` file in `rom/` or the current directory.
    #[arg(long, global = true, env = "DQ9_ROM")]
    rom: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Header, banner, binaries and overlay summary.
    Info,
    /// List filesystem entries: id, size, leading magic, path.
    Ls {
        /// Only paths starting with this prefix (e.g. `data/map/`).
        prefix: Option<String>,
    },
    /// Per-extension file counts, sizes and leading magics.
    Stats,
    /// Extract binaries (decompressed), overlays, filesystem and a `layout.json` memory map.
    Extract {
        #[arg(long, default_value = "build/rom")]
        out: PathBuf,
        /// Skip the 7.5k data files; only binaries, overlays and layout.
        #[arg(long)]
        code_only: bool,
        /// Also unpack every `.gp2` archive (decompressed) into `<out>/gp2/<archive>/`.
        #[arg(long, conflicts_with = "code_only")]
        unpack: bool,
    },
    /// List (and optionally extract, decompressed) the members of a GPC2 archive (`.gp2`).
    Gp2 {
        /// Path inside the ROM filesystem, e.g. `data/scenario/M02M0.gp2`.
        path: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Print the records of a record-table member (`.bin`/`.bmmp`/`.bcfg`) of a `.gp2`.
    Table {
        /// Archive inside the ROM, e.g. `data/scenario/M02M0.gp2`.
        archive: String,
        /// Member name, e.g. `018_en.bin`.
        member: String,
    },
    /// Dump every string of the record tables of one language to text files.
    Text {
        /// Language suffix of the members (`en`, `fr`, `de`, `it`, `es`).
        #[arg(long, default_value = "en")]
        lang: String,
        #[arg(long, default_value = "build/text")]
        out: PathBuf,
    },
    /// Render a game font to PNG: the glyph sheet, or one line of text with `--text`.
    Font {
        /// `me` (message font) or `s7` (small font).
        #[arg(default_value = "me")]
        font: String,
        /// Markup to lay out, e.g. "Caract<'e>ristiques".
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value = "build/font.png")]
        out: PathBuf,
        #[arg(long, default_value_t = 4)]
        scale: usize,
    },
    /// Run one dialogue string through the message pipeline and render its pages to PNG.
    Msg {
        /// Archive inside the ROM, e.g. `data/scenario/M02M0.gp2`.
        archive: String,
        /// Member name, e.g. `018_en.bin`.
        member: String,
        /// Record index in the table.
        #[arg(default_value_t = 0)]
        record: usize,
        #[arg(long, default_value = "build/msg.png")]
        out: PathBuf,
    },
    /// Decode an NPC dialogue file and show the line chosen for a given game state.
    Talk(talk::TalkArgs),
    /// Disassemble the event script of an event archive, with its texts next to the
    /// message natives.
    Stb {
        /// Event archive inside the ROM, e.g. `data/event/ev03030.gp2`.
        path: String,
        /// Language of the texts shown.
        #[arg(long, default_value = "en")]
        lang: String,
    },
    /// List (and optionally extract) the members of a NARC file in the ROM.
    Narc {
        /// Path inside the ROM filesystem, e.g. `data/map/F01M01.ambl`.
        path: String,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Decompress members that start with a Nintendo compression header; members that
        /// fail to decode are kept raw and flagged `raw`.
        #[arg(long, short)]
        decompress: bool,
    },
    /// Write one ROM file to disk, decompressing it if requested.
    Cat {
        path: String,
        #[arg(long)]
        out: PathBuf,
        /// Decompress a stream with a Nintendo compression header (LZ10/LZ11/Huffman/RLE).
        #[arg(long, short)]
        decompress: bool,
    },
    /// List the maps of `data/map/maplist9.bin` as the game loads them, or check them
    /// against the copy in a melonDS state's RAM and show the state's current map.
    Maps {
        #[arg(long)]
        state: Option<PathBuf>,
    },
    /// Build `re/functions.txt` from a checkout of the USA decompilation (DQIX/dqix-decomp):
    /// every function, at European addresses, classified game / platform / excluded.
    Functions {
        /// Root of the dqix-decomp checkout.
        decomp: PathBuf,
        #[arg(long, default_value = "re/functions.txt")]
        out: PathBuf,
    },
    /// Porting progress: game functions cited by `port:` markers in the port crates.
    /// Needs no ROM.
    Coverage {
        #[arg(long, default_value = "re/functions.txt")]
        functions: PathBuf,
        /// Directory of the crates to scan (tool crates are skipped).
        #[arg(long, default_value = "crates")]
        crates: PathBuf,
        /// Rewrite the progress block of this README (between the coverage markers).
        #[arg(long)]
        readme: Option<PathBuf>,
        /// With --readme: fail instead of writing when the block is out of date.
        #[arg(long, requires = "readme")]
        check: bool,
    },
    /// Summarize a DS save file (`.sav`) and verify its checksums. The ROM is optional: when
    /// found, character names are decoded with its message font.
    Save {
        path: PathBuf,
        /// Also list levels, equipment and appearance.
        #[arg(long, short)]
        verbose: bool,
    },
    /// Inspect a melonDS savestate (`.ml1`): sections, frame, scenario, resident code compared
    /// with the ROM when one is found, or `--peek ADDR[:LEN]` (hex) bytes of main RAM / TCM.
    State {
        path: PathBuf,
        #[arg(long, num_args = 1..)]
        peek: Vec<String>,
        /// Compare the RAM with the `.sav` the game was loaded from (`SAVE_COPIES`).
        #[arg(long)]
        save: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Commands that do not read the ROM, so that they also run in CI.
    match &cli.command {
        Command::Functions { decomp, out } => return coverage::run_functions(decomp, out),
        Command::Coverage {
            functions,
            crates,
            readme,
            check,
        } => return coverage::run_coverage(functions, crates, readme.as_deref(), *check),
        Command::Save { path, verbose } => return save::run(path, *verbose, cli.rom.as_deref()),
        Command::State { path, peek, save } => {
            return state::run(path, peek, save.as_deref(), cli.rom.as_deref());
        }
        _ => {}
    }
    let rom_path = match cli.rom {
        Some(path) => path,
        None => find_rom()?,
    };
    let bytes = fs::read(&rom_path).with_context(|| format!("reading {}", rom_path.display()))?;
    let rom = Rom::parse(&bytes).context("parsing ROM")?;
    match cli.command {
        Command::Info => info(&rom),
        Command::Ls { prefix } => ls(&rom, prefix.as_deref().unwrap_or("")),
        Command::Stats => stats(&rom),
        Command::Extract {
            out,
            code_only,
            unpack,
        } => extract::run(&rom, &out, code_only, unpack),
        Command::Gp2 { path, out } => gp2(&rom, &path, out.as_deref()),
        Command::Table { archive, member } => text::table(&rom, &archive, &member),
        Command::Text { lang, out } => text::dump(&rom, &lang, &out),
        Command::Font {
            font,
            text,
            out,
            scale,
        } => {
            let id = match font.as_str() {
                "me" => dq9_formats::FontId::Message,
                "s7" => dq9_formats::FontId::Small,
                other => bail!("unknown font {other:?} (expected me or s7)"),
            };
            font::run(&rom, id, text.as_deref(), &out, scale)
        }
        Command::Msg {
            archive,
            member,
            record,
            out,
        } => msg::run(&rom, &archive, &member, record, &out),
        Command::Talk(args) => talk::run(&rom, &args),
        Command::Stb { path, lang } => stb::run(&rom, &path, &lang),
        Command::Narc {
            path,
            out,
            decompress,
        } => narc(&rom, &path, out.as_deref(), decompress),
        Command::Maps { state } => maps::run(&rom, state.as_deref()),
        Command::Cat {
            path,
            out,
            decompress,
        } => {
            let data = rom.file_by_path(&path)?;
            let data = if decompress {
                compress::decompress(data)?
            } else {
                data.to_vec()
            };
            fs::write(&out, data).with_context(|| format!("writing {}", out.display()))
        }
        // Handled above, before the ROM is read.
        Command::Functions { .. }
        | Command::Coverage { .. }
        | Command::Save { .. }
        | Command::State { .. } => Ok(()),
    }
}

fn find_rom() -> Result<PathBuf> {
    for dir in ["rom", "."] {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let found: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nds")))
            .collect();
        match found.as_slice() {
            [] => continue,
            [one] => return Ok(one.clone()),
            many => bail!("several ROMs in {dir}/, pick one with --rom: {many:?}"),
        }
    }
    bail!("no *.nds found in rom/ or the current directory; pass --rom or set DQ9_ROM")
}

/// Leading 4 bytes, printable ASCII kept, others as `.`.
pub(crate) fn magic(data: &[u8]) -> String {
    data.iter()
        .take(4)
        .map(|&b| {
            if b.is_ascii_graphic() {
                char::from(b)
            } else {
                '.'
            }
        })
        .collect()
}

fn info(rom: &Rom) -> Result<()> {
    let h = &rom.header;
    println!(
        "title      {} ({}{}), version {}",
        h.title, h.game_code, h.maker_code, h.version
    );
    println!(
        "header CRC {:#06x} ({})",
        h.header_crc,
        if h.crc_ok() { "ok" } else { "MISMATCH" }
    );
    for (lang, title) in BANNER_LANGUAGES.iter().zip(rom.banner_titles()?) {
        println!("banner     {lang:<9} {}", title.replace('\n', " / "));
    }
    let arm9 = rom.arm9()?;
    let p = &arm9.params;
    println!(
        "arm9       load {:#010x} entry {:#010x} rom {:#x} bytes -> {:#x} decompressed (compressed: {})",
        arm9.load_addr,
        arm9.entry,
        h.arm9.size,
        arm9.data.len(),
        arm9.compressed
    );
    println!(
        "           sdk {:#010x}, bss {:#010x}..{:#010x}",
        p.sdk_version, p.static_bss_start, p.static_bss_end
    );
    for a in arm9.autoloads()? {
        println!(
            "autoload   {:#010x} size {:#x} bss {:#x}",
            a.ram_addr, a.size, a.bss_size
        );
    }
    println!(
        "arm7       load {:#010x} entry {:#010x} size {:#x}",
        h.arm7.load_addr, h.arm7.entry, h.arm7.size
    );
    let overlays = rom.overlays()?;
    println!(
        "overlays   {} ({} compressed)",
        overlays.len(),
        overlays.iter().filter(|o| o.compressed).count()
    );
    println!(
        "files      {} named, {} FAT entries",
        rom.files.files.len(),
        rom.file_count()
    );
    Ok(())
}

fn ls(rom: &Rom, prefix: &str) -> Result<()> {
    for f in rom
        .files
        .files
        .iter()
        .filter(|f| f.path.starts_with(prefix))
    {
        let data = rom.file(f.id)?;
        println!("{:5} {:>9} {:4} {}", f.id, data.len(), magic(data), f.path);
    }
    Ok(())
}

fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or("", |(_, ext)| ext)
}

fn stats(rom: &Rom) -> Result<()> {
    #[derive(Default)]
    struct Group {
        count: usize,
        bytes: usize,
        magics: BTreeMap<String, usize>,
    }
    let mut groups: BTreeMap<&str, Group> = BTreeMap::new();
    for f in &rom.files.files {
        let data = rom.file(f.id)?;
        let g = groups.entry(extension(&f.path)).or_default();
        g.count += 1;
        g.bytes += data.len();
        *g.magics.entry(magic(data)).or_default() += 1;
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(_, g)| std::cmp::Reverse(g.bytes));
    println!("{:<8} {:>6} {:>12}  magics", "ext", "files", "bytes");
    for (ext, g) in groups {
        let mut magics: Vec<_> = g.magics.into_iter().collect();
        magics.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let shown: Vec<String> = magics
            .iter()
            .take(4)
            .map(|(m, n)| format!("{m:?}x{n}"))
            .collect();
        let more = magics.len().saturating_sub(4);
        let more = if more > 0 {
            format!(" +{more} more")
        } else {
            String::new()
        };
        println!(
            "{ext:<8} {:>6} {:>12}  {}{more}",
            g.count,
            g.bytes,
            shown.join(" ")
        );
    }
    Ok(())
}

fn narc(rom: &Rom, path: &str, out: Option<&Path>, decompress: bool) -> Result<()> {
    let narc = Narc::parse(rom.file_by_path(path)?).with_context(|| format!("{path} as NARC"))?;
    if let Some(out) = out {
        fs::create_dir_all(out)?;
    }
    for i in 0..narc.len() {
        let index = u16::try_from(i)?;
        let raw = narc.file(index)?;
        let (data, codec) = match raw.first() {
            Some(0x10 | 0x11 | 0x24 | 0x28 | 0x30) if decompress => {
                match compress::decompress(raw) {
                    Ok(d) => (Cow::Owned(d), "nds"),
                    Err(_) => (Cow::Borrowed(raw), "raw"),
                }
            }
            _ => (Cow::Borrowed(raw), ""),
        };
        let name = narc
            .name(index)
            .map_or_else(|| format!("{i:04}.bin"), str::to_owned);
        println!(
            "{i:4} {:>9} {:4} {codec:3} {name}",
            data.len(),
            magic(&data)
        );
        if let Some(out) = out {
            let dest = out.join(&name);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, &data).with_context(|| format!("writing {}", dest.display()))?;
        }
    }
    Ok(())
}

fn gp2(rom: &Rom, path: &str, out: Option<&Path>) -> Result<()> {
    let archive =
        Gpc2::parse(rom.file_by_path(path)?).with_context(|| format!("{path} as GPC2"))?;
    for entry in &archive.entries {
        let data = archive
            .member(entry)
            .with_context(|| format!("{path}/{}", entry.name))?;
        println!(
            "{:08x} {:>8} -> {:>8} {:4} {}",
            entry.hash,
            entry.size,
            data.len(),
            magic(&data),
            entry.name
        );
    }
    if let Some(out) = out {
        extract::unpack_gp2(&archive, out)?;
    }
    Ok(())
}
