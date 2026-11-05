//! The whole load chain of a map (`0x02013e28`): map list, `.ambl` (textures, block list,
//! zone settings), lighting, `.amdj` (manifests, then each resource's files and each
//! placement's instance), atmospheric effects.

use super::manifest::{Manifest, Placement, Resource, resource_flags as rf};
use super::transform::{Transform, hierarchy};
use super::{BlockList, LightingFile, MapFiles, ZoneInfo, contains, extension, lz77, member_name};
use crate::maplist::MapRecord;
use crate::table::Table;
use nds_rom::{Narc, Result};

/// Game state that changes what a map loads or shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// `LightingManager+0x98` (0 to 3): picks resource variants and visible instances.
    pub time_of_day: u32,
    /// `ActiveGrottoClass::GetActiveGrottoEnviron`, for grotto maps.
    pub grotto_environ: i32,
}

/// What a resource slot became (`Part+0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    /// `0xffff`: no resource in the slot, or its model is missing.
    None,
    /// 0: a model (`Model3D`, `0x02014d90`).
    Model,
    /// 1: a collision mesh (`0x02015144`).
    Collision,
    /// 2: an animated object (`Object3D`, `0x020151dc`).
    Object,
}

/// Animation files and the slot or loader they go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimKind {
    /// Skeletal animation (`Part+0x14`, `Object3D` type 0).
    Nsbca,
    /// Material animation (`Part+0x10`, type 1).
    Nsbma,
    /// Texture SRT animation (`Part+0xc`, type 3).
    Nsbta,
    /// Texture pattern animation (`Part+8`, type 2).
    Nsbtp,
    /// Animation package of an object (`Object3D::LoadType0AnimationPackageFromBCFGScript`).
    Bcfg,
}

impl AnimKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Nsbca => "nsbca",
            Self::Nsbma => "nsbma",
            Self::Nsbta => "nsbta",
            Self::Nsbtp => "nsbtp",
            Self::Bcfg => "bcfg",
        }
    }
}

/// Animations of a model, by resource flag (`data_020e6fb8`).
const MODEL_ANIMATIONS: [(u8, AnimKind); 4] = [
    (rf::NSBCA, AnimKind::Nsbca),
    (rf::NSBMA, AnimKind::Nsbma),
    (rf::NSBTA, AnimKind::Nsbta),
    (rf::NSBTP, AnimKind::Nsbtp),
];

/// Animations of an object, by resource flag (`data_020e6f58`).
const OBJECT_ANIMATIONS: [(u8, AnimKind); 5] = [
    (rf::NSBCA, AnimKind::Nsbca),
    (rf::NSBMA, AnimKind::Nsbma),
    (rf::NSBTA, AnimKind::Nsbta),
    (rf::NSBTP, AnimKind::Nsbtp),
    (rf::OBJECT, AnimKind::Bcfg),
];

/// An animation file a part loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Animation {
    pub kind: AnimKind,
    /// Member of the `.amdj`.
    pub member: String,
}

/// What the game builds from one resource slot (`Block+0x40`, 0x18 bytes each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub kind: PartKind,
    /// Resource name used: the resource's, or the variant's for the time of day.
    pub name: Vec<u8>,
    /// Model or collision member loaded (`.nsbmd`, `.col2`), when present.
    pub file: Option<String>,
    /// Animation members loaded, in load order; flagged ones that are missing are skipped.
    pub animations: Vec<Animation>,
    /// The `.nsbtx` (index in [`Map::textures`]) whose textures a model's animations use.
    pub texture_source: Option<usize>,
}

/// One placement as the game links it (`Block+0x44`, 0x70 bytes each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// `+0`: [`Placement::instance`].
    pub id: u16,
    /// `+0x24`: the part shown, if any.
    pub part: Option<usize>,
    /// `+0x28`: parent instance.
    pub parent: Option<usize>,
    /// `+2` bit 2: hidden at this time of day.
    pub hidden: bool,
    /// World transform (`+6`, `+8`, `+0x14`).
    pub world: Transform,
}

