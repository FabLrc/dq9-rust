use super::*;
use dq9_formats::map::{WarpVolume, ZoneVolume, ZoneWarp};

fn fx(v: f32) -> i32 {
    (v * 4096.0) as i32
}

fn fx3(v: [f32; 3]) -> Fx3 {
    v.map(fx)
}

/// A zone as the parser stores it: centre and full size in world units, box angle and facing
/// in radians.
fn zone(kind: u32, center: [f32; 3], size: [f32; 3], angle: f32, facing: f32) -> Zone {
    let size = fx3(size);
    Zone {
        kind,
        volume: ZoneVolume {
            center: fx3(center),
            size,
            angle: fx(angle) as i16,
            facing: fx(facing) as i16,
            radius_sq: square(size[0] / 2) + square(size[2] / 2),
        },
        class: 0,
        number: 0,
        flags: 0,
        sound: 0,
        warp: None,
    }
}

fn doorway(center: [f32; 3], size: [f32; 3], facing: f32, flags: u16, to: u16) -> Zone {
    Zone {
        class: 0,
        number: 9,
        flags,
        warp: Some(ZoneWarp {
            destination: to,
            arrival: fx3([1.0, 0.5, 2.0]),
            arrival_angle: fx(3.0) as i16,
        }),
        ..zone(ZONE_DOORWAY, center, size, 0.0, facing)
    }
}

fn area(number: u8, center: [f32; 3], size: [f32; 3]) -> Zone {
    Zone {
        class: number,
        ..zone(ZONE_AREA, center, size, 0.0, 0.0)
    }
}

fn warp(center: [f32; 3], size: [f32; 3], angle: f32, to: u16, kind: i32) -> Warp {
    let (c, s) = (fx3(center), fx3(size));
    let half = s.map(|v| v / 2);
    Warp {
        volume: WarpVolume {
            high: [0, 1, 2].map(|i| c[i] + half[i]),
            low: [0, 1, 2].map(|i| c[i] - half[i]),
            center: c,
            angle: fx(angle) as i16,
            radius_sq: square(s[0] / 2) + square(s[2] / 2),
        },
        kind,
        destination: to,
        arrival: fx3([10.0, 1.0, -3.0]),
        arrival_angle: fx(1.5) as i16,
        entry: -1,
    }
}

fn at(x: f32, y: f32, z: f32, input: Vec3) -> Leader {
    // Facing +z, like a hero who walked down the screen.
    Leader {
        position: Vec3::new(x, y, z),
        yaw: yaw_of_angle(0.0),
        input,
    }
}

const IDLE: Vec3 = Vec3::ZERO;
const NORTH: Vec3 = Vec3::new(0.0, 0.0, -1.0);

#[test]
fn rotated_boxes_turn_the_point_back_by_minus_their_angle() {
    // A box 4 wide (x) and 1 deep (z) turned by a quarter of a turn plus an eighth.
    let center = fx3([0.0, 0.0, 0.0]);
    let b = [fx(2.0), fx(1.0), fx(0.5), fx(-2.0), fx(-1.0), fx(-0.5)];
    let angle = fx(core::f32::consts::FRAC_PI_4);
    let r = square(fx(2.0)) + square(fx(0.5));
    // local x = x cos a - z sin a, local z = x sin a + z cos a.
    assert!(in_rotated_box(fx3([1.0, 0.0, -1.0]), &b, angle, center, r));
    assert!(!in_rotated_box(fx3([1.0, 0.0, 1.0]), &b, angle, center, r));
    // The same point against the unturned box.
    assert!(in_rotated_box(fx3([1.0, 0.0, 0.2]), &b, 0, center, r));
    assert!(!in_rotated_box(fx3([1.0, 0.0, 1.0]), &b, 0, center, r));
}

#[test]
fn box_edges_are_included_and_height_counts() {
    let b = [fx(1.0), fx(2.0), fx(3.0), fx(-1.0), fx(0.0), fx(1.0)];
    assert!(in_box([fx(1.0), fx(2.0), fx(3.0)], &b));
    assert!(in_box([fx(-1.0), fx(0.0), fx(1.0)], &b));
    assert!(!in_box([fx(1.0) + 1, fx(1.0), fx(2.0)], &b));
    assert!(!in_box([0, fx(2.0) + 1, fx(2.0)], &b));
    assert!(!in_box([0, fx(-0.1), fx(2.0)], &b));
    assert!(!in_box([0, fx(1.0), fx(3.0) + 1], &b));
    assert!(!in_box([0, fx(1.0), fx(1.0) - 1], &b));
}

