//! Format invariants on the real game data; skipped when the ROM is absent.

use std::collections::HashSet;

use dq9_formats::dialogue::{self, Kind, Session, TalkState};
use dq9_formats::map::{self, Context, Map, PartKind};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::stb::{Instr, Script};
use dq9_formats::trigger;
use dq9_formats::{Font, FontId, Gpc2, Table, Value, level5};
use nds_rom::Rom;

/// Every collision member in the ROM obeys the complete `.col2` table invariants.
#[test]
fn collision_meshes_parse() {
    use nds_rom::Narc;

    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut meshes = 0;
    let mut triangles = 0;
    for archive_file in rom
        .files
        .files
        .iter()
        .filter(|file| file.path.ends_with(".amdj"))
    {
        let archive = Narc::parse(rom.file(archive_file.id).unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", archive_file.path));
        for entry in archive
            .names
            .files
            .iter()
            .filter(|entry| entry.path.ends_with(".col2"))
        {
            let data = archive.file(entry.id).unwrap();
            let unpacked = nds_rom::compress::decompress(data)
                .unwrap_or_else(|error| panic!("{}/{}: {error}", archive_file.path, entry.path));
            let collision = map::Collision::parse(&unpacked).unwrap_or_else(|error| {
                panic!(
                    "{}/{} header {:02x?}: {error}",
                    archive_file.path,
                    entry.path,
                    &unpacked[..unpacked.len().min(16)]
                )
            });
            meshes += 1;
            triangles += collision.triangles.len();
        }
    }
    assert_eq!(meshes, 1178);
    assert_eq!(triangles, 109_122);
}

/// Every lighting script in the ROM decompresses (LZ10) and parses: 504 `.bats` members in the
/// 14 `ats_*.ambl` archives.
#[test]
fn lighting_scripts_parse() {
    use dq9_formats::lighting::LightingInfo;
    use nds_rom::Narc;

    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut scripts = 0;
    for archive_file in rom
        .files
        .files
        .iter()
        .filter(|file| file.path.starts_with("data/map/ats_") && file.path.ends_with(".ambl"))
    {
        let archive = Narc::parse(rom.file(archive_file.id).unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", archive_file.path));
        for entry in archive
            .names
            .files
            .iter()
            .filter(|entry| entry.path.ends_with(".bats"))
        {
            let data = archive.file(entry.id).unwrap();
            let unpacked = nds_rom::compress::decompress(data)
                .unwrap_or_else(|error| panic!("{}/{}: {error}", archive_file.path, entry.path));
            LightingInfo::parse(&unpacked)
                .unwrap_or_else(|error| panic!("{}/{}: {error}", archive_file.path, entry.path));
            scripts += 1;
        }
    }
    assert_eq!(scripts, 504);
}

/// Calls `f(archive path, member name, decompressed bytes)` for every GPC2 member.
fn for_each_member(rom: &Rom, mut f: impl FnMut(&str, &str, &[u8])) {
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let archive = Gpc2::parse(rom.file(file.id).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", file.path));
        for entry in &archive.entries {
            let content = archive
                .member(entry)
                .unwrap_or_else(|e| panic!("{}/{}: {e}", file.path, entry.name));
            f(&file.path, &entry.name, &content);
        }
    }
}

/// Every event script decodes: code reachable from its entries holds only known opcodes,
/// each function's code follows its record, and every native call's id is a constant.
#[test]
fn event_scripts_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut scripts, mut natives, mut ids) = (0, 0, HashSet::new());
    for_each_member(&rom, |archive, name, data| {
        if !name.ends_with(".stb") {
            return;
        }
        let script = Script::parse(data).unwrap_or_else(|e| panic!("{archive}/{name}: {e}"));
        for function in script.functions.values() {
            for (i, id) in function.native_calls() {
                let id = id.unwrap_or_else(|| panic!("{archive}/{name}: native at {i}"));
                ids.insert(id);
                natives += 1;
            }
            for (_, instr) in &function.code {
                if let Instr::Str(offset) = *instr {
                    assert!(script.string(offset).is_some(), "{archive}/{name}");
                }
            }
        }
        scripts += 1;
    });
    assert_eq!(scripts, 689);
    assert_eq!(natives, 80_237);
    // 228 of the 304 natives registered in overlay 1 (`ov_001::02164d6c`).
    assert_eq!(ids.len(), 228);
}

