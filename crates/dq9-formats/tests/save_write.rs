//! The write path of `dq9_formats::save`: every setter moves only its own bytes plus the
//! three checksums, round-trips through `Save::parse`, and leaves every other field of the
//! slot (every other reader accessor) equal.
//!
//! Synthetic image always; a real save when `DQ9_SAVE` names one (no save is shipped with the
//! repository), like `real_save.rs`.
//!
//! "Every other field equal" is proven on the bytes: a reader only reads the bytes of its
//! field, so a diff confined to the setter's range cannot move another field — and the dump
//! below re-reads every accessor to check it.

use core::ops::Range;
use std::collections::BTreeMap;

use dq9_formats::save::{
    self, Block, CHARACTER_COUNT, EquipSlot, EventFlags, FILE_SIZE, MAGIC, SLOT_SIZE, Save, Slot,
    SlotMut, VOCATION_COUNT, Warp,
};

type Apply = Box<dyn Fn(&mut SlotMut<'_>) -> Option<Range<usize>>>;
type Read = Box<dyn for<'s> Fn(&Slot<'s>) -> String>;

struct Op {
    name: &'static str,
    /// Dump keys this setter is allowed to change.
    keys: &'static [&'static str],
    apply: Apply,
    read: Read,
    expected: &'static str,
    /// Whether the base image is the synthetic one: only then are the exact bytes and the
    /// readback known (a real save already holds its own values).
    synthetic: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A sealed image: magic, slot 0 present, all fields zero. `intermission` marks a suspend save.
fn image(intermission: bool) -> Vec<u8> {
    let mut data = vec![0u8; FILE_SIZE];
    data[..MAGIC.len()].copy_from_slice(MAGIC);
    let mut save = Save::parse(&data).unwrap();
    {
        let mut slot = save.slot_mut(0).unwrap();
        slot.set_exists(true);
        slot.set_intermission(intermission);
        slot.seal();
    }
    save.as_bytes().to_vec()
}

fn real_save() -> Option<Vec<u8>> {
    let path = std::env::var_os("DQ9_SAVE")?;
    Some(std::fs::read(path).expect("DQ9_SAVE is readable"))
}

/// Every reader accessor of one slot, keyed as the `keys` of an [`Op`].
fn dump(slot: &Slot<'_>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut put = |key: String, value: String| {
        m.insert(key, value);
    };
    put("status".into(), format!("{:?}", slot.status()));
    put("exists".into(), slot.exists().to_string());
    put("intermission".into(), slot.is_intermission().to_string());
    put("chapter".into(), slot.chapter().to_string());
    put("scenario_step".into(), slot.scenario_step().to_string());
    put(
        "scenario_substep".into(),
        slot.scenario_substep().to_string(),
    );
    put("day_timer".into(), slot.day_timer().to_string());
    let (h, min, s) = slot.play_time();
    put("play_time".into(), format!("{h}:{min}:{s}"));
    put("area".into(), slot.area().to_string());
    put("warp".into(), format!("{:?}", slot.warp()));
    put("standby_count".into(), slot.standby_count().to_string());
    put("party_count".into(), slot.party_count().to_string());
    put("party_order".into(), format!("{:?}", slot.party_order()));
    for member in 0..4 {
        put(
            format!("held_items.{member}"),
            format!("{:?}", slot.held_items(member)),
        );
    }
    put("gold_on_hand".into(), slot.gold_on_hand().to_string());
    put("gold_in_bank".into(), slot.gold_in_bank().to_string());
    put("mini_medals".into(), slot.mini_medals().to_string());
    put(
        "visited_locations".into(),
        slot.visited_locations().to_string(),
    );
    put("special_guests".into(), slot.special_guests().to_string());
    put("party_tricks".into(), slot.party_tricks().to_string());
    put(
        "unlocked_vocations".into(),
        slot.unlocked_vocations().to_string(),
    );
    let ev = slot.event_flags();
    put("current_track".into(), ev.current_track().to_string());
    put("global_flags".into(), hex(ev.global_flags()));
    put("quests".into(), hex(&ev.as_bytes()[0x2cc..0x332]));
    for index in 0..5 {
        let track = ev.track(index).unwrap();
        for (field, value) in [
            ("chapter", track.chapter().to_string()),
            ("step", track.step().to_string()),
            ("substep", track.substep().to_string()),
            ("chapter_flags", hex(track.chapter_flags())),
            ("step_flags", hex(track.step_flags())),
        ] {
            put(format!("event.track.{index}.{field}"), value);
        }
    }
    for index in 0..CHARACTER_COUNT {
        let c = slot.character(index).unwrap();
        let key = |field: &str| format!("character.{index}.{field}");
        put(key("name_raw"), hex(c.name_raw()));
        put(key("vocation"), c.vocation().to_string());
        for v in 0..VOCATION_COUNT {
            put(key(&format!("level.{v}")), c.level(v).unwrap().to_string());
            put(
                key(&format!("revocations.{v}")),
                c.revocations(v).unwrap().to_string(),
            );
            put(
                key(&format!("experience.{v}")),
                c.experience(v).unwrap().to_string(),
            );
            for s in 0..save::SEEDS_PER_VOCATION {
                put(
                    key(&format!("seed.{v}.{s}")),
                    c.seed(v, s).unwrap().to_string(),
                );
            }
        }
        put(
            key("skill_points"),
            c.unallocated_skill_points().to_string(),
        );
        put(key("proficiency_flags"), hex(c.proficiency_flags()));
        put(key("face"), c.face().to_string());
        put(key("hair_style"), c.hair_style().to_string());
        put(key("face_item"), c.face_item().to_string());
        put(key("hair_item"), c.hair_item().to_string());
        put(key("gender"), c.gender().to_string());
        put(key("skin_color"), c.skin_color().to_string());
        put(key("eye_color"), c.eye_color().to_string());
        put(key("hair_color"), c.hair_color().to_string());
        put(key("body"), format!("{:?}", c.body()));
        for slot in EquipSlot::ALL {
            put(
                key(&format!("equipment.{slot:?}")),
                c.equipment(slot).to_string(),
            );
        }
    }
    m
}