#[test]
fn a_turned_box_refuses_beyond_its_radius_an_unturned_one_does_not() {
    // The radius is made from width and height here (added areas): a tall thin box.
    let area = |degrees| {
        let op = Op {
            code: 0x8f,
            arg: 3,
            operands: [1.0f32, 5.0, 40.0, -1.0, -5.0, -40.0]
                .map(Value::Float)
                .into_iter()
                .chain([Value::Int(degrees)])
                .collect(),
        };
        AddedArea::new(&op).unwrap()
    };
    // Width 2 and height 10: radius² = 1 + 25. A point 30 away across the ground is inside
    // the box but beyond the radius.
    let far = fx3([0.0, 0.0, 30.0]);
    assert!(area(0).holds(far));
    assert!(!area(1).holds(far));
    assert!(area(1).holds(fx3([0.0, 0.0, 3.0])));
}

#[test]
fn zones_without_depth_are_cylinders_from_the_center_up() {
    let mut slot = Slot::new(&zone(5, [0.0, 1.0, 0.0], [1.0, 2.0, 0.0], 0.0, 0.0));
    assert!(slot.holds(fx3([0.9, 1.0, 0.0])));
    assert!(slot.holds(fx3([0.0, 3.0, 0.5])));
    assert!(!slot.holds(fx3([1.1, 1.5, 0.0])));
    assert!(!slot.holds(fx3([0.0, 0.9, 0.0])));
    assert!(!slot.holds(fx3([0.0, 3.1, 0.0])));
}

#[test]
fn once_only_zones_hold_until_left_and_disabled_ones_never() {
    let inside = fx3([0.0, 0.0, 0.0]);
    let outside = fx3([9.0, 0.0, 0.0]);
    let mut slot = Slot::new(&zone(3, [0.0; 3], [2.0, 2.0, 2.0], 0.0, 0.0));
    slot.bits = ONCE;
    assert!(slot.holds(inside));
    assert!(slot.holds(inside));
    assert!(!slot.holds(outside));
    // Left after being entered: spent.
    assert!(!slot.holds(inside));
    let mut off = Slot::new(&zone(3, [0.0; 3], [2.0, 2.0, 2.0], 0.0, 0.0));
    off.bits = OFF;
    assert!(!off.holds(inside));
}

fn field(regions: &Regions, file: Option<Vec<u8>>) -> Field {
    Field::new(1100, regions, file).unwrap()
}

fn changes(events: &[Event]) -> Vec<MapChange> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::MapChange(c) => Some(*c),
            _ => None,
        })
        .collect()
}

fn door_regions() -> Regions {
    Regions {
        warps: Vec::new(),
        // A doorway at the origin facing +z: the hero enters walking -z.
        zones: vec![doorway([0.0; 3], [2.0, 2.0, 2.0], 0.0, FLAG_WARP, 7)],
    }
}

#[test]
fn walking_into_a_doorway_opens_it_on_the_fourth_frame() {
    let mut f = field(&door_regions(), None);
    let mut state = State::default();
    for frame in 0..3 {
        let events = f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state);
        assert!(events.is_empty(), "frame {frame}");
    }
    let events = f.step(&at(0.0, 0.0, 0.4, NORTH), &mut state);
    let changed = changes(&events);
    assert_eq!(changed.len(), 1);
    let c = changed[0];
    assert_eq!(c.destination, 7);
    assert_eq!(
        c.cause,
        Cause::Doorway {
            class: 0,
            number: 9
        }
    );
    assert_eq!(c.position, Some(Vec3::new(1.0, 0.5, 2.0)));
    assert!((c.angle.unwrap() - 3.0).abs() < 1e-3);
}

#[test]
fn a_doorway_needs_the_hero_to_walk_into_it() {
    let mut state = State::default();
    // Standing still inside: nothing, however long.
    let mut f = field(&door_regions(), None);
    for _ in 0..20 {
        assert!(f.step(&at(0.0, 0.0, 0.5, IDLE), &mut state).is_empty());
    }
    // Walking out (+z, the way the door faces) is not accepted.
    let mut f = field(&door_regions(), None);
    for _ in 0..20 {
        assert!(f.step(&at(0.0, 0.0, 0.5, -NORTH), &mut state).is_empty());
    }
    // Walking along the door (sideways) never counts to three.
    let mut f = field(&door_regions(), None);
    for _ in 0..20 {
        assert!(f.step(&at(0.0, 0.0, 0.5, Vec3::X), &mut state).is_empty());
    }
    // Outside the volume.
    let mut f = field(&door_regions(), None);
    for _ in 0..20 {
        assert!(f.step(&at(0.0, 0.0, 4.0, NORTH), &mut state).is_empty());
    }
}