/// Every `.gp2` is a GPC2 archive whose index hashes match its names (checked by `parse`) and
/// whose members all decompress to their declared size.
#[test]
fn all_gp2_archives_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut archives = 0;
    let mut members = 0;
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let archive = Gpc2::parse(rom.file(file.id).unwrap()).unwrap();
        for entry in &archive.entries {
            let content = archive.member(entry).unwrap();
            if let (Ok((_, size)), std::borrow::Cow::Owned(_)) =
                (level5::header(archive.stored(entry)), &content)
            {
                assert_eq!(content.len(), size, "{}/{}", file.path, entry.name);
            }
            members += 1;
        }
        archives += 1;
    }
    assert_eq!(archives, 1671);
    assert_eq!(members, 53_639);
}

/// Record tables: every string offset starts a string (checked by `parse`), the header's
/// string count equals the number of distinct strings referenced, and the known population
/// parses.
#[test]
fn record_tables_parse() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut tables = 0;
    for_each_member(&rom, |archive, name, data| {
        if !(name.ends_with(".bin") || name.ends_with(".bmmp") || name.ends_with(".bcfg")) {
            return;
        }
        let Ok(table) = Table::parse(data) else {
            return;
        };
        let strings: HashSet<*const u8> = table
            .records
            .iter()
            .flat_map(|r| &r.values)
            .filter_map(|v| match v {
                Value::Str(Some(s)) => Some(s.as_ptr()),
                _ => None,
            })
            .collect();
        assert_eq!(
            strings.len(),
            table.string_count as usize,
            "{archive}/{name}"
        );
        tables += 1;
    });
    // 47 075 `.bin` + 283 `.bmmp` + 28 `.bcfg`; the other 152 `.bin` use other formats.
    assert_eq!(tables, 47_386);
}

/// NPC dialogue files: every record has a handled kind, a text, a quest id and condition in
/// the ranges the game tests, and a slot in groups 0-2.
#[test]
fn dialogue_files_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut files, mut lines) = (0, 0);
    let mut kinds = HashSet::new();
    for_each_member(&rom, |archive, name, data| {
        let is_dialogue = archive.starts_with("data/scenario/")
            && name.len() == 10
            && name[..3].bytes().all(|b| b.is_ascii_digit())
            && name.ends_with(".bin");
        if !is_dialogue {
            return;
        }
        files += 1;
        let table = Table::parse(data).unwrap();
        for line in dialogue::parse(&table).unwrap_or_else(|e| panic!("{archive}/{name}: {e}")) {
            lines += 1;
            let kind = line
                .kind
                .unwrap_or_else(|| panic!("{archive}/{name}: unknown kind"));
            kinds.insert(format!("{kind:?}"));
            assert!(line.text.is_some(), "{archive}/{name}");
            assert!(
                (0..240).contains(&line.slot),
                "{archive}/{name}: slot {}",
                line.slot
            );
            if matches!(kind, Kind::Quest | Kind::HostQuest) {
                assert!(
                    (0..0xcc).contains(&line.a),
                    "{archive}/{name}: quest {}",
                    line.a
                );
                assert!(
                    (-1..=4).contains(&line.b),
                    "{archive}/{name}: state {}",
                    line.b
                );
            }
        }
    });
    assert_eq!(files, 42_305);
    assert_eq!(lines, 189_940);
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort();
    assert_eq!(kinds, ["HostStep", "Quest", "SessionStepB", "Step"]);
}