/// A loaded `.bmdj` (`Zone3D+0x41c` list, 0x58 bytes each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The `.amdj` it comes from.
    pub archive: String,
    /// Its `.bmdj` member.
    pub member: String,
    /// `+0`: id of the [`BlockEntry`](super::BlockEntry) that selected it.
    pub entry: u32,
    /// `+0x48`: that entry's offset. `0x020177e4` adds it to root translations into a value
    /// it never stores: it moves nothing there.
    pub offset: [i32; 3],
    /// `+4`.
    pub manifest: Manifest,
    /// Whether the game built its parts and instances: all blocks are, except the older
    /// ones when the second ocean/sky archive adds several.
    pub processed: bool,
    /// One per resource slot ([`Manifest::resource_capacity`]).
    pub parts: Vec<Part>,
    /// One per placement.
    pub instances: Vec<Instance>,
}

/// Everything a map loads, in the game's structures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    pub files: MapFiles,
    /// Whether the `.ambl` was found; without it the game loads nothing more.
    pub ambl_found: bool,
    /// `Zone3D+0xc`.
    pub zone: ZoneInfo,
    /// `.nsbtx` members of the `.ambl`, newest first (`Zone3D+0x418`).
    pub textures: Vec<String>,
    /// `Zone3D+0x6c`.
    pub block_list: BlockList,
    /// Lighting script queued, and whether it was found (`0x02014c14`).
    pub lighting: Option<(LightingFile, bool)>,
    /// `.amdj` archives loaded.
    pub amdj_loaded: Vec<String>,
    /// Blocks, newest first (`Zone3D+0x41c`).
    pub blocks: Vec<Block>,
    /// Atmospheric effects archive queued once the last `.amdj` is loaded
    /// (`ZoneInfo::effects_file`).
    pub effects: Option<String>,
}

/// A member of the mounted archive by path (`GetFileFromNARCInMemory`, `0x020afd1c`): the
/// SDK's file system compares names without case (`FSi_StrNICmp`), and resources rely on it
/// (`m08M0600.imd` is `M08M0600.nsbmd`).
fn member<'a>(narc: &Narc<'a>, name: &str) -> Option<&'a [u8]> {
    let entry = narc
        .names
        .files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case(name))?;
    narc.file(entry.id).ok()
}

/// A member by exact name (`GetFileInNarc`, `0x02075258`: `strcmp`).
fn member_exact<'a>(narc: &Narc<'a>, name: &str) -> Option<&'a [u8]> {
    let entry = narc.names.find(name)?;
    narc.file(entry.id).ok()
}

/// Named members of a NARC in index order (`PrepareReadFileInNARCByID`).
fn members<'n, 'a>(narc: &'n Narc<'a>) -> impl Iterator<Item = (&'n str, Result<&'a [u8]>)> {
    narc.names
        .files
        .iter()
        .map(move |f| (f.path.as_str(), narc.file(f.id)))
}