/// Bytes the three checksums occupy inside one slot.
fn is_checksum_byte(at: usize) -> bool {
    let rel = at % SLOT_SIZE;
    [16..20, 36..40, 132..136].iter().any(|r| r.contains(&rel))
}

fn check(base: &[u8], op: &Op) {
    let mut save = Save::parse(base).unwrap();
    let before = save.as_bytes().to_vec();
    let range = {
        let mut slot = save.slot_mut(0).unwrap();
        (op.apply)(&mut slot).unwrap_or_else(|| panic!("{}: out of range", op.name))
    };
    // The contract: a setter leaves the checksums stale, and the CLI seals before writing.
    save.slot_mut(0).unwrap().seal();
    let after = save.as_bytes().to_vec();

    for (at, (&a, &b)) in before.iter().zip(&after).enumerate() {
        if a != b {
            assert!(
                is_checksum_byte(at) || range.contains(&at),
                "{}: byte {at} differs, outside {range:?}",
                op.name
            );
        }
    }
    if op.synthetic {
        assert_ne!(before, after, "{}: changed nothing", op.name);
    }
    assert_eq!(
        &before[SLOT_SIZE..],
        &after[SLOT_SIZE..],
        "{}: slot 1 changed",
        op.name
    );

    let reread = Save::parse(&after).unwrap();
    let slot = reread.slot(0).unwrap();
    assert!(
        slot.checksums_valid(),
        "{}: {:?}",
        op.name,
        slot.checksums()
    );
    if op.synthetic {
        assert_eq!((op.read)(&slot), op.expected, "{}: readback", op.name);
    }

    let before_dump = dump(&Save::parse(&before).unwrap().slot(0).unwrap());
    let after_dump = dump(&slot);
    let mut changed: Vec<&str> = after_dump
        .iter()
        .filter(|(key, value)| before_dump.get(*key) != Some(value))
        .map(|(key, _)| key.as_str())
        .collect();
    // Both sides sorted: the dump iterates in key order, a `keys` list is written by hand.
    changed.sort_unstable();
    let mut expected = op.keys.to_vec();
    expected.sort_unstable();
    assert!(
        changed.iter().all(|key| expected.contains(key)),
        "{}: other fields moved: {changed:?}",
        op.name
    );
    if op.synthetic {
        assert!(!changed.is_empty(), "{}: no field changed", op.name);
    }
}

