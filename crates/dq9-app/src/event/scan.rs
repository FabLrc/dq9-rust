//! The headless run of an event (`EventPlayer::prepare`): which models, motion packages and
//! scene state its script needs, and the ROM files it reads.

use std::cell::OnceCell;
use std::collections::BTreeSet;

use dq9_core::cutscene::{Cutscene, Target};
use dq9_core::script::{Director, Event, Files, Frame};
use dq9_core::trigger::{Request, State};
use nds_rom::Rom;

use super::{LANGUAGE, MAX_UPDATES};

/// The ROM's files, parsed when the first one is asked for (a step that starts no event never
/// reads any).
pub(super) struct LazyRom<'a> {
    bytes: &'a [u8],
    rom: OnceCell<Option<Rom<'a>>>,
}

impl<'a> LazyRom<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            rom: OnceCell::new(),
        }
    }

    fn rom(&self) -> Option<&Rom<'a>> {
        self.rom
            .get_or_init(|| Rom::parse(self.bytes).ok())
            .as_ref()
    }
}

impl Files for LazyRom<'_> {
    fn file(&self, path: &str) -> Option<&[u8]> {
        self.rom()?.file_by_path(path).ok()
    }
}

/// The model one character of the script needs, found by a headless run.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Plan {
    pub(super) target: Target,
    pub(super) path: String,
    pub(super) packages: Vec<String>,
}

/// What a headless run of an event found out.
#[derive(Debug, Default)]
pub(super) struct Scan {
    pub(super) models: Vec<Plan>,
    /// Motion packages of the hero, in the order they are added.
    pub(super) hero_packages: Vec<String>,
    /// Updates the script took with every wait completed at once.
    pub(super) updates: u32,
    /// The script did not load or stopped on an error.
    pub(super) failure: Option<String>,
    /// Map NPCs the script gives a motion (`AnimatedScene::prepare_npc_motions`).
    pub(super) npc_motions: BTreeSet<u16>,
    /// Motion packages the script adds to map NPCs (natives 229, 567), by NPC id.
    pub(super) npc_packages: Vec<(u16, String)>,
    /// Field sprite ids of the balloons it puts up.
    pub(super) balloons: BTreeSet<i32>,
    /// Scene flag from the event's `EventScript` (no separate list parse).
    pub(super) scene_flag: Option<u16>,
}

/// Plays `event` headless up to its end, noting the models and packages each target gets.
/// `guest` is the model of the party's guest, which cast mode 10 binds.
pub(super) fn scan(rom: &[u8], event: u16, world: &State, guest: Option<String>) -> Scan {
    let files = LazyRom::new(rom);
    let mut director = Director::new(LANGUAGE);
    director.set_guest(guest);
    director.dispatch(vec![Request::Event(event)]);
    let mut cutscene = Cutscene::new();
    let mut out = Scan::default();
    'run: while out.updates < MAX_UPDATES {
        let events = director.step(&files, Frame::default(), world);
        out.updates += 1;
        if out.scene_flag.is_none() {
            out.scene_flag = director.scene_flag();
        }
        for e in &events {
            cutscene.apply(e);
            if let Some(balloon) = cutscene.balloon() {
                out.balloons.insert(balloon.sprite);
            }
            if let Some(signal) = e.instant_signal() {
                director.signal(signal);
            }
            match e {
                Event::Ended { .. } => {
                    note(&cutscene, &mut out);
                    break 'run;
                }
                Event::LoadFailed { reason, .. } => {
                    out.failure = Some(reason.clone());
                    break 'run;
                }
                Event::Fault(fault) => {
                    out.failure = Some(fault.to_string());
                    break 'run;
                }
                _ => {}
            }
        }
        note(&cutscene, &mut out);
        if !director.is_active() {
            break;
        }
    }
    out
}

fn note(cutscene: &Cutscene, out: &mut Scan) {
    for (target, node) in cutscene.nodes() {
        match target {
            Target::Hero => {
                for (path, _) in &node.packages {
                    if !out.hero_packages.contains(path) {
                        out.hero_packages.push(path.clone());
                    }
                }
            }
            Target::Model(_) | Target::Cast(_) | Target::Object(_) => {
                let Some(path) = &node.model else { continue };
                let plan = match out
                    .models
                    .iter_mut()
                    .find(|p| p.target == target && p.path == *path)
                {
                    Some(plan) => plan,
                    None => {
                        out.models.push(Plan {
                            target,
                            path: path.clone(),
                            packages: Vec::new(),
                        });
                        out.models.last_mut().expect("just pushed")
                    }
                };
                for (package, _) in &node.packages {
                    if !plan.packages.contains(package) {
                        plan.packages.push(package.clone());
                    }
                }
            }
            Target::Npc(id) => {
                if let Ok(id) = u16::try_from(id) {
                    if node.animation.is_some() {
                        out.npc_motions.insert(id);
                    }
                    for (path, _) in &node.packages {
                        if !out.npc_packages.contains(&(id, path.clone())) {
                            out.npc_packages.push((id, path.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The character archive of a monster (`enemy/z000a.mon`): the first `.cchr` of its member of
/// `data/pack_lv5/enemy.gp2`, LZ-decompressed (`LoadMonsterCharacter`, native 233).
pub(super) fn monster_archive(files: &LazyRom, path: &str) -> Option<Vec<u8>> {
    let member = path.strip_prefix("enemy/").unwrap_or(path);
    let enemies = dq9_formats::Gpc2::parse(files.file("data/pack_lv5/enemy.gp2")?).ok()?;
    let entry = enemies.entries.iter().find(|e| e.name == member)?;
    let mon = enemies.member(entry).ok()?;
    let monster = nds_rom::Narc::parse(&mon).ok()?;
    let cchr = monster
        .names
        .files
        .iter()
        .find(|e| e.path.ends_with(".cchr"))?;
    nds_rom::compress::decompress(monster.file(cchr.id).ok()?).ok()
}

/// [`monster_archive`] on the whole ROM image: the battle scene reads a monster's body
/// without an event (`crate::battle`).
pub(crate) fn monster_archive_bytes(rom: &[u8], path: &str) -> Option<Vec<u8>> {
    monster_archive(&LazyRom::new(rom), path)
}

/// The sheet of a sprite the field draws by itself (`fieldSpriteFiles`, `docs/re/stb-natives.md`).
pub(super) fn field_sprite(id: i32) -> Option<&'static str> {
    Some(match id {
        2 => "fuki_com",
        3 => "fuki_in",
        4 => "fuki_hkn",
        12 => "field_mon01",
        13 => "ev_mark",
        14 => "field_qu",
        15 => "fuki_com_q",
        16 => "fuki_com_apc",
        _ => return None,
    })
}

/// A sprite sheet of `data/bin/icon.nsarc`, where the field keeps its own.
pub(super) fn icon_sprite(files: &LazyRom, name: &str) -> Option<Vec<u8>> {
    let narc = nds_rom::Narc::parse(files.file("data/bin/icon.nsarc")?).ok()?;
    let wanted = format!("{name}.spr");
    let entry = narc
        .names
        .files
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case(&wanted))?;
    narc.file(entry.id).ok().map(<[u8]>::to_vec)
}