#[test]
fn a_straying_step_restarts_the_doorway_count() {
    let mut f = field(&door_regions(), None);
    let mut state = State::default();
    for _ in 0..2 {
        f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state);
    }
    // Sideways for one frame: back to zero.
    f.step(&at(0.0, 0.0, 0.5, Vec3::X), &mut state);
    for _ in 0..3 {
        assert!(f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state).is_empty());
    }
    assert!(Field::changes_map(
        &f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state)
    ));
}

#[test]
fn a_used_doorway_is_skipped() {
    let mut f = field(&door_regions(), None);
    f.doorways[0].flags |= DOORWAY_USED;
    let mut state = State::default();
    for _ in 0..10 {
        assert!(f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state).is_empty());
    }
}

/// A trigger file from records of cells: `[room, from chapter, from step, to chapter, to
/// step, event, words…]`, ints and floats typed like the game's tables.
enum Cell {
    I(u32),
    F(f32),
}

fn trigger_file(records: &[Vec<Cell>]) -> Vec<u8> {
    let mut body = Vec::new();
    for cells in records {
        body.extend_from_slice(&1u16.to_le_bytes());
        body.push(cells.len() as u8);
        for chunk in cells.chunks(4) {
            body.push(chunk.iter().enumerate().fold(0u8, |acc, (i, c)| {
                acc | (match c {
                    Cell::I(_) => 1u8,
                    Cell::F(_) => 2,
                }) << (2 * i)
            }));
        }
        while body.len() % 4 != 0 {
            body.push(0xff);
        }
        for c in cells {
            let raw = match c {
                Cell::I(v) => *v,
                Cell::F(f) => f.to_bits(),
            };
            body.extend_from_slice(&raw.to_le_bytes());
        }
    }
    let pool = (0x10 + body.len()).next_multiple_of(16);
    let mut data = Vec::new();
    for h in [records.len() as u32, pool as u32, 0, 0] {
        data.extend_from_slice(&h.to_le_bytes());
    }
    data.extend(body);
    data.resize(pool, 0xff);
    data
}

fn word(code: u16, arg: u16) -> Cell {
    Cell::I(u32::from(code) << 16 | u32::from(arg))
}

/// A record loaded in room 1100 at every story point.
fn record(event: u32, words: Vec<Cell>) -> Vec<Cell> {
    let mut cells = vec![
        Cell::I(1100),
        Cell::I(0),
        Cell::I(0),
        Cell::I(99),
        Cell::I(99),
        Cell::I(event),
    ];
    cells.extend(words);
    cells
}

#[test]
fn a_trigger_warp_replaces_the_doorways_destination() {
    // Event 17 for door (class 0, number 9): warp to room 55 at (3, 0, 4) facing 1.
    let file = trigger_file(&[record(
        17,
        vec![
            word(0x1d, 0),
            Cell::I(9),
            Cell::I(0),
            word(0x79, 55),
            word(0x7a, 0),
            Cell::F(3.0),
            Cell::F(0.0),
            Cell::F(4.0),
            word(0x7b, 0),
            Cell::F(1.0),
        ],
    )]);
    let mut f = field(&door_regions(), Some(file));
    let mut state = State::default();
    let mut events = Vec::new();
    for _ in 0..4 {
        events = f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state);
    }
    let changed = changes(&events);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].destination, 55);
    assert_eq!(changed[0].position, Some(Vec3::new(3.0, 0.0, 4.0)));
    assert_eq!(changed[0].cause, Cause::Trigger);
    assert!((changed[0].angle.unwrap() - 1.0).abs() < 1e-3);
}

#[test]
fn a_trigger_for_another_door_does_not_apply() {
    // Number 8, not 9: the door's own destination stands, and nothing else is requested.
    let file = trigger_file(&[record(
        17,
        vec![word(0x1d, 0), Cell::I(8), Cell::I(0), word(0x79, 55)],
    )]);
    let mut f = field(&door_regions(), Some(file));
    let mut state = State::default();
    let mut events = Vec::new();
    for _ in 0..4 {
        events = f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state);
    }
    assert_eq!(changes(&events)[0].destination, 7);
}