/// George (Stornway, chapter M, NPC 18) once quest 11 is accepted: the in-progress line.
#[test]
fn dialogue_selection_follows_quest_state() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = Gpc2::parse(rom.file_by_path("data/scenario/M02M0.gp2").unwrap()).unwrap();
    let data = archive.member(archive.find("018_en.bin").unwrap()).unwrap();
    let table = Table::parse(&data).unwrap();
    let lines = dialogue::parse(&table).unwrap();
    let quest = |state: u8| move |q: i32| if q == 11 { state } else { 0 };
    let accepted = quest(2);
    let mut talk = TalkState {
        step: 1,
        substep: 0,
        context: 0,
        talk_count: [0, 0],
        daytime: true,
        session: Session::default(),
        quest: &accepted,
    };
    let chosen = dialogue::select(&lines, &talk).unwrap();
    let text = lines[chosen].text.unwrap();
    assert!(text.starts_with(b"<QUEST=11>//George// Those betterflies"));
    // Quest done (state 3): the closing line.
    let done = quest(3);
    talk.quest = &done;
    let chosen = dialogue::select(&lines, &talk).unwrap();
    assert!(
        lines[chosen]
            .text
            .unwrap()
            .starts_with(b"//George// I suppose those monsters")
    );
}

/// Every scenario trigger file parses, and every talk trigger names a dialogue context.
#[test]
fn trigger_files_parse() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut files, mut triggers) = (0, 0);
    for file in rom
        .files
        .files
        .iter()
        .filter(|f| f.path.starts_with("data/scenario/trigger") && f.path.ends_with(".bin"))
    {
        let table = Table::parse(rom.file(file.id).unwrap()).unwrap();
        let parsed = trigger::parse(&table).unwrap_or_else(|e| panic!("{}: {e}", file.path));
        for t in &parsed {
            if let Some((_, context)) = t.talk() {
                assert!(
                    (0..240).contains(&context),
                    "{}: context {context}",
                    file.path
                );
            }
        }
        files += 1;
        triggers += parsed.len();
    }
    assert_eq!(files, 76);
    assert_eq!(triggers, 5_895);
}

/// Both fonts parse, and every accented-letter tag used in the European texts (`<'e>`,
/// `<^a>`, `<:O>`, `<,c>`, `<~n>`…) is a glyph of the message font.
#[test]
fn fonts_cover_accent_tags() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let font = |id: FontId| {
        let info = rom
            .file_by_path(&format!("data/pack_lv5/fi_{}.bin", id.name()))
            .unwrap();
        let data = rom
            .file_by_path(&format!("data/pack_lv5/fd_{}.bin", id.name()))
            .unwrap();
        Font::parse(info, data, id).unwrap()
    };
    assert_eq!(font(FontId::Small).glyphs.len(), 245);
    let message = font(FontId::Message);
    assert_eq!(message.glyphs.len(), 242);

    let mut tags = HashSet::new();
    for_each_member(&rom, |_, name, data| {
        let Ok(table) = Table::parse(data) else {
            return;
        };
        if !name.ends_with(".bin") {
            return;
        }
        for value in table.records.iter().flat_map(|r| &r.values) {
            let Value::Str(Some(s)) = value else { continue };
            for w in s.windows(4) {
                if w[0] == b'<'
                    && b"`'^:~,".contains(&w[1])
                    && w[2].is_ascii_alphabetic()
                    && w[3] == b'>'
                {
                    tags.insert(w.to_vec());
                }
            }
        }
    });
    assert!(tags.len() > 40, "only {} accent tags found", tags.len());
    for tag in &tags {
        let index = message
            .find(tag)
            .unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(tag)));
        assert_eq!(message.glyphs[usize::from(index)].text, tag.as_slice());
    }
}

