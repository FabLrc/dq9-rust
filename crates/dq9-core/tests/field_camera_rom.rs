//! What a map change gives the field camera (`ov_017::0219e384`), against the ROM; skipped when
//! the ROM is absent. `docs/re/field-camera.md`.

use dq9_core::camera::{FRAMINGS, framing, home_yaw};
use dq9_formats::Table;
use dq9_formats::map::ZoneInfo;
use dq9_formats::maplist::{self, MapList};
use nds_rom::Rom;

/// The four framing entries are the words the game's setters load: the literals after
/// `func_020a27dc`, `0x020a2840` and `0x020a2878`, and the immediates of the others.
#[test]
fn the_framings_are_the_games_constants() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let arm9 = rom.arm9().unwrap();
    let word = |address: u32| {
        let at = (address - arm9.load_addr) as usize;
        u32::from_le_bytes(arm9.data[at..at + 4].try_into().unwrap()) as i32
    };
    // Entry 0 (`0x020a27dc`): literal `dy`, `mov r2, #0xe000`, `mov r1, #0xf000`.
    assert_eq!(word(0x020a_280c), FRAMINGS[0].dy);
    assert_eq!(word(0x020a_2800) as u32, 0xe3a0_1a0f);
    assert_eq!(word(0x020a_27ec) as u32, 0xe3a0_2a0e);
    assert_eq!(
        (FRAMINGS[0].distance, FRAMINGS[0].half_fov),
        (0xe000, 0xf000)
    );
    // Entry 1 (`0x020a2810`): `mov r1, #0x6000`, `mov r2, #0xb000`, `mov r1, #0xf000`.
    assert_eq!(word(0x020a_2818) as u32, 0xe3a0_1a06);
    assert_eq!(word(0x020a_2820) as u32, 0xe3a0_2a0b);
    assert_eq!(word(0x020a_2834) as u32, 0xe3a0_1a0f);
    assert_eq!((FRAMINGS[1].dy, FRAMINGS[1].distance), (0x6000, 0xb000));
    assert_eq!(FRAMINGS[1].half_fov, 0xf000);
    // Entry 2 (`0x020a2840`): two literals, `mov r1, #0x19000`.
    assert_eq!(word(0x020a_2870), FRAMINGS[2].dy);
    assert_eq!(word(0x020a_2874), FRAMINGS[2].distance);
    assert_eq!(word(0x020a_2864) as u32, 0xe3a0_1a19);
    assert_eq!(FRAMINGS[2].half_fov, 0x19000);
    // Entry 3 (`0x020a2878`): literal `dy`, distance `dy + 0x2000` (`add r2, r1, #0x2000`),
    // `mov r1, #0x19000`.
    assert_eq!(word(0x020a_28a8), FRAMINGS[3].dy);
    assert_eq!(word(0x020a_2888) as u32, 0xe281_2a02);
    assert_eq!(word(0x020a_289c) as u32, 0xe3a0_1a19);
    assert_eq!(FRAMINGS[3].distance, FRAMINGS[3].dy + 0x2000);
    assert_eq!(FRAMINGS[3].half_fov, 0x19000);
}

/// Over the whole map list: the framing entry of each record and the home yaw of its zone.
/// 438 maps use entry 0, 431 entry 1, `O00` entry 2, `O01` entry 3; every home yaw is 0 but
/// `M01`'s 1228.
#[test]
fn the_map_list_decides_the_framing_and_the_home_yaw() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let mut entries = [0usize; 4];
    let mut turned = Vec::new();
    for record in list
        .records
        .iter()
        .filter(|r| r.id != 0 && !r.code.is_empty())
    {
        let chosen = framing(record.byte_c);
        let index = FRAMINGS.iter().position(|f| *f == chosen).unwrap();
        entries[index] += 1;
        let mut zone = ZoneInfo::default();
        zone.load_map_list(&table, record.id).unwrap();
        if home_yaw(zone.world_angle) != 0 {
            turned.push((record.id, home_yaw(zone.world_angle)));
        }
        if matches!(record.id, 10000 | 10100) {
            assert_eq!(index, if record.id == 10000 { 2 } else { 3 });
        }
    }
    assert_eq!(entries, [438, 431, 1, 1]);
    assert_eq!(turned, [(1100, 1228)], "the exterior of Angel Falls only");
}