fn area_regions() -> Regions {
    Regions {
        warps: Vec::new(),
        zones: vec![
            area(7, [0.0; 3], [2.0, 2.0, 2.0]),
            area(8, [5.0, 0.0, 0.0], [2.0, 2.0, 2.0]),
        ],
    }
}

fn area_file() -> Vec<u8> {
    trigger_file(&[
        record(2, vec![word(7, 7), word(0x64, 101)]),
        record(5, vec![word(7, 7), word(0x64, 102)]),
        record(2, vec![word(7, 8), word(0x64, 103)]),
        record(5, vec![word(7, 8), word(0x64, 104)]),
    ])
}

#[test]
fn leaving_one_area_for_another_fires_5_then_2() {
    let mut f = field(&area_regions(), Some(area_file()));
    let mut state = State::default();
    assert!(f.step(&at(9.0, 0.0, 0.0, IDLE), &mut state).is_empty());
    assert!(state.flags.is_empty());
    f.step(&at(0.0, 0.0, 0.0, IDLE), &mut state);
    assert_eq!(state.flags.iter().copied().collect::<Vec<_>>(), [101]);
    // Still inside: nothing more.
    f.step(&at(0.5, 0.0, 0.0, IDLE), &mut state);
    assert_eq!(state.flags.len(), 1);
    // Straight into the other area.
    f.step(&at(5.0, 0.0, 0.0, IDLE), &mut state);
    assert_eq!(
        state.flags.iter().copied().collect::<Vec<_>>(),
        [101, 102, 103]
    );
    // Out of every area: leaving fires 5 only.
    f.step(&at(9.0, 0.0, 0.0, IDLE), &mut state);
    assert!(state.flags.contains(&104));
}

#[test]
fn the_first_area_of_the_list_wins_and_the_newest_is_first() {
    // Two areas overlap; the one defined last is tried first.
    let regions = Regions {
        warps: Vec::new(),
        zones: vec![
            area(7, [0.0; 3], [2.0, 2.0, 2.0]),
            area(8, [0.0; 3], [2.0, 2.0, 2.0]),
        ],
    };
    let mut f = field(&regions, Some(area_file()));
    let mut state = State::default();
    f.step(&at(0.0, 0.0, 0.0, IDLE), &mut state);
    assert_eq!(state.flags.iter().copied().collect::<Vec<_>>(), [103]);
}

#[test]
fn the_load_event_adds_areas_that_fire_events_when_entered() {
    // Event 20 adds area 10: box (2,1,2)-(-2,-1,-2), turned 0°.
    let file = trigger_file(&[
        record(
            20,
            vec![
                word(9, 1100),
                word(0x8f, 10),
                Cell::F(2.0),
                Cell::F(1.0),
                Cell::F(2.0),
                Cell::F(-2.0),
                Cell::F(-1.0),
                Cell::F(-2.0),
                Cell::I(0),
            ],
        ),
        record(2, vec![word(7, 10), word(0x64, 201)]),
        record(5, vec![word(7, 10), word(0x64, 202)]),
    ]);
    let mut f = field(&Regions::default(), Some(file));
    let mut state = State::default();
    assert!(f.enter(&mut state, false).is_empty());
    assert!(f.step(&at(9.0, 0.0, 0.0, IDLE), &mut state).is_empty());
    f.step(&at(1.0, 0.0, 1.0, IDLE), &mut state);
    assert_eq!(f.entered_area, Some(10));
    assert_eq!(state.flags.iter().copied().collect::<Vec<_>>(), [201]);
    f.step(&at(3.0, 0.0, 1.0, IDLE), &mut state);
    assert_eq!(state.flags.iter().copied().collect::<Vec<_>>(), [201, 202]);
}

#[test]
fn the_arrival_event_only_runs_after_a_transition() {
    let file = trigger_file(&[record(3, vec![word(9, 1100), word(0x64, 301)])]);
    let mut f = field(&Regions::default(), Some(file));
    let mut state = State::default();
    f.enter(&mut state, false);
    assert!(state.flags.is_empty());
    f.enter(&mut state, true);
    assert!(state.flags.contains(&301));
}