fn ops() -> Vec<Op> {
    vec![
        Op {
            name: "day_timer",
            keys: &["day_timer"],
            apply: Box::new(|s| Some(s.set_day_timer(42.5))),
            read: Box::new(|s| s.day_timer().to_string()),
            expected: "42.5",
            synthetic: true,
        },
        Op {
            name: "play_time",
            keys: &["play_time"],
            apply: Box::new(|s| Some(s.set_play_time(98, 32, 44))),
            read: Box::new(|s| {
                let (h, m, sec) = s.play_time();
                format!("{h}:{m}:{sec}")
            }),
            expected: "98:32:44",
            synthetic: true,
        },
        Op {
            name: "area",
            keys: &["area"],
            apply: Box::new(|s| Some(s.set_area(109))),
            read: Box::new(|s| s.area().to_string()),
            expected: "109",
            synthetic: true,
        },
        Op {
            name: "exists",
            keys: &["exists", "status"],
            apply: Box::new(|s| Some(s.set_exists(false))),
            read: Box::new(|s| s.exists().to_string()),
            expected: "false",
            synthetic: true,
        },
        Op {
            name: "intermission",
            keys: &["intermission", "status", "warp", "area"],
            apply: Box::new(|s| Some(s.set_intermission(true))),
            read: Box::new(|s| s.is_intermission().to_string()),
            expected: "true",
            synthetic: true,
        },
        Op {
            name: "chapter",
            keys: &["chapter"],
            apply: Box::new(|s| Some(s.set_chapter(19))),
            read: Box::new(|s| s.chapter().to_string()),
            expected: "19",
            synthetic: true,
        },
        Op {
            name: "scenario_step",
            keys: &["scenario_step"],
            apply: Box::new(|s| Some(s.set_scenario_step(2))),
            read: Box::new(|s| s.scenario_step().to_string()),
            expected: "2",
            synthetic: true,
        },
        Op {
            name: "scenario_substep",
            keys: &["scenario_substep"],
            apply: Box::new(|s| Some(s.set_scenario_substep(1))),
            read: Box::new(|s| s.scenario_substep().to_string()),
            expected: "1",
            synthetic: true,
        },
        Op {
            name: "standby_count",
            keys: &["standby_count"],
            apply: Box::new(|s| Some(s.set_standby_count(9))),
            read: Box::new(|s| s.standby_count().to_string()),
            expected: "9",
            synthetic: true,
        },
        Op {
            name: "party_count",
            keys: &["party_count", "party_order"],
            apply: Box::new(|s| Some(s.set_party_count(3))),
            read: Box::new(|s| s.party_count().to_string()),
            expected: "3",
            synthetic: true,
        },
        Op {
            name: "party_order",
            keys: &["party_count", "party_order"],
            apply: Box::new(|s| {
                let order = s.set_party_order(&[3, 2, 1, 0]);
                let count = s.set_party_count(4);
                Some(order.start..count.end)
            }),
            read: Box::new(|s| format!("{:?}", s.party_order())),
            expected: "[3, 2, 1, 0]",
            synthetic: true,
        },
        Op {
            name: "held_items",
            keys: &["held_items.2"],
            apply: Box::new(|s| s.set_held_items(2, &[1, 2, 3, 4, 5, 6, 7, 8])),
            read: Box::new(|s| format!("{:?}", s.held_items(2))),
            expected: "Some([1, 2, 3, 4, 5, 6, 7, 8])",
            synthetic: true,
        },
        Op {
            name: "held_item",
            keys: &["held_items.1"],
            apply: Box::new(|s| s.set_held_item(1, 3, 900)),
            read: Box::new(|s| format!("{:?}", s.held_items(1))),
            expected: "Some([0, 0, 0, 900, 0, 0, 0, 0])",
            synthetic: true,
        },
        Op {
            name: "gold_on_hand",
            keys: &["gold_on_hand"],
            apply: Box::new(|s| Some(s.set_gold_on_hand(1234))),
            read: Box::new(|s| s.gold_on_hand().to_string()),
            expected: "1234",
            synthetic: true,
        },
        Op {
            name: "gold_in_bank",
            keys: &["gold_in_bank"],
            apply: Box::new(|s| Some(s.set_gold_in_bank(99_999))),
            read: Box::new(|s| s.gold_in_bank().to_string()),
            expected: "99999",
            synthetic: true,
        },
        Op {
            name: "mini_medals",
            keys: &["mini_medals"],
            apply: Box::new(|s| Some(s.set_mini_medals(12))),
            read: Box::new(|s| s.mini_medals().to_string()),
            expected: "12",
            synthetic: true,
        },
        Op {
            name: "visited_locations",
            keys: &["global_flags", "visited_locations"],
            apply: Box::new(|s| Some(s.set_visited_locations(0xdead_beef))),
            read: Box::new(|s| s.visited_locations().to_string()),
            expected: "3735928559",
            synthetic: true,
        },
        Op {
            name: "special_guests",
            keys: &["special_guests"],
            apply: Box::new(|s| Some(s.set_special_guests(0b1010))),
            read: Box::new(|s| s.special_guests().to_string()),
            expected: "10",
            synthetic: true,
        },
        Op {
            name: "party_tricks",
            keys: &["global_flags", "party_tricks"],
            apply: Box::new(|s| Some(s.set_party_tricks(0b110))),
            read: Box::new(|s| s.party_tricks().to_string()),
            expected: "6",
            synthetic: true,
        },
        Op {
            name: "unlocked_vocations",
            keys: &["global_flags", "unlocked_vocations"],
            apply: Box::new(|s| Some(s.set_unlocked_vocations(0x0fc0))),
            read: Box::new(|s| s.unlocked_vocations().to_string()),
            expected: "4032",
            synthetic: true,
        },
        Op {
            name: "current_track",
            keys: &["current_track"],
            apply: Box::new(|s| Some(s.event_flags_mut().set_current_track(3))),
            read: Box::new(|s| s.event_flags().current_track().to_string()),
            expected: "3",
            synthetic: true,
        },
        Op {
            name: "track.chapter",
            keys: &["event.track.2.chapter"],
            apply: Box::new(|s| {
                s.event_flags_mut()
                    .track_mut(2)
                    .map(|mut t| t.set_chapter(19))
            }),
            read: Box::new(|s| s.event_flags().track(2).unwrap().chapter().to_string()),
            expected: "19",
            synthetic: true,
        },
        Op {
            name: "track.step",
            keys: &["event.track.2.step"],
            apply: Box::new(|s| s.event_flags_mut().track_mut(2).map(|mut t| t.set_step(7))),
            read: Box::new(|s| s.event_flags().track(2).unwrap().step().to_string()),
            expected: "7",
            synthetic: true,
        },
        Op {
            name: "track.substep",
            keys: &["event.track.2.substep"],
            apply: Box::new(|s| {
                s.event_flags_mut()
                    .track_mut(2)
                    .map(|mut t| t.set_substep(5))
            }),
            read: Box::new(|s| s.event_flags().track(2).unwrap().substep().to_string()),
            expected: "5",
            synthetic: true,
        },
        Op {
            name: "track.chapter_flags",
            keys: &["event.track.2.chapter_flags"],
            apply: Box::new(|s| {
                s.event_flags_mut()
                    .track_mut(2)
                    .map(|mut t| t.set_chapter_flags(&[1, 2, 3]))
            }),
            read: Box::new(|s| hex(s.event_flags().track(2).unwrap().chapter_flags())),
            expected: "01020300000000000000000000",
            synthetic: true,
        },
        Op {
            name: "track.step_flags",
            keys: &["event.track.2.step_flags"],
            apply: Box::new(|s| {
                s.event_flags_mut()
                    .track_mut(2)
                    .map(|mut t| t.set_step_flags(&[4, 5]))
            }),
            read: Box::new(|s| hex(s.event_flags().track(2).unwrap().step_flags())),
            expected: "040500000000000000000000",
            synthetic: true,
        },
        Op {
            name: "global_flag",
            keys: &["global_flags"],
            apply: Box::new(|s| s.event_flags_mut().set_global_flag(9, true, false)),
            read: Box::new(|s| s.event_flags().global_flag(9).to_string()),
            expected: "true",
            synthetic: true,
        },
        Op {
            name: "story_flag",
            keys: &["global_flags"],
            apply: Box::new(|s| s.event_flags_mut().set_global_flag(0x400, true, true)),
            read: Box::new(|s| {
                let ev = s.event_flags();
                // 0x400 is past the plain array only for `story_flag`.
                (ev.story_flag(0x400) && ev.global_flag(0x400 + 0x6fa)).to_string()
            }),
            expected: "true",
            synthetic: true,
        },
        Op {
            name: "character.name",
            keys: &["character.3.name_raw"],
            apply: Box::new(|s| {
                s.character_mut(3)
                    .map(|mut c| c.set_name_raw(&[0x12, 0x2f, 0x30, 0x39]))
            }),
            read: Box::new(|s| hex(s.character(3).unwrap().name_raw())),
            expected: "122f3039000000000000",
            synthetic: true,
        },
        Op {
            name: "character.vocation",
            keys: &["character.3.vocation"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_vocation(4))),
            read: Box::new(|s| s.character(3).unwrap().vocation().to_string()),
            expected: "4",
            synthetic: true,
        },
        Op {
            name: "character.level",
            keys: &["character.3.level.5"],
            apply: Box::new(|s| s.character_mut(3).and_then(|mut c| c.set_level(5, 42))),
            read: Box::new(|s| s.character(3).unwrap().level(5).unwrap().to_string()),
            expected: "42",
            synthetic: true,
        },
        Op {
            name: "character.revocations",
            keys: &["character.3.revocations.5"],
            apply: Box::new(|s| s.character_mut(3).and_then(|mut c| c.set_revocations(5, 2))),
            read: Box::new(|s| s.character(3).unwrap().revocations(5).unwrap().to_string()),
            expected: "2",
            synthetic: true,
        },
        Op {
            name: "character.experience",
            keys: &["character.3.experience.5"],
            apply: Box::new(|s| {
                s.character_mut(3)
                    .and_then(|mut c| c.set_experience(5, 777_777))
            }),
            read: Box::new(|s| s.character(3).unwrap().experience(5).unwrap().to_string()),
            expected: "777777",
            synthetic: true,
        },
        Op {
            name: "character.seed",
            keys: &["character.3.seed.5.4"],
            apply: Box::new(|s| s.character_mut(3).and_then(|mut c| c.set_seed(5, 4, 1000))),
            read: Box::new(|s| {
                let c = s.character(3).unwrap();
                format!("{:?}", [c.seed(5, 3), c.seed(5, 4), c.seed(5, 5)])
            }),
            expected: "[Some(0), Some(1000), Some(0)]",
            synthetic: true,
        },
        Op {
            name: "character.skill_points",
            keys: &["character.3.skill_points"],
            apply: Box::new(|s| {
                s.character_mut(3)
                    .map(|mut c| c.set_unallocated_skill_points(37))
            }),
            read: Box::new(|s| {
                s.character(3)
                    .unwrap()
                    .unallocated_skill_points()
                    .to_string()
            }),
            expected: "37",
            synthetic: true,
        },
        Op {
            name: "character.proficiency_flags",
            keys: &["character.3.proficiency_flags"],
            apply: Box::new(|s| {
                s.character_mut(3)
                    .map(|mut c| c.set_proficiency_flags(&[1, 2, 3, 4]))
            }),
            read: Box::new(|s| hex(s.character(3).unwrap().proficiency_flags())),
            expected: "010203040000000000000000000000000000000000000000000000000000000000000000",
            synthetic: true,
        },
        Op {
            name: "character.equipment",
            keys: &["character.3.equipment.Head"],
            apply: Box::new(|s| {
                s.character_mut(3)
                    .map(|mut c| c.set_equipment(EquipSlot::Head, 9101))
            }),
            read: Box::new(|s| {
                s.character(3)
                    .unwrap()
                    .equipment(EquipSlot::Head)
                    .to_string()
            }),
            expected: "9101",
            synthetic: true,
        },
        Op {
            name: "character.face",
            keys: &["character.3.face", "character.3.face_item"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_face_item(9020))),
            read: Box::new(|s| s.character(3).unwrap().face_item().to_string()),
            expected: "9020",
            synthetic: true,
        },
        Op {
            name: "character.hair_style",
            keys: &["character.3.hair_style", "character.3.hair_item"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_hair_item(9001))),
            read: Box::new(|s| s.character(3).unwrap().hair_item().to_string()),
            expected: "9001",
            synthetic: true,
        },
        Op {
            name: "character.gender",
            keys: &["character.3.gender"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_gender(1))),
            read: Box::new(|s| s.character(3).unwrap().gender().to_string()),
            expected: "1",
            synthetic: true,
        },
        Op {
            name: "character.skin_color",
            keys: &["character.3.skin_color"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_skin_color(5))),
            read: Box::new(|s| s.character(3).unwrap().skin_color().to_string()),
            expected: "5",
            synthetic: true,
        },
        Op {
            name: "character.eye_color",
            keys: &["character.3.eye_color"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_eye_color(12))),
            read: Box::new(|s| s.character(3).unwrap().eye_color().to_string()),
            expected: "12",
            synthetic: true,
        },
        Op {
            name: "character.hair_color",
            keys: &["character.3.hair_color"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_hair_color(3))),
            read: Box::new(|s| s.character(3).unwrap().hair_color().to_string()),
            expected: "3",
            synthetic: true,
        },
        Op {
            name: "character.body",
            keys: &["character.3.body"],
            apply: Box::new(|s| s.character_mut(3).map(|mut c| c.set_body(11, 22))),
            read: Box::new(|s| format!("{:?}", s.character(3).unwrap().body())),
            expected: "(11, 22)",
            synthetic: true,
        },
    ]
}

