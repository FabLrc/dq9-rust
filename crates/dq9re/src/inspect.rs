//! ROM inspection commands: header and banner, filesystem listing and statistics, NARC and
//! GPC2 archives.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use dq9_formats::Gpc2;
use nds_rom::rom::BANNER_LANGUAGES;
use nds_rom::{Narc, Rom, compress};

use crate::extract;
use crate::magic;

pub(crate) fn info(rom: &Rom) -> Result<()> {
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

pub(crate) fn ls(rom: &Rom, prefix: &str) -> Result<()> {
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

pub(crate) fn stats(rom: &Rom) -> Result<()> {
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

pub(crate) fn narc(rom: &Rom, path: &str, out: Option<&Path>, decompress: bool) -> Result<()> {
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

pub(crate) fn gp2(rom: &Rom, path: &str, out: Option<&Path>) -> Result<()> {
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