#[test]
fn entering_a_room_selects_its_scenario_track() {
    let mut f = Field::new(1700, &Regions::default(), None).unwrap();
    let mut state = State::default();
    f.enter(&mut state, false);
    assert_eq!(state.track, 2);
}

#[test]
fn triggers_load_by_the_progress_of_the_rooms_track() {
    // The trigger exists from chapter 2: with the story at 1.1 it is not loaded.
    let mut late = record(3, vec![word(9, 1100), word(0x64, 5)]);
    late[1] = Cell::I(2);
    late[2] = Cell::I(1);
    let file = trigger_file(&[late]);
    let mut f = field(&Regions::default(), Some(file));
    let story = dq9_formats::cast::Story {
        chapter: 1,
        step: 1,
        substep: 1,
        ..Default::default()
    };
    let mut state = initial_state(&story);
    f.enter(&mut state, true);
    assert!(state.flags.is_empty());
    for track in &mut state.tracks {
        track.chapter = 2;
        track.step = 1;
    }
    f.enter(&mut state, true);
    assert!(state.flags.contains(&5));
}

#[test]
fn a_warp_box_changes_map_after_event_6() {
    let regions = Regions {
        warps: vec![warp([0.0, 0.0, 10.0], [2.0, 2.0, 1.0], 0.0, 42, 0)],
        zones: Vec::new(),
    };
    let file = trigger_file(&[record(6, vec![word(9, 1100), word(0x64, 401)])]);
    let mut f = field(&regions, Some(file));
    let mut state = State::default();
    assert!(f.step(&at(0.0, 0.0, 8.0, NORTH), &mut state).is_empty());
    let events = f.step(&at(0.9, 0.5, 10.2, NORTH), &mut state);
    let changed = changes(&events);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].destination, 42);
    assert_eq!(changed[0].position, Some(Vec3::new(10.0, 1.0, -3.0)));
    assert_eq!(changed[0].cause, Cause::Warp { index: 0, kind: 0 });
    assert!(state.flags.contains(&401));
    // The box has a height: too high above its center, no change.
    let mut f = field(&regions, None);
    assert!(f.step(&at(0.0, 1.5, 10.0, NORTH), &mut state).is_empty());
}

#[test]
fn a_turned_warp_box_follows_its_angle() {
    // Long along x (4) and thin along z (0.5), turned a quarter turn plus: the long side
    // now lies along the world's z axis.
    let quarter = core::f32::consts::FRAC_PI_2;
    let regions = Regions {
        warps: vec![warp([0.0; 3], [4.0, 2.0, 0.5], quarter, 3, 0)],
        zones: Vec::new(),
    };
    let mut state = State::default();
    let mut f = field(&regions, None);
    assert!(Field::changes_map(
        &f.step(&at(0.0, 0.0, 1.8, IDLE), &mut state)
    ));
    let mut f = field(&regions, None);
    assert!(f.step(&at(1.8, 0.0, 0.0, IDLE), &mut state).is_empty());
}

#[test]
fn trigger_requests_without_a_map_change_are_handed_back() {
    // Event 2 of area 7 talks to NPC 3 with context 5 (deferred action 0x76).
    let file = trigger_file(&[record(2, vec![word(7, 7), word(0x76, 3), Cell::I(5 << 16)])]);
    let mut f = field(&area_regions(), Some(file));
    let mut state = State::default();
    let events = f.step(&at(0.0, 0.0, 0.0, IDLE), &mut state);
    assert_eq!(
        events,
        [Event::Request(Request::Talk { npc: 3, context: 5 })]
    );
}

#[test]
fn an_unmodelled_condition_is_reported_not_evaluated() {
    let file = trigger_file(&[record(2, vec![word(0x45, 0), Cell::I(0), word(0x64, 9)])]);
    let mut f = field(&area_regions(), Some(file));
    let mut state = State::default();
    let events = f.step(&at(0.0, 0.0, 0.0, IDLE), &mut state);
    assert_eq!(events, [Event::Unsupported(0x45)]);
    assert!(state.flags.is_empty());
}

#[test]
fn trigger_file_names() {
    assert_eq!(
        trigger_path("M01M09").as_deref(),
        Some("data/scenario/triggerM01.bin")
    );
    assert_eq!(
        trigger_path("F01").as_deref(),
        Some("data/scenario/triggerF.bin")
    );
    assert_eq!(trigger_path("M"), None);
}