/// Every dialogue/menu string of the five languages goes through the whole message pipeline;
/// afterwards, text bytes are glyphs, spaces, or tags the pipeline leaves to other systems.
#[test]
fn message_pipeline_leaves_only_glyphs() {
    use dq9_formats::message::{self, Name, Token};
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let info = rom.file_by_path("data/pack_lv5/fi_me.bin").unwrap();
    let data = rom.file_by_path("data/pack_lv5/fd_me.bin").unwrap();
    let font = Font::parse(info, data, FontId::Message).unwrap();
    let mut leftovers = std::collections::BTreeMap::<String, usize>::new();
    let mut messages = 0;
    for_each_member(&rom, |_, name, data| {
        if !name.ends_with(".bin") {
            return;
        }
        let Ok(table) = Table::parse(data) else {
            return;
        };
        for value in table.records.iter().flat_map(|r| &r.values) {
            let Value::Str(Some(s)) = value else { continue };
            let text = message::expand_values(s, |_| b"1".to_vec());
            let text = message::normalize(&font, &text);
            let text = message::eval_conditionals(&text, |_| 0);
            let text = message::expand_names(&text, |n| match n {
                Name::Substitution { .. } | Name::Inflected { .. } => b"Name".to_vec(),
                Name::Weapon | Name::Actor => b"Name".to_vec(),
            });
            let text = message::apply_cap(&font, &text);
            for token in message::compile(&text) {
                let Token::Text(bytes) = token else { continue };
                let mut rest = bytes.as_slice();
                while !rest.is_empty() {
                    if let Some(i) = font.find(rest) {
                        rest = &rest[font.glyphs[usize::from(i)].text.len()..];
                    } else if rest[0] == b' ' {
                        rest = &rest[1..];
                    } else {
                        let end = message::find_tag_end(rest).filter(|_| rest[0] == b'<');
                        let len = end.map_or(1, |e| e + 1);
                        let key: String = String::from_utf8_lossy(&rest[..len])
                            .chars()
                            .map(|c| if c.is_ascii_digit() { '#' } else { c })
                            .collect();
                        *leftovers.entry(key).or_default() += 1;
                        rest = &rest[len..];
                    }
                }
            }
            messages += 1;
        }
    });
    assert_eq!(messages, 272_484);
    // Tags left for other systems (see docs/re/text.md), plus two typos in the game data
    // (`<ADDRESSE>`, `<END_IF_MALE>` with its block) and `<END>`/`<END_R_TURN>` right after a
    // tag `Msg_Compile` drops (it then copies the next '<' as text, like the game).
    let expected = [
        "<ADDRESSE>",
        "<CEN>",
        "<CURSOR=#>",
        "<DAMA>",
        "<ELSE_NOT_MALE>",
        "<END>",
        "<END_IF_MALE>",
        "<END_R_TURN>",
        "<IF_MALE>",
        "<LG>",
        "<LUIDA>",
        "<PLTT=##>",
        "<PLTT=#>",
        "<RENKIN>",
        "<RIKKA>",
        "<TC##>",
        "<TC#>",
        "<TRI-L>",
        "<TRI-R>",
        "<W=###>",
        "<W=##>",
    ];
    assert_eq!(
        leftovers.keys().map(String::as_str).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn map_list_loads() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    assert_eq!(list.records.len(), 1_010);
    let mut ids = HashSet::new();
    let (mut shipped, mut with_archive) = (0, 0);
    for r in list.records.iter().filter(|r| r.id != 0) {
        assert!(ids.insert(r.id), "map id {} twice", r.id);
        let code = std::str::from_utf8(r.code).unwrap();
        shipped += 1;
        if rom.file_by_path(&format!("data/map/{code}.amdj")).is_ok() {
            with_archive += 1;
        }
    }
    // Ids are unique on shipped maps. 155 of the 179 without a `.amdj` are grotto floors
    // (kind 6), whose maps are generated.
    assert_eq!((shipped, with_archive), (871, 692));
}

/// Counts over assembled maps.
#[derive(Debug, Default, PartialEq, Eq)]
struct MapTotals {
    maps: usize,
    without_ambl: usize,
    lighting: usize,
    lighting_missing: usize,
    effects: usize,
    doorways: usize,
    blocks: usize,
    unbuilt_blocks: usize,
    resources: usize,
    texture_links: usize,
    placements: usize,
    models: usize,
    objects: usize,
    collisions: usize,
    animations: usize,
    flagged_but_missing: usize,
    texture_sources: usize,
    with_parent: usize,
    hidden_at_0: usize,
    unreached: usize,
}

impl MapTotals {
    fn add(&mut self, map: &Map) {
        self.maps += 1;
        if !map.ambl_found {
            self.without_ambl += 1;
        }
        if let Some((_, found)) = &map.lighting {
            self.lighting += 1;
            self.lighting_missing += usize::from(!found);
        }
        self.effects += usize::from(map.effects.is_some());
        self.doorways += map.doorways.len();
        for block in &map.blocks {
            self.blocks += 1;
            let m = &block.manifest;
            if !block.processed {
                self.unbuilt_blocks += 1;
                continue;
            }
            assert_eq!(m.resources.len(), m.resource_capacity, "{}", block.member);
            assert_eq!(m.placements.len(), m.placement_capacity, "{}", block.member);
            self.resources += m.resources.len();
            self.texture_links += m.textures.len();
            self.placements += m.placements.len();
            for (part, r) in block.parts.iter().zip(&m.resources) {
                // Every resource resolves to members of its archive.
                assert!(part.file.is_some(), "{}: {:?}", block.member, part.name);
                match part.kind {
                    PartKind::Model => self.models += 1,
                    PartKind::Object => self.objects += 1,
                    PartKind::Collision => self.collisions += 1,
                    PartKind::None => panic!("{}: {:?} unresolved", block.member, part.name),
                }
                self.animations += part.animations.len();
                if part.kind != PartKind::Collision {
                    let mask = if part.kind == PartKind::Object {
                        0x1f
                    } else {
                        0xf
                    };
                    let flagged = (r.flags & mask).count_ones() as usize;
                    self.flagged_but_missing += flagged - part.animations.len();
                }
                self.texture_sources += usize::from(part.texture_source.is_some());
            }
            for inst in &block.instances {
                assert!(
                    inst.part.is_some(),
                    "{}: instance {}",
                    block.member,
                    inst.id
                );
                self.with_parent += usize::from(inst.parent.is_some());
                self.hidden_at_0 += usize::from(inst.hidden);
            }
            // Instances the hierarchy walk does not reach keep the default transform.
            self.unreached += block
                .instances
                .iter()
                .enumerate()
                .filter(|(i, inst)| {
                    *i != 0 && inst.parent.is_none() && m.placements[*i].parent >= 0
                })
                .count();
        }
    }
}

/// Every shipped map assembles (`dq9_formats::map`), every resource of a built block resolves
/// to members of its archive and every placement to a part; the SDK sine table is the one the
/// transforms compute.
#[test]
fn maps_assemble() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    // g_fixSinCosTable (0x020e9460): 4096 pairs of fx16 sine, cosine.
    let arm9 = rom.arm9().unwrap();
    let table_at = (0x020e_9460 - arm9.load_addr) as usize;
    for i in 0..4096 {
        let at = table_at + 4 * i;
        let b = &arm9.data[at..at + 4];
        let entry = (
            i16::from_le_bytes([b[0], b[1]]),
            i16::from_le_bytes([b[2], b[3]]),
        );
        assert_eq!(map::sin_cos_entry(i), entry, "sine table entry {i}");
    }

    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let file = |p: &str| map::rom_file(&rom, p);
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    // Records sharing their archives (grotto floors, ...) are counted once.
    let mut seen = HashSet::new();
    let (mut records, mut with_ambl) = (0, 0);
    let mut totals = MapTotals::default();
    let mut add = |map: Map| {
        if seen.insert((map.files.ambl.clone(), map.files.amdj.clone())) {
            totals.add(&map);
        }
    };
    for r in list.records.iter().filter(|r| r.id != 0) {
        let map = Map::assemble(r, &table, &list, file, &ctx)
            .unwrap_or_else(|e| panic!("map {} {:?}: {e}", r.id, r.code));
        records += 1;
        with_ambl += usize::from(map.ambl_found);
        add(map);
    }
    // The other grotto environments: a floor and a boss room each.
    for grotto_environ in 2..=5 {
        for id in [40001, 41101] {
            let ctx = Context {
                time_of_day: 0,
                grotto_environ,
            };
            add(Map::assemble(list.find(id).unwrap(), &table, &list, file, &ctx).unwrap());
        }
    }
    // 75 records name archives that do not ship (dummies, debug maps); `S09M03` and `M03M05`
    // only load through the case-insensitive file system.
    assert_eq!((records, with_ambl), (871, 796));
    assert_eq!(
        totals,
        MapTotals {
            // 667 `.ambl` (all but the 14 `ats_*`), 669 `.amdj`.
            maps: 742,
            without_ambl: 75,
            lighting: 663,
            lighting_missing: 2,
            effects: 15,
            doorways: 1402,
            blocks: 754,
            unbuilt_blocks: 0,
            resources: 5341,
            texture_links: 529,
            placements: 5651,
            models: 4096,
            objects: 112,
            collisions: 1133,
            animations: 2125,
            flagged_but_missing: 36,
            texture_sources: 845,
            with_parent: 1288,
            hidden_at_0: 713,
            unreached: 0,
        }
    );
}