/// Ops that only make sense on a suspend (intermission) save.
fn suspend_ops() -> Vec<Op> {
    vec![
        Op {
            name: "warp (suspend)",
            keys: &["warp", "area"],
            apply: Box::new(|s| {
                Some(s.set_warp(Warp {
                    map: 1234,
                    angle: -123,
                    position: [1, 2, 3],
                }))
            }),
            read: Box::new(|s| format!("{:?}", s.warp())),
            expected: "Some(Warp { map: 1234, angle: -123, position: [1, 2, 3] })",
            synthetic: true,
        },
        Op {
            name: "area (suspend)",
            keys: &["area", "warp"],
            apply: Box::new(|s| Some(s.set_area(20002))),
            read: Box::new(|s| s.area().to_string()),
            expected: "20002",
            synthetic: true,
        },
    ]
}

#[test]
fn every_setter_is_confined_and_round_trips() {
    let normal = image(false);
    for op in ops() {
        check(&normal, &op);
    }
    let suspend = image(true);
    for op in suspend_ops() {
        check(&suspend, &op);
    }
}

#[test]
fn a_write_without_a_seal_reports_a_bad_checksum() {
    let mut save = Save::parse(&image(false)).unwrap();
    save.slot_mut(0).unwrap().set_gold_on_hand(1234);
    let bad: Vec<_> = save
        .slot(0)
        .unwrap()
        .checksums()
        .into_iter()
        .filter(|c| !c.is_valid())
        .map(|c| c.which)
        .collect();
    assert_eq!(bad, [save::Checksum::B], "gold lies in the B range only");
    save.slot_mut(0).unwrap().seal();
    assert!(save.slot(0).unwrap().checksums_valid());
}