impl Map {
    /// Assembles map `record`: `map_list` is the parsed `maplist9.bin`, `file` returns a ROM
    /// file by path, compared without case like the game ([`rom_file`](super::rom_file)).
    /// Errors on malformed archives or tables, where the game would read garbage.
    pub fn assemble<'r>(
        record: &MapRecord<'_>,
        map_list: &Table<'_>,
        file: impl Fn(&str) -> Option<&'r [u8]>,
        ctx: &Context,
    ) -> Result<Self> {
        let id = record.id;
        let files = MapFiles::new(id, record.code, ctx.grotto_environ);
        // Zone3D::SwitchZone, then 0x0201c734 on the map list it queued.
        let mut zone = ZoneInfo::default();
        zone.load_map_list(map_list, id)?;
        let mut map = Self {
            files,
            ambl_found: false,
            zone,
            textures: Vec::new(),
            block_list: BlockList::default(),
            lighting: None,
            amdj_loaded: Vec::new(),
            blocks: Vec::new(),
            effects: None,
        };
        // 0x02014118: a failed load stops the chain.
        let Some(ambl) = file(&map.files.ambl) else {
            return Ok(map);
        };
        map.ambl_found = true;
        map.load_ambl(&Narc::parse(ambl)?)?;
        // 0x02014b14, 0x02014c14.
        if let Some(lighting) = map.zone.lighting_file(id, ctx.grotto_environ) {
            let found = file(&lighting.archive)
                .and_then(|a| Narc::parse(a).ok())
                .is_some_and(|narc| member_exact(&narc, &lighting.member).is_some());
            map.lighting = Some((lighting, found));
        }
        // 0x020145b8 then 0x0201470c, once per archive.
        // A failed load ends the chain, before the effects.
        for (index, path) in map.files.amdj.clone().iter().enumerate() {
            let Some(amdj) = file(path) else {
                return Ok(map);
            };
            map.amdj_loaded.push(path.clone());
            // The second ocean/sky archive (Zone3D+0x42c == 2) builds one block only.
            let only_newest = index == 1;
            map.load_amdj(path, &Narc::parse(amdj)?, only_newest, ctx)?;
        }
        map.effects = map.zone.effects_file();
        Ok(map)
    }

    /// `0x02014118`: every `.nsbtx` first (`0x0201446c`), then `.bmbl` (`0x020143a0`),
    /// `.dat` (`0x0205e114`) and `.bpos` (`0x020143e8`) in member order.
    fn load_ambl(&mut self, narc: &Narc<'_>) -> Result<()> {
        for (name, _) in members(narc) {
            if extension(name) == Some(".nsbtx") {
                self.textures.insert(0, name.to_owned());
            }
        }
        for (name, data) in members(narc) {
            match extension(name) {
                Some(".bmbl") => {
                    self.block_list.clear();
                    self.block_list.load(&Table::parse(&lz77(data?)?)?)?;
                }
                Some(".dat") => self.zone.load_dat(&Table::parse(&lz77(data?)?)?)?,
                Some(".bpos") => self.block_list.load(&Table::parse(&lz77(data?)?)?)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// `0x0201470c`: loads every `.bmdj` member once per block entry whose name it contains
    /// (`0x02014910`), then builds the blocks from the list head (`0x02014a34`, `0x020177e4`)
    /// with this archive mounted: all of them, or only the head for the second ocean/sky
    /// archive.
    fn load_amdj(
        &mut self,
        path: &str,
        narc: &Narc<'_>,
        only_newest: bool,
        ctx: &Context,
    ) -> Result<()> {
        for (name, data) in members(narc) {
            if extension(name) != Some(".bmdj") {
                continue;
            }
            let data = data?;
            for entry in &self.block_list.entries {
                if !contains(name.as_bytes(), &entry.name) {
                    continue;
                }
                let manifest = Manifest::parse(&Table::parse(&lz77(data)?)?)?;
                self.blocks.insert(
                    0,
                    Block {
                        archive: path.to_owned(),
                        member: name.to_owned(),
                        entry: entry.id,
                        offset: entry.offset,
                        manifest,
                        processed: false,
                        parts: Vec::new(),
                        instances: Vec::new(),
                    },
                );
            }
        }
        let count = if only_newest {
            self.blocks.len().min(1)
        } else {
            self.blocks.len()
        };
        for block in &mut self.blocks[..count] {
            block.parts = parts(&block.manifest, narc, &self.textures, ctx);
            block.instances = instances(&block.manifest, &block.parts, ctx)?;
            block.processed = true;
        }
        Ok(())
    }
}

/// `0x02014a34`: one part per resource slot. A name whose 4th character is `A` is a
/// collision, flag [`OBJECT`](rf::OBJECT) an object, anything else a model.
fn parts(manifest: &Manifest, narc: &Narc<'_>, textures: &[String], ctx: &Context) -> Vec<Part> {
    (0..manifest.resource_capacity)
        .map(|i| match manifest.resources.get(i) {
            None => Part {
                kind: PartKind::None,
                name: Vec::new(),
                file: None,
                animations: Vec::new(),
                texture_source: None,
            },
            Some(r) if r.name.get(3) == Some(&b'A') => {
                // 0x02015144: a collision even when its file is missing.
                let name = member_name(&r.name, "col2");
                Part {
                    kind: PartKind::Collision,
                    name: r.name.clone(),
                    file: member(narc, &name).map(|_| name),
                    animations: Vec::new(),
                    texture_source: None,
                }
            }
            Some(r) if r.flags & rf::OBJECT == 0 => model(manifest, r, narc, textures, ctx),
            Some(r) => object(r, narc),
        })
        .collect()
}

/// Animation members of `name` selected by `flags`, those present only.
fn animations(name: &[u8], flags: u8, table: &[(u8, AnimKind)], narc: &Narc<'_>) -> Vec<Animation> {
    table
        .iter()
        .filter(|(mask, _)| flags & mask != 0)
        .map(|&(_, kind)| (kind, member_name(name, kind.extension())))
        .filter(|(_, m)| member(narc, m).is_some())
        .map(|(kind, member)| Animation { kind, member })
        .collect()
}

/// `0x02014d90`: the variant for the time of day if any, its `.nsbmd`, then the texture
/// archive of its first linked texture (bit order) that some `.nsbtx` name contains, then
/// its flagged animations.
fn model(
    manifest: &Manifest,
    r: &Resource,
    narc: &Narc<'_>,
    textures: &[String],
    ctx: &Context,
) -> Part {
    let (name, flags, links) = r
        .variants
        .iter()
        .find(|v| i32::from(v.time_of_day as i16) == ctx.time_of_day as i32)
        .map_or((&r.name, r.flags, r.textures), |v| {
            (&v.name, v.flags, v.textures)
        });
    let nsbmd = member_name(name, "nsbmd");
    if member(narc, &nsbmd).is_none() {
        return Part {
            kind: PartKind::None,
            name: name.clone(),
            file: None,
            animations: Vec::new(),
            texture_source: None,
        };
    }
    let texture_source = (0..8)
        .filter(|bit| links & (1 << bit) != 0)
        .filter_map(|bit| manifest.textures.get(bit))
        .find_map(|link| textures.iter().position(|t| contains(t.as_bytes(), link)));
    Part {
        kind: PartKind::Model,
        name: name.clone(),
        file: Some(nsbmd),
        animations: animations(name, flags, &MODEL_ANIMATIONS, narc),
        texture_source,
    }
}

/// `0x020151dc`: the `.nsbmd`, then the flagged animations and the `.bcfg` package.
fn object(r: &Resource, narc: &Narc<'_>) -> Part {
    let nsbmd = member_name(&r.name, "nsbmd");
    let found = member(narc, &nsbmd).is_some();
    Part {
        kind: if found {
            PartKind::Object
        } else {
            PartKind::None
        },
        name: r.name.clone(),
        animations: if found {
            animations(&r.name, r.flags, &OBJECT_ANIMATIONS, narc)
        } else {
            Vec::new()
        },
        file: found.then_some(nsbmd),
        texture_source: None,
    }
}

/// `0x020177e4`: links each placement to its part and parent, hides it for the time of day,
/// and computes the world transforms.
fn instances(manifest: &Manifest, parts: &[Part], ctx: &Context) -> Result<Vec<Instance>> {
    if manifest.placements.len() < manifest.placement_capacity {
        // The game would read placements past the end of the list.
        return Err(nds_rom::Error::Malformed(
            "bmdj declares more placements than it holds",
        ));
    }
    let nodes = hierarchy(&manifest.placements)?;
    Ok(manifest
        .placements
        .iter()
        .zip(nodes)
        .map(|(p, node)| Instance {
            id: node.id,
            part: part_of(p, manifest, parts),
            parent: node.parent,
            hidden: !p.visible(ctx.time_of_day),
            world: node.world,
        })
        .collect())
}

/// The part a placement shows: a negative resource takes the slot numbered by its instance;
/// a collision without its file shows nothing.
fn part_of(p: &Placement, manifest: &Manifest, parts: &[Part]) -> Option<usize> {
    let index = if p.resource < 0 {
        usize::try_from(p.instance as i16).ok()?
    } else {
        manifest.resource_index(p.resource)?
    };
    let part = parts.get(index)?;
    (part.kind == PartKind::None || part.file.is_some()).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::OCEAN;

    enum V {
        I(u32),
        F(f32),
        S(&'static str),
    }
    use V::{F, I, S};

    /// A record table (`table.md`).
    fn table(records: &[(u16, Vec<V>)]) -> Vec<u8> {
        let (mut body, mut pool) = (Vec::new(), Vec::new());
        for (kind, values) in records {
            body.extend(kind.to_le_bytes());
            body.push(values.len() as u8);
            let mut types = vec![0u8; values.len().div_ceil(4)];
            for (i, v) in values.iter().enumerate() {
                let t = match v {
                    S(_) => 0,
                    I(_) => 1,
                    F(_) => 2,
                };
                types[i / 4] |= t << (2 * (i % 4));
            }
            body.extend(types);
            body.resize(body.len().next_multiple_of(4), 0xff);
            for v in values {
                let raw = match *v {
                    S(s) => {
                        let at = pool.len() as u32;
                        pool.extend(s.as_bytes());
                        pool.push(0);
                        at
                    }
                    I(i) => i,
                    F(f) => f.to_bits(),
                };
                body.extend(raw.to_le_bytes());
            }
        }
        let pool_at = (0x10 + body.len()).next_multiple_of(16);
        let mut out = Vec::new();
        for word in [records.len(), pool_at, pool.len(), 0] {
            out.extend((word as u32).to_le_bytes());
        }
        out.extend(body);
        out.resize(pool_at, 0xff);
        out.extend(pool);
        out.resize(out.len().next_multiple_of(16), 0);
        out
    }

    /// LZ10 stream of literals only.
    fn lz(data: &[u8]) -> Vec<u8> {
        let mut out = (0x10 | (data.len() as u32) << 8).to_le_bytes().to_vec();
        for chunk in data.chunks(8) {
            out.push(0);
            out.extend(chunk);
        }
        out
    }

    /// A NARC with named members.
    fn narc(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut btaf = (members.len() as u32).to_le_bytes().to_vec();
        let mut image = Vec::new();
        let mut names = Vec::new();
        for (name, data) in members {
            btaf.extend((image.len() as u32).to_le_bytes());
            image.extend(data);
            btaf.extend((image.len() as u32).to_le_bytes());
            names.push(name.len() as u8);
            names.extend(name.as_bytes());
        }
        names.push(0);
        let btnf = [8u32.to_le_bytes(), [0, 0, 1, 0]].concat();
        let btnf = [btnf, names].concat();
        let mut out = b"NARC".to_vec();
        out.extend(0xFFFEu16.to_le_bytes());
        out.extend(0x0100u16.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(3u16.to_le_bytes());
        for (magic, body) in [(b"BTAF", &btaf), (b"BTNF", &btnf), (b"GMIF", &image)] {
            out.extend(magic);
            out.extend(((body.len() + 8) as u32).to_le_bytes());
            out.extend(body);
        }
        out
    }

    fn placement(instance: u32, resource: u32, parent: u32, x: f32, flags: u32) -> (u16, Vec<V>) {
        let mut v = vec![
            I(instance),
            I(resource),
            F(x),
            F(0.0),
            F(0.0),
            I(parent),
            S(""),
        ];
        v.extend([F(1.0), F(1.0), F(1.0), F(0.0), F(0.0), F(0.0), I(flags)]);
        (0x6f, v)
    }

    #[test]
    fn ocean_assembles_like_the_game() {
        let mut map_values: Vec<V> = (0..22).map(|_| I(0)).collect();
        map_values[0] = I(u32::from(OCEAN));
        map_values[4] = S("O00");
        map_values[11] = S("Q01");
        let map_list = table(&[(0x67, map_values)]);
        let ambl = narc(&[
            ("O00T1.nsbtx", vec![]),
            (
                "O00.bmbl",
                lz(&table(&[
                    (0x64, vec![I(2)]),
                    (0x65, vec![I(0), F(1.0), F(0.0), F(0.0), S("O00b")]),
                    (0x65, vec![I(1), F(0.0), F(0.0), F(0.0), S("O00a")]),
                ])),
            ),
            ("O00.dat", lz(&table(&[(0x6d, vec![S("FX")])]))),
            ("O00T2.nsbtx", vec![]),
        ]);
        let b = narc(&[
            (
                "O00b.bmdj",
                lz(&table(&[
                    (0x70, vec![S("T2")]),
                    (0x6a, vec![I(3)]),
                    (0x6c, vec![I(0), S("O00M0001.imd"), I(0x05), I(1)]),
                    (0x6c, vec![I(1), S("O00A0000.imd")]),
                    (0x6c, vec![I(2), S("o00m0002.imd"), I(0x12)]),
                    (0x6d, vec![I(4)]),
                    placement(0, 0, u32::MAX, 1.0, 63),
                    placement(1, 1, 0, 0.5, 63),
                    placement(2, 2, 0, 0.5, 47),
                    placement(3, u32::MAX, u32::MAX, 2.0, 63),
                ])),
            ),
            ("O00M0001.nsbmd", vec![]),
            ("O00M0001.nsbta", vec![]),
            ("O00M0002.nsbmd", vec![]),
            ("O00M0002.nsbma", vec![]),
            ("O00M0002.bcfg", vec![]),
        ]);
        let one_block = lz(&table(&[
            (0x6a, vec![I(1)]),
            (0x6c, vec![I(0), S("O00M0003.imd")]),
            (0x6d, vec![I(1)]),
            placement(0, 0, u32::MAX, 0.0, 63),
        ]));
        let a = narc(&[
            ("O00a.bmdj", one_block.clone()),
            ("O00a2.bmdj", one_block),
            ("O00M0003.nsbmd", vec![]),
        ]);
        let ats = narc(&[("Q01.bats", vec![])]);
        let files = |path: &str| -> Option<&[u8]> {
            Some(match path {
                "data/map/O00.ambl" => &ambl,
                "data/map/O00b.amdj" => &b,
                "data/map/O00a.amdj" => &a,
                "data/map/ats_Q.ambl" => &ats,
                _ => return None,
            })
        };
        let record = MapRecord {
            id: OCEAN,
            area: 0,
            flag_4: false,
            code: b"O00",
            byte_c: 0,
            byte_d: 0,
            byte_e: 0,
            region: None,
            label: None,
        };
        let map_list = Table::parse(&map_list).unwrap();
        let ctx = Context {
            time_of_day: 1,
            grotto_environ: 0,
        };
        let map = Map::assemble(&record, &map_list, files, &ctx).unwrap();
        assert_eq!(map.textures, ["O00T2.nsbtx", "O00T1.nsbtx"]);
        let (lighting, found) = map.lighting.clone().unwrap();
        assert!(found);
        assert_eq!(lighting.member, "Q01.bats");
        assert_eq!(
            map.amdj_loaded,
            ["data/map/O00b.amdj", "data/map/O00a.amdj"]
        );
        assert_eq!(map.effects.as_deref(), Some("data/map/FX.mse"));
        // Newest first: the second `a` manifest, the first, then `b`.
        let members: Vec<_> = map.blocks.iter().map(|b| b.member.as_str()).collect();
        assert_eq!(members, ["O00a2.bmdj", "O00a.bmdj", "O00b.bmdj"]);
        let built: Vec<_> = map.blocks.iter().map(|b| b.processed).collect();
        assert_eq!(built, [true, false, true]);
        let block = &map.blocks[2];
        assert_eq!((block.entry, block.offset), (0, [4096, 0, 0]));
        let [model, collision, object] = [&block.parts[0], &block.parts[1], &block.parts[2]];
        assert_eq!(model.kind, PartKind::Model);
        assert_eq!(model.file.as_deref(), Some("O00M0001.nsbmd"));
        // NSBCA is flagged but missing.
        assert_eq!(model.animations.len(), 1);
        assert_eq!(model.animations[0].kind, AnimKind::Nsbta);
        assert_eq!(model.texture_source, Some(0));
        assert_eq!(
            (collision.kind, collision.file.as_ref()),
            (PartKind::Collision, None)
        );
        // Lookups in the mounted archive ignore case.
        assert_eq!(object.kind, PartKind::Object);
        assert_eq!(object.file.as_deref(), Some("o00m0002.nsbmd"));
        let anims: Vec<_> = object.animations.iter().map(|a| a.kind).collect();
        assert_eq!(anims, [AnimKind::Nsbma, AnimKind::Bcfg]);
        let parts: Vec<_> = block.instances.iter().map(|i| i.part).collect();
        // The collision has no file; a negative resource takes slot `instance` (3: none).
        assert_eq!(parts, [Some(0), None, Some(2), None]);
        let hidden: Vec<_> = block.instances.iter().map(|i| i.hidden).collect();
        // 47 has bit 4 clear: stored with bit 4 set, always hidden.
        assert_eq!(hidden, [false, false, true, false]);
        let x: Vec<_> = block
            .instances
            .iter()
            .map(|i| i.world.position[0])
            .collect();
        // Children add their parent's position; the block offset is not applied.
        assert_eq!(x, [4096, 6144, 6144, 8192]);
    }

    #[test]
    fn a_missing_ambl_stops_the_chain() {
        let map_list = table(&[]);
        let map_list = Table::parse(&map_list).unwrap();
        let record = MapRecord {
            id: 1,
            area: 0,
            flag_4: false,
            code: b"M01",
            byte_c: 0,
            byte_d: 0,
            byte_e: 0,
            region: None,
            label: None,
        };
        let map = Map::assemble(&record, &map_list, |_| None, &Context::default()).unwrap();
        assert!(!map.ambl_found && map.blocks.is_empty() && map.lighting.is_none());
    }
}
