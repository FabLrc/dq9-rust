//! `dq9re hero`: the parts a character is drawn with (`dq9_formats::hero`,
//! `docs/re/characters.md` § "The hero").

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use dq9_formats::Gpc2;
use dq9_formats::hero::{HeroLook, ItemModels, Outfit};
use dq9_formats::map;
use dq9_formats::save::Save;
use nds_rom::Rom;

#[derive(clap::Args)]
pub struct HeroArgs {
    /// `.sav` whose hero is described; without it, the new-game hero.
    #[arg(long)]
    save: Option<PathBuf>,
    /// Character index in the slot (default: the hero, first of the party).
    #[arg(long)]
    character: Option<usize>,
}

pub fn run(rom: &Rom, args: &HeroArgs) -> Result<()> {
    let look = match &args.save {
        None => HeroLook::new_game(),
        Some(path) => {
            let bytes =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            let save = Save::parse(&bytes)?;
            let slot = save
                .slot(save.current_slot().context("no committed slot")?)
                .context("slot")?;
            let index = args
                .character
                .unwrap_or_else(|| usize::from(slot.standby_count()));
            let c = slot.character(index).context("no such character")?;
            let (w, h) = c.body();
            println!("character {index}: body words {w:#x} {h:#x} ({w}, {h})");
            println!("raw 352..380: {:02x?}", &c.bytes()[352..380]);
            HeroLook::from_character(&c)
        }
    };
    println!("{look:#?}");
    let defs = map::rom_file(rom, "data/prm/itemdt_def.gp2").context("itemdt_def.gp2")?;
    let defs = Gpc2::parse(defs)?;
    let entry = defs.find("itemdt_def.nat").context("itemdt_def.nat")?;
    let data = defs.member(entry)?;
    let models = ItemModels::parse(&data)?;
    let pc = Gpc2::parse(map::rom_file(rom, "data/pack_lv5/chara_pc.gp2").context("chara_pc")?)?;
    let has = |name: &str| pc.find(name).is_some();
    println!("{:#?}", Outfit::resolve(&look, &models, has));
    Ok(())
}