#[test]
fn seal_only_is_byte_identical() {
    for intermission in [false, true] {
        let bytes = image(intermission);
        let mut save = Save::parse(&bytes).unwrap();
        save.slot_mut(0).unwrap().seal();
        assert_eq!(save.as_bytes(), &bytes[..]);
    }
}

#[test]
fn mirror_reproduces_the_write_protocols_second_copy() {
    let mut save = Save::parse(&image(false)).unwrap();
    save.slot_mut(0).unwrap().set_gold_on_hand(1234);
    save.slot_mut(0).unwrap().seal();
    let header = save.as_bytes()[..16].to_vec();
    assert!(save.mirror_normal_slot());
    let bytes = save.as_bytes();
    assert_eq!(&bytes[..16], &header[..], "the file's magic is not touched");
    assert_eq!(&bytes[16..16 + 0x6fe4], &bytes[SLOT_SIZE + 16..][..0x6fe4]);
    assert_eq!(
        bytes[SLOT_SIZE + 16 + 0x6fe4..],
        vec![0; SLOT_SIZE - 16 - 0x6fe4]
    );
}

#[test]
fn mirror_skips_a_suspend_save() {
    let suspend = image(true);
    let mut save = Save::parse(&suspend).unwrap();
    assert!(!save.mirror_normal_slot());
    assert_eq!(save.as_bytes(), &suspend[..]);
}