/// Every `.chr` archive reads by role: its model, its motion table and each motion's
/// animation files; monster archives (`.cchr` of `enemy.gp2`) too.
#[test]
fn character_archives_read_by_role() {
    use dq9_formats::chr::{self, Chr, Role};
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut archives, mut models, mut tables, mut motions, mut with_files) = (0, 0, 0, 0, 0);
    let (mut orphans, mut long_names, mut npc_motions, mut npc_without_files) = (0, 0, 0, 0);
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".chr")) {
        let chr =
            Chr::parse(rom.file(file.id).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", file.path));
        archives += 1;
        models += usize::from(chr.model.is_some());
        tables += usize::from(chr.motion_table.is_some());
        let found = chr.motions();
        motions += found.len();
        with_files += found.iter().filter(|m| !m.files.is_empty()).count();
        long_names += found.iter().filter(|m| m.motion.name.len() > 15).count();
        // NPC models (`chara_sub`): animation members no motion of the table names.
        if file.path.starts_with("data/chara_sub/") {
            npc_motions += found.len();
            npc_without_files += found.iter().filter(|m| m.files.is_empty()).count();
            for i in 0..chr.narc.len() as u16 {
                if let Some(Role::Animation { motion, .. }) = chr.narc.name(i).map(chr::role)
                    && chr.motion_index(motion.as_bytes()).is_none()
                {
                    orphans += 1;
                }
            }
        }
    }
    println!(
        "{archives} archives, {models} models, {tables} motion tables, {motions} motions \
         ({with_files} with files), chara_sub: {npc_motions} motions, {npc_without_files} without \
         files, {orphans} orphan animations; {long_names} long names"
    );
    let enemy = Gpc2::parse(rom.file_by_path("data/pack_lv5/enemy.gp2").unwrap()).unwrap();
    let mut monsters = 0;
    for entry in enemy.entries.iter().filter(|e| e.name.ends_with(".mon")) {
        let mon = enemy.member(entry).unwrap();
        let narc = nds_rom::Narc::parse(&mon).unwrap();
        for i in 0..narc.len() as u16 {
            if narc.name(i).is_some_and(|n| n.contains(".cchr")) {
                let data = nds_rom::compress::decompress(narc.file(i).unwrap()).unwrap();
                let chr = Chr::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", entry.name));
                assert!(
                    chr.model.is_some() && chr.motion_table.is_some(),
                    "{}",
                    entry.name
                );
                monsters += 1;
            }
        }
    }
    // 1 911 `.nsbmd` and 2 512 `.bcfg` members: one archive holds two of each.
    assert_eq!(
        (archives, models, tables, motions, with_files),
        (2683, 1910, 2511, 6466, 3782)
    );
    // NPC models: the `step_*` motions name stretches of `walk` and have no file.
    assert_eq!((npc_motions, npc_without_files, orphans), (1055, 626, 7));
    // Names fit the game's 16-byte copy.
    assert_eq!(long_names, 0);
    assert_eq!(monsters, 601);
}