#[test]
fn yaw_of_the_games_angle_matches_the_heros_convention() {
    // Walking +z is yaw π for `Hero::step` (atan2(x, -z)) and game angle 0.
    assert!((yaw_of_angle(0.0) - core::f32::consts::PI).abs() < 1e-6);
    assert!(
        (yaw_of_angle(core::f32::consts::FRAC_PI_2) - core::f32::consts::FRAC_PI_2).abs() < 1e-6
    );
    assert!((angle_of_yaw(yaw_of_angle(0.7)) - 0.7).abs() < 1e-6);
}

#[test]
fn actions_6c_and_88_mark_and_unmark_the_matching_doorway() {
    // Doorway class 0, number 9, facing 0°: marked used by the load event, released when
    // the leader enters area 7. Another number is left alone.
    let file = trigger_file(&[
        record(
            20,
            vec![word(9, 1100), word(0x6c, 0), Cell::I(9), Cell::I(0)],
        ),
        record(
            20,
            vec![word(9, 1100), word(0x6c, 0), Cell::I(8), Cell::I(0)],
        ),
        record(2, vec![word(7, 7), word(0x88, 0), Cell::I(9), Cell::I(0)]),
    ]);
    let mut regions = door_regions();
    regions
        .zones
        .push(area(7, [20.0, 0.0, 0.0], [2.0, 2.0, 2.0]));
    let mut f = field(&regions, Some(file));
    let mut state = State::default();
    f.enter(&mut state, false);
    assert_eq!(f.doorway_flags(), [(0, 9, FLAG_WARP | DOORWAY_USED)]);
    // Marked: walking in does nothing.
    for _ in 0..10 {
        assert!(f.step(&at(0.0, 0.0, 0.5, NORTH), &mut state).is_empty());
    }
    f.step(&at(20.0, 0.0, 0.0, IDLE), &mut state);
    assert_eq!(f.doorway_flags(), [(0, 9, FLAG_WARP)]);
}

#[test]
fn a_marked_doorway_with_the_wrong_facing_is_not_matched() {
    // 90° is not the doorway's facing (0 radians).
    let file = trigger_file(&[record(
        20,
        vec![word(9, 1100), word(0x6c, 0), Cell::I(9), Cell::I(90)],
    )]);
    let mut f = field(&door_regions(), Some(file));
    f.enter(&mut State::default(), false);
    assert_eq!(f.doorway_flags(), [(0, 9, FLAG_WARP)]);
}

#[test]
fn a_scripted_battle_chains_the_scene_after_its_victory() {
    // ev21507 ends -> battle 26 (action 0x78); won -> ev21520; lost -> nothing.
    let file = trigger_file(&[
        record(11, vec![word(8, 21507), word(0x78, 26)]),
        record(15, vec![word(0xc, 26), word(0x77, 21520)]),
    ]);
    let mut f = field(&Regions::default(), Some(file));
    let mut state = State::default();
    assert_eq!(
        f.event_ended(21507, &mut state),
        [Event::Request(Request::Battle(26))]
    );
    assert_eq!(f.event_ended(21508, &mut state), []);
    assert_eq!(
        f.battle_ended(26, true, &mut state),
        [Event::Request(Request::Event(21520))]
    );
    assert_eq!(f.battle_ended(25, true, &mut state), []);
    assert_eq!(f.battle_ended(26, false, &mut state), []);
}

#[test]
fn the_end_of_a_conversation_fires_event_1_with_the_npc_context_and_answer() {
    // NPC 101 asked with context 193 and answered with the first choice starts ev21591.
    let file = trigger_file(&[record(
        1,
        vec![
            word(6, 101),
            word(0xb, 193),
            word(0x10, 0),
            word(0x77, 21591),
        ],
    )]);
    let mut f = field(&Regions::default(), Some(file));
    let mut state = State::default();
    assert_eq!(
        f.talk_ended(101, 193, Some(0), &mut state),
        [Event::Request(Request::Event(21591))]
    );
    // No prompt: the answer reads 0.
    assert_eq!(
        f.talk_ended(101, 193, None, &mut state),
        [Event::Request(Request::Event(21591))]
    );
    assert_eq!(f.talk_ended(101, 193, Some(1), &mut state), []);
    assert_eq!(f.talk_ended(101, 192, Some(0), &mut state), []);
    assert_eq!(f.talk_ended(102, 193, Some(0), &mut state), []);
}