#[test]
fn indices_are_bounds_checked_on_the_write_side() {
    let mut save = Save::parse(&image(false)).unwrap();
    let mut slot = save.slot_mut(0).unwrap();
    assert!(slot.character_mut(CHARACTER_COUNT).is_none());
    assert!(slot.set_held_item(4, 0, 1).is_none());
    assert!(slot.set_held_items(4, &[0; 8]).is_none());
    {
        let mut c = slot.character_mut(0).unwrap();
        assert!(c.set_level(VOCATION_COUNT, 1).is_none());
        assert!(c.set_revocations(VOCATION_COUNT, 1).is_none());
        assert!(c.set_experience(VOCATION_COUNT, 1).is_none());
        assert!(c.set_seed(VOCATION_COUNT, 0, 1).is_none());
        assert!(c.set_seed(0, save::SEEDS_PER_VOCATION, 1).is_none());
        assert_eq!(c.set_face(1).len(), 1);
        assert_eq!(c.set_hair_style(2).len(), 1);
    }
    let mut ev = slot.event_flags_mut();
    assert!(ev.track_mut(5).is_none());
    assert!(ev.set_global_flag(-1, true, false).is_none());
    assert!(ev.set_global_flag(0x234 * 8, true, false).is_none());
}

#[test]
fn blocks_are_reachable_for_writing_through_the_typed_views() {
    // The event block the mutable view wraps is the same one `Slot::event_flags` reads.
    let mut save = Save::parse(&image(false)).unwrap();
    save.slot_mut(0)
        .unwrap()
        .event_flags_mut()
        .set_global_flag(1, true, false)
        .unwrap();
    let raw = Save::parse(save.as_bytes()).unwrap();
    let ev = raw.slot(0).unwrap().event_flags();
    let block = raw.slot(0).unwrap().block(Block::Progress);
    assert_eq!(&block[16..16 + save::EVENT_FLAGS_SIZE], ev.as_bytes());
    assert!(ev.global_flag(1));
    assert!(
        EventFlags::from_bytes(ev.as_bytes())
            .unwrap()
            .global_flag(1)
    );
}