/// The cast of every map with a `.npc` archive, at the start of every chapter by day and by
/// night: every NPC is drawn with a sheet or archive that exists and reads.
#[test]
fn casts_resolve_to_existing_archives() {
    use dq9_formats::cast::{self, Appearance, Story};
    use dq9_formats::chr::Chr;
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let enemy = Gpc2::parse(rom.file_by_path("data/pack_lv5/enemy.gp2").unwrap()).unwrap();
    let exists = |p: &str| map::rom_file(&rom, p).is_some();
    let (mut maps, mut casts, mut npcs, mut unset) = (0, 0, 0, 0);
    let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
    let mut archives = HashSet::new();
    for record in &list.records {
        let Some(archive) = map::rom_file(&rom, &cast::archive_path(record.code)) else {
            continue;
        };
        maps += 1;
        for chapter in 1..=19 {
            for daytime in [true, false] {
                let story = Story {
                    chapter,
                    step: 1,
                    substep: 1,
                    daytime,
                    ..Story::default()
                };
                let c = cast::load(archive, record.id, &story).unwrap();
                casts += 1;
                unset += usize::from(c.read_unset);
                for npc in &c.npcs {
                    npcs += 1;
                    let a = npc.appearance(exists);
                    let (kind, path) = match &a {
                        Appearance::None => ("none", None),
                        Appearance::Sprite { sheet } => ("sprite", Some(sheet.clone())),
                        Appearance::Model { chr } => ("model", Some(chr.clone())),
                        Appearance::Monster { member } => ("monster", Some(member.clone())),
                    };
                    *kinds.entry(kind).or_default() += 1;
                    let Some(path) = path else { continue };
                    let ok = match a {
                        Appearance::Monster { member } => enemy.find(&member).is_some(),
                        Appearance::Model { chr } => map::rom_file(&rom, &chr)
                            .is_some_and(|d| Chr::parse(d).is_ok_and(|c| c.model.is_some())),
                        _ => exists(&path),
                    };
                    assert!(
                        ok,
                        "{}: NPC {} drawn with missing {path}",
                        record.id, npc.member.id
                    );
                    archives.insert(path);
                    if let Some(extra) = npc.placement.chr {
                        let extra = format!("data/{}", String::from_utf8_lossy(extra));
                        assert!(exists(&extra), "{extra}");
                    }
                }
            }
        }
    }
    let kinds: Vec<_> = kinds.into_iter().collect();
    assert_eq!(
        (maps, casts, npcs, archives.len()),
        (508, 19304, 36170, 238)
    );
    assert_eq!(
        kinds,
        [
            ("model", 2511),
            ("monster", 370),
            ("none", 12122),
            ("sprite", 21167)
        ]
    );
    // No record reads a field before a record of its file has set it.
    assert_eq!(unset, 0);

    // Stornway church (C01M09) as the game built it in `build/melonds/demo.ml1`: story
    // 19.2.1 at night, quest 4 done; same nodes, positions and record counters as in RAM.
    let story = Story {
        chapter: 19,
        step: 2,
        substep: 1,
        quests: [(4, 3)].into(),
        ..Story::default()
    };
    let archive = map::rom_file(&rom, "data/scenario/C01.npc").unwrap();
    let c = cast::load(archive, 109, &story).unwrap();
    let got: Vec<_> = c
        .npcs
        .iter()
        .map(|n| {
            let p = &n.placement;
            (n.member.id, p.position, p.angle, p.order, p.quest)
        })
        .collect();
    assert_eq!(
        got,
        [
            (25, [696, 1433, -14499], 778, 0x3d, -1),
            (10, [-2703, 409, 12369], 12861, 0x21, 4)
        ]
    );
}