#[test]
fn every_setter_is_confined_on_a_real_save() {
    let Some(bytes) = real_save() else {
        eprintln!("DQ9_SAVE not set, skipping");
        return;
    };
    // The demo save is a normal save in slot 0: run the normal ops on it as it is, and the
    // warp ops on a suspend variant of it (the header byte is all that makes a suspend save).
    for mut op in ops() {
        op.synthetic = false;
        check(&bytes, &op);
    }
    let mut suspend = Save::parse(&bytes).unwrap();
    {
        let mut slot = suspend.slot_mut(0).unwrap();
        slot.set_intermission(true);
        slot.seal();
    }
    let suspend = suspend.as_bytes().to_vec();
    for mut op in suspend_ops() {
        op.synthetic = false;
        check(&suspend, &op);
    }
}

#[test]
fn a_real_saves_no_op_edit_is_byte_identical() {
    let Some(bytes) = real_save() else {
        eprintln!("DQ9_SAVE not set, skipping");
        return;
    };
    let mut save = Save::parse(&bytes).unwrap();
    for index in 0..save::SLOT_COUNT {
        save.slot_mut(index).unwrap().seal();
    }
    assert_eq!(save.as_bytes(), &bytes[..]);
    // The mirror rule holds on the demo: copying slot 0 into slot 1 changes no byte.
    assert!(save.mirror_normal_slot());
    assert_eq!(save.as_bytes(), &bytes[..]);
}