/// Every sprite sheet but two reads to its end; every step shows an existing frame.
#[test]
fn sprite_sheets_read() {
    use dq9_formats::spr::Sprite;
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut sheets, mut frames, mut with_stand) = (0, 0, 0);
    let (mut bad_frames, mut bad_next, mut failed) = (0, 0, Vec::new());
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".spr")) {
        sheets += 1;
        let Ok(s) = Sprite::parse(rom.file(file.id).unwrap()) else {
            failed.push(file.path.as_str());
            continue;
        };
        frames += s.frames.len();
        with_stand += usize::from(s.find(b"stand_down") != 0);
        for a in &s.animations {
            bad_frames += a
                .steps
                .iter()
                .filter(|st| st.frame as usize >= s.frames.len())
                .count();
            bad_next += a
                .steps
                .iter()
                .filter(|st| st.next as usize >= a.steps.len())
                .count();
        }
    }
    // The two that do not read are a menu frame and a menu sheet, not loose NPC sheets.
    assert_eq!(failed, ["data/ani/waku.spr", "data/menu/dqa_rox.spr"]);
    assert_eq!((sheets, frames, with_stand), (1300, 4262, 193));
    // Single-step static sheets (`spr`) name step 1 next, past their one step.
    assert_eq!((bad_frames, bad_next), (0, 1960));
}
