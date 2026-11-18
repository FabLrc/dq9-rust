//! ROM-backed regressions of the autopilot (`docs/re/movement.md`): they walk with the hero's
//! own movement and check the observed map, height and event, not just a planned grid.

use super::*;

/// Start-up options of a ROM-backed regression on an explicit map and story point.
fn options(map: &str, story: (u8, u8, u8)) -> crate::app::Options {
    crate::app::Options {
        map: Some(map.into()),
        save: None,
        time: None,
        clock: None,
        layout: crate::app::Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: story.0,
            step: story.1,
            substep: story.2,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: false,
        battle: None,
        play: Default::default(),
    }
}

/// A number of a checkpoint's JSON line (`"x":1.234`).
fn number(line: &str, key: &str) -> f32 {
    let marker = format!("\"{key}\":");
    let start = line
        .find(&marker)
        .unwrap_or_else(|| panic!("no {key} in {line}"))
        + marker.len();
    let end = line[start..]
        .find([',', '}'])
        .map_or(line.len(), |end| start + end);
    line[start..end]
        .parse()
        .unwrap_or_else(|_| panic!("{key} in {line} is not a number"))
}

/// The pilot must not step into the well warp of Angel Falls on its way to the chapter 2
/// opening. From `M01` at story 2.1.1 it talks to NPC 6 at `(32.30, -0.40, -3.70)` and event
/// 2110 plays; the `0x72` warp index 6 (`M01M08`, box `x 30..32.5`, `z -7.25..-4.75`) fires
/// from mere presence, so a route along the well's north edge teleported the hero. The
/// unwalked first run (`build/evidence/j4-chapter2-route/run1.log`) landed in 1108 on every
/// walk out of the village.
#[test]
fn the_pilot_walks_around_the_well_of_angel_falls_to_the_chapter_2_opening() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("M01", (2, 1, 1)), Some(rom.to_vec()));
    // The opening checkpoint is what the test reads: the run stops on it instead of on the
    // budget, which the pilot spent walking on afterwards (its 2110 checkpoint lands at tick
    // 750 of the 4 000 fixed steps, measured).
    let played = |app: &App| {
        app.checkpoints
            .iter()
            .any(|line| line.contains("\"event\":2110"))
    };
    app.play_auto_until(4_000, true, played);

    let opening = app
        .checkpoints
        .iter()
        .position(|line| line.contains("\"event\":2110"))
        .expect("the chapter 2 opening (event 2110) must play");
    assert!(
        app.checkpoints[..opening]
            .iter()
            .all(|line| !line.contains("\"map\":1108")),
        "no checkpoint may reach M01M08 before event 2110: {:?}",
        &app.checkpoints[..opening]
    );
    let line = &app.checkpoints[opening];
    assert!(
        line.contains("\"map\":1100"),
        "event 2110 plays in Angel Falls (1100): {line}"
    );
    let (x, y, z) = (number(line, "x"), number(line, "y"), number(line, "z"));
    assert!(
        (y + 0.4).abs() < 0.8,
        "the hero stands on the village plaza (y -0.40), not the room below: {line}"
    );
    assert!(
        x > 25.0 && (-8.0..-2.0).contains(&z),
        "the hero walked to the east side of the plaza, by NPC 5/6: {line}"
    );
    assert!(
        app.journal.events().contains(&2110),
        "the journal lists event 2110: {:?}",
        app.journal.events()
    );
}

/// Leaving Erinn's room (`M01M10`, story 2.2.1) needs its doors pushed and the warp entered
/// by the stair: the only exit is the warp to `M01M07` (box `x 1.85..2.85`, `y -0.76..0.74`,
/// `z 4.73..6.73`), behind door `0/2` (`0x73` zone at `(2.72, 1.48)`). The stair is a lane
/// 0.2 wide (hero centres `z 4.8..5.0`) down the ramp, entered round the end of a wall
/// through a gap of centimetres beside the room's east wall: the 0.5 grid cannot join it, so
/// the pilot walks the last metres with the hero's own movement
/// (`docs/re/movement.md`; failed runs `build/evidence/j4-chapter2-route/run7.log`, `run12.err`).
#[test]
fn the_pilot_pushes_erinns_doors_and_leaves_map_1110_by_the_stair_warp() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("1110", (2, 2, 1)), Some(rom.to_vec()));
    app.place_hero(2.5, -1.5).expect("the room's floor");
    // The first walked frame raises `KEY_REFRESH_FLAG` (`KeyManager::RefreshKeys`), which is
    // part of the story digest: a run that has walked for minutes already has it. Set it
    // without moving so the goal is not cancelled by its own first step.
    app.pilot_dir = Some(Vec3::ZERO);
    app.run_steps(1);

    // The warp to M01M07 is what the test reads, with the arrival checkpoint captured in the
    // same step: the run stops there instead of on the 3 000-step budget.
    let left_by_warp = |app: &App| {
        app.journal.steps.iter().any(|(_, step)| {
            matches!(step, Step::MapChange { from: Some(1110), to: 1107, cause }
                if cause.contains("Warp { index: 0"))
        })
    };
    app.play_auto_until(3_000, true, left_by_warp);

    let steps = &app.journal.steps;
    let pushed = steps.iter().position(|(_, step)| {
        matches!(step, Step::Autopilot(what) if what.contains("walks into the door 0/2"))
    });
    let left = steps.iter().position(|(_, step)| {
        matches!(step, Step::MapChange { from: Some(1110), to: 1107, cause }
            if cause.contains("Warp { index: 0"))
    });
    assert!(
        pushed.is_some() && left.is_some() && pushed < left,
        "the pilot must push door 0/2, then leave by the warp to M01M07: {steps:?}"
    );
    // The hero is now in M01M07, at the warp's arrival on the ground floor.
    let arrival = app
        .checkpoints
        .iter()
        .find(|line| line.contains("\"map\":1107"))
        .expect("a checkpoint in M01M07");
    assert!(
        (number(arrival, "y") - 0.16).abs() < 0.1,
        "the arrival is on the floor of M01M07 (y 0.16): {arrival}"
    );
    assert!(
        app.checkpoints
            .iter()
            .filter(|line| line.contains("\"map\":1110"))
            .all(|line| number(line, "x") > -5.0 && number(line, "y") > -1.0),
        "the hero stays inside the room until the warp fires: {:?}",
        app.checkpoints
    );
}

/// The walk into the warp of `M01M10` starts from the east landing, north of the wall that
/// shields the stair: the search must find the gap by the room's east wall, the lane, and the
/// ramp down to the warp's box, and the warp must fire with the hero **on the ramp**, lower
/// than the landing but inside the box's heights, not from the walkway above it.
#[test]
fn the_hero_walks_the_stair_lane_into_the_warp_of_erinns_room() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("1110", (2, 2, 1)), Some(rom.to_vec()));
    app.place_hero(4.0, 3.0).expect("the landing");
    let hero = app.hero.expect("a hero");
    let landing = hero.position();
    let exit = app
        .field
        .as_ref()
        .expect("a field")
        .exits()
        .into_iter()
        .find(|exit| exit.destination == Some(1107))
        .expect("the warp to M01M07");
    assert!(
        !exit_contains(&exit, landing),
        "the landing is outside the volume: {landing:?}"
    );

    let plan = app
        .approach_plan(hero, &exit)
        .expect("a walk from the landing into the volume");
    let mut before = landing;
    let mut fed = 0;
    while app.current.as_ref().map(|(entry, _)| entry.id) == Some(1110) {
        assert!(fed < plan.len(), "the plan ended in the room: {before:?}");
        // The field takes the pad on every second step: the same cadence as the pilot's.
        app.pilot_dir = Some(plan[fed]);
        if app.tick.is_multiple_of(2) {
            fed += 1;
        }
        before = app.hero.expect("a hero").position();
        app.run_steps(1);
    }
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(1107),
        "the warp leads to M01M07"
    );
    // One update (at most 0.2 unit) before the warp fired, the hero was lower than the
    // landing (0.16) and inside the box's heights, at the box's side.
    let volume = exit.warp.expect("a warp");
    assert!(
        before.y < landing.y - 0.1 && before.y > volume.low[1] as f32 / 4096.0,
        "the hero went down the ramp into the box: {before:?} from {landing:?}"
    );
    assert!(
        flat(approach::nearest(&volume, before) - before).length() < 0.25,
        "the hero was at the volume's edge when it fired: {before:?}"
    );
}

/// At 2.2.3 the mayor's attendant (NPC 16 of `M01M05`) starts event 2230 when talked to:
/// `room 1105 2.2..2.2 event 0: if 0x6(16) 0x3(5) then 0x76(16, 192)`, then `event 1: if
/// 0x6(16) 0xb(192) then 0x77(2230)`. The rule above them, `0x6(16) 0x56(0)` (talk 193), wins
/// while the party has no guest: condition `0x56` falls back to "character 0xce exists" (the
/// party tags are 0 in the original, `docs/re/trigger.md`), and the end of event 2210 attaches
/// Ivor there. The test gives the state that ending leaves (`State::attach_guest`), nothing else.
#[test]
fn the_pilot_talks_to_the_mayors_attendant_for_event_2230_with_the_guest() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("1105", (2, 2, 3)), Some(rom.to_vec()));
    app.state.attach_guest();
    app.pilot_dir = Some(Vec3::ZERO);
    app.run_steps(1);

    app.play_auto(1_500, true);

    let events = app.journal.events();
    assert!(
        events.contains(&2230) && events.contains(&2240),
        "the attendant's conversation must start 2230, then 2240 follows: {events:?}"
    );
    assert!(
        app.state.tracks[0].flags[0].contains(&5),
        "event 2230 sets track flag 5: {:?}",
        app.state.tracks[0].flags
    );
}

/// The pilot leaves a family of maps for the rooms the story has something to start in:
/// from the field at 2.2 the Mountain Pass (`S01M01`, 5101: the arrival rule of event 2300 and
/// the rules of 2320, 2350, 22591, 22592) comes before the Hexagon (`D01`), whose rules are
/// loaded from 2.4 on (`dq9re triggers D01`: events 2500 to 2550 at `2.4..2.4`). Measured
/// in `run15`, which took the Hexagon first at 2.2.3 and ended in a hall with nothing to do.
#[test]
fn the_story_has_rules_in_the_pass_at_2_2_and_in_the_hexagon_at_2_4() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let app = App::new_headless(options("M01", (2, 2, 3)), Some(rom.to_vec()));
    assert!(
        app.story_rules_in(5101) >= 5,
        "the pass starts events at 2.2: {}",
        app.story_rules_in(5101)
    );
    assert_eq!(
        (app.story_rules_in(7100), app.story_rules_in(7101)),
        (0, 0),
        "the Hexagon has nothing loaded at 2.2"
    );
    let app = App::new_headless(options("M01", (2, 4, 1)), Some(rom.to_vec()));
    assert!(
        app.story_rules_in(7101) >= 5,
        "the Hexagon starts events at 2.4: {}",
        app.story_rules_in(7101)
    );
    assert_eq!(app.story_rules_in(9_999), 0, "an unknown room has no rule");
}

/// The walkable grid covers the ground of the map, not only the box of the places the pilot
/// must visit. In `D01M04` (the Hexagon's B1-A) every exit, area and the hero stand at `x 0`;
/// the way from the arrival warp to the doors of `D01M05` runs out to `x 20` and back along
/// corridors, which a box of those points plus a margin of 6 cut at `z 9.5` (341 nodes, "no
/// walkable way back"). `dq9re map D01M04 --reach 0 0.26 16` finds 2397 nodes with the
/// hero's own steps and reaches `(0, 0.26, -20)`.
#[test]
fn the_grid_follows_the_corridors_of_the_hexagon_out_of_the_box_of_its_doors() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("7104", (2, 4, 5)), Some(rom.to_vec()));
    app.place_hero(0.0, 16.0).expect("the arrival");
    let from = app.hero.expect("a hero").position();
    let field = app.field.as_ref().expect("a field");
    let exits = field.exits();
    let mut points = vec![from];
    points.extend(exits.iter().map(|e| e.position));
    points.extend(field.area_points());
    let mut bot = Pilot::default();
    app.ensure_grid(&mut bot, 7104, &points, from.y);

    let door = exits
        .iter()
        .find(|exit| exit.destination == Some(7105) && exit.door.is_some())
        .expect("a doorway to D01M05");
    let (end, route, stage) = app
        .exit_spot(&bot, 7104, from, door)
        .expect("a route to the doorway of D01M05");
    assert!(!stage, "the grid itself joins the arrival and the doorway");
    assert!(
        end.z < -20.0 && (end.y - 0.26).abs() < 0.2 && route.len() > 60,
        "the route runs the corridors to the doors on the floor at y 0.26: {end:?}, {} waypoints",
        route.len()
    );
    let farthest = route.iter().fold(0.0f32, |far, at| far.max(at.x.abs()));
    assert!(
        farthest > 12.0,
        "the route leaves the column of the points for a side corridor: x up to {farthest}"
    );
}

/// The pilot in route mode, with the rooms it knows to lead to each other.
fn route_bot(links: &[(u16, &[u16])]) -> Pilot {
    let mut bot = Pilot {
        mode: Some(route::Route::new(2750, 7)),
        ..Pilot::default()
    };
    for (room, to) in links {
        bot.links.insert(*room, to.to_vec());
    }
    bot
}

/// A route-mode pilot's journal lines of `kind` (`route: <kind>...`).
fn route_lines(app: &App, kind: &str) -> usize {
    app.journal
        .steps
        .iter()
        .filter(|(_, step)| matches!(step, Step::Autopilot(s) if s.starts_with(&format!("route: {kind}"))))
        .count()
}

/// Which facilities the story opens, from the lines the game would select (`dialogue::select`
/// on the room's track, as `Talk::start`): the village shop's keeper (NPC 19 of `M01M03`) sells
/// all along, while Ivor (NPC 7) only offers the inn from step 7 of chapter 2 and in chapter 3
/// (`dq9re talk data/scenario/M01B0.gp2 007_en.bin --step 4`: at 2.4 he talks of his father).
/// The pilot heals with herbs while the inn is shut, so it must not walk to Ivor for it.
#[test]
fn the_inn_is_shut_from_2_1_to_2_6_and_the_shop_is_open_all_along() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let rooms: &[(u16, &[u16])] = &[(1100, &[1102, 1103]), (1102, &[1100]), (1103, &[1100])];
    let at = |story: (u8, u8, u8)| {
        let mut app = App::new_headless(options("1100", story), Some(rom.to_vec()));
        app.run_steps(1);
        let bot = route_bot(rooms);
        (
            app.route_facility(&bot, route::Facility::Inn),
            app.route_facility(&bot, route::Facility::Shop),
        )
    };
    // Every step from 2.1 to 2.6 (the first substep): no inn, whoever is where.
    for step in 1..=6 {
        assert_eq!(at((2, step, 1)).0, None, "an inn at 2.{step}.1");
    }
    for story in [(2, 1, 1), (2, 4, 5)] {
        assert_eq!(at(story), (None, Some((1103, 19))), "at {story:?}");
    }
    for story in [(2, 7, 1), (3, 1, 1)] {
        assert_eq!(at(story).0, Some((1102, 7)), "at {story:?}");
    }
}

/// A hurt hero with gold at the village shop in route mode buys medicinal herbs with the shop
/// screen's Confirm presses, then uses one from the field menu (Items, the herb's row, the
/// target): the HP comes back, the gold goes, and the bag keeps the rest.
#[test]
fn the_pilot_buys_herbs_at_the_shop_when_hurt_and_uses_one_from_the_field_menu() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("1103", (2, 4, 5)), Some(rom.to_vec()));
    app.run_steps(1);
    app.party.members[0].max_hp = 20;
    app.party.members[0].hp = 4;
    app.party.gold = 60;
    let mut bot = route_bot(&[(1103, &[1100]), (1100, &[1103])]);
    app.play_loop(&mut bot, 600, true);
    let party = app.party();
    assert_eq!(party.members[0].hp, 20, "a herb heals the hero");
    assert!(party.gold < 60, "the herbs were paid: {}", party.gold);
    let herbs = party.bag.count(22000);
    assert!(herbs >= 1, "the unused herbs stay in the bag: {herbs}");
    assert_eq!(route_lines(&app, "uses a medicinal herb"), 1);
}

/// Ivor's inn at chapter 3: the pilot talks to him, the inn screen opens, Confirm picks Stay, and
/// the party is healed for the price of the bed.
#[test]
fn the_pilot_rests_at_ivors_inn_when_the_story_opens_it() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("1102", (3, 1, 1)), Some(rom.to_vec()));
    app.run_steps(1);
    app.party.members[0].max_hp = 20;
    app.party.members[0].hp = 4;
    app.party.gold = 60;
    let mut bot = route_bot(&[(1102, &[1100]), (1100, &[1102])]);
    app.play_loop(&mut bot, 600, true);
    let party = app.party();
    assert_eq!(party.members[0].hp, 20, "a night at the inn heals the hero");
    assert!(party.gold < 60, "the stay was paid: {}", party.gold);
    assert_eq!(party.bag.count(22000), 0, "no herb was bought");
}

/// After a defeat the pilot stops following the story: it hunts the roamers of the field and
/// goes on once the party is a level stronger (the bar). Deterministic with the seed of 7.
#[test]
fn after_a_defeat_the_pilot_hunts_the_field_until_a_level_up_then_the_story_goes_on() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut opts = options("F01", (2, 4, 5));
    opts.play = crate::app::BattlePlay {
        fight: true,
        seed: Some(7),
    };
    let mut app = App::new_headless(opts, Some(rom.to_vec()));
    app.set_roaming_enabled(true);
    app.run_steps(1);
    let mut bot = route_bot(&[]);
    bot.mode.as_mut().unwrap().lost_at(1);
    // Chunks of 250 fixed steps: the bar is read after each one, so the hunt ends within a
    // quarter of a second of the level-up. With the former 2 000-step chunks it stopped at
    // tick 4 000, the bar being crossed in the eleventh 250-step chunk (tick 2 751,
    // measured): at least 1 249 steps of fights paid for nothing.
    for _ in 0..240 {
        app.play_loop(&mut bot, 250, true);
        if route_lines(&app, "stronger") > 0 {
            break;
        }
    }
    let level = app.party().members[0].level();
    assert!(level >= 2, "the hunt gives a level: Lv {level}");
    assert_eq!(route_lines(&app, "stronger"), 1, "the bar is crossed once");
    assert!(
        !bot.mode.as_ref().unwrap().waiting(),
        "the story is released"
    );
}

/// Where the story resumes after a fall: the room of a fall outside a field, else the first
/// fall; a lost fight sets the bar, a won one does not. The journal is what the app writes.
#[test]
fn a_fall_sets_the_bar_and_where_the_story_resumes_a_fall_in_a_field_keeps_it() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("1100", (2, 4, 5)), Some(rom.to_vec()));
    app.run_steps(1);
    let mut bot = route_bot(&[]);
    let wipe_out = |app: &mut App, room: u16| {
        let from = app.journal.steps.len();
        app.journal.record(
            app.tick,
            Step::MapChange {
                from: Some(room),
                to: 1106,
                cause: "wipe-out".into(),
            },
        );
        from
    };
    // A fall in the Hexagon's fourth floor (7104): the story resumes there.
    let from = wipe_out(&mut app, 7104);
    app.route_observe(&mut bot, from);
    let route = bot.mode.as_ref().unwrap();
    assert!(route.waiting(), "the lost fight sets the bar");
    assert_eq!(route.resume_room(), Some(7104));
    // A fall in the field while hunting does not move it.
    let from = wipe_out(&mut app, 20001);
    app.route_observe(&mut bot, from);
    assert_eq!(bot.mode.as_ref().unwrap().resume_room(), Some(7104));
    // Another fall outside a field does.
    let from = wipe_out(&mut app, 7105);
    app.route_observe(&mut bot, from);
    assert_eq!(bot.mode.as_ref().unwrap().resume_room(), Some(7105));
    // A won set battle leaves the bar as it was.
    bot.mode.as_mut().unwrap().lost_at(99);
    let from = app.journal.steps.len();
    app.journal.record(
        app.tick,
        Step::Battle {
            number: 2,
            record: None,
            won: true,
            fought: None,
        },
    );
    app.route_observe(&mut bot, from);
    assert!(bot.mode.as_ref().unwrap().waiting());
    assert_eq!(route_lines(&app, "set battle 2 won"), 1);
}

/// Every map load shuts the doors again, so a route-mode pilot that comes back to a room forgets
/// the doors it opened on an earlier visit and pushes them again (`route1`, the first full run,
/// stopped at 2.6.4 in front of door 0/8 of Erinn's house, where NPC 98 waits for event 22594).
/// The story pilot keeps its memory, as before the route mode.
#[test]
fn a_route_pilot_forgets_the_doors_it_opened_when_it_comes_back_to_a_room() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let after_a_visit = |route_mode: bool| {
        let mut app = App::new_headless(options("1100", (2, 4, 5)), Some(rom.to_vec()));
        app.run_steps(1);
        let mut bot = if route_mode {
            route_bot(&[])
        } else {
            Pilot::default()
        };
        // A door of an earlier visit (no door of the map is 9/9).
        bot.opened.insert((9, 9));
        bot.built = Some(1107);
        app.pursue(&mut bot);
        bot.opened.contains(&(9, 9))
    };
    assert!(!after_a_visit(true), "the route mode pushes the door again");
    assert!(after_a_visit(false), "the story pilot keeps its memory");
}

/// D01M02 has two stairs up from D01M01 and one way on: coming in by the west one (warp 0, at
/// `(-7.80, 0.26, 13.35)`) the grid finds no way to the exit to D01M03 (route2: "no way on from
/// map 7102 to 7103"), so the pilot books that entrance as a dead end for the trip and goes back
/// out by it, and the next try takes the other stair.
#[test]
fn a_trip_that_finds_no_way_on_books_the_entrance_and_goes_back_out() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("7102", (2, 4, 5)), Some(rom.to_vec()));
    app.run_steps(1);
    app.place_hero(-7.8, 13.35)
        .expect("the west arrival is on the ground");
    let hero = app.hero.expect("the hero").position();
    let mut bot = route_bot(&[(7102, &[7101, 7103]), (7101, &[7102])]);
    let mut points = vec![hero];
    points.extend(
        app.field
            .as_ref()
            .expect("a field")
            .exits()
            .iter()
            .map(|e| e.position),
    );
    app.ensure_grid(&mut bot, 7102, &points, hero.y);
    // The pilot walked out of 7101 by one of its exits to 7102: its key is the trip's entrance.
    bot.arrived_by = Some((7101, 7102, 42));
    let digest = app.story_digest();
    assert!(
        app.route_travel(&mut bot, 7102, digest, hero, 7103),
        "the pilot walks back out"
    );
    assert!(bot.dead_ends.contains(&(7101, 7102, 42, digest)));
    match bot.goal {
        Some(Goal::Exit { exit, .. }) => assert_eq!(exit.destination, Some(7101)),
        other => panic!("a walk back to 7101 was expected: {other:?}"),
    }
}

/// A hurt party that walks from the Hexagon's B1-B (7105, where NPC 203 starts the Hexagoon)
/// to the shop must come back: the trip is booked as the story's resume room, as a fall is.
/// Otherwise the pilot's books (`left`, `tried_exits`) mark the way into the dungeon as taken at
/// this point of the story and nothing leads back once the herb is used: the `battles=show` run
/// of `j4-web-pilot` stopped with free control on `1107` without NPC 203 having been talked to,
/// because its party was hurt on arrival at 7105 (the shown fights had shifted the roamers'
/// draws), where the instant run's was whole.
#[test]
fn a_heal_trip_from_the_story_room_resumes_there_after_the_herb() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let mut app = App::new_headless(options("7105", (2, 4, 5)), Some(rom.to_vec()));
    app.run_steps(1);
    app.party.members[0].max_hp = 34;
    app.party.members[0].hp = 10;
    app.party.gold = 60;
    let mut bot = route_bot(&[(7105, &[7104]), (7104, &[7105, 1103]), (1103, &[7104])]);
    assert!(app.pursue(&mut bot), "the hurt party walks to the shop");
    assert_eq!(
        bot.mode.as_ref().unwrap().resume_room(),
        Some(7105),
        "the story goes on at 7105 once the party is healed"
    );
    // Back at 7105 healed: the wait ends, the room's books are forgotten and the talk is offered.
    app.party.members[0].hp = 34;
    bot.goal = None;
    assert!(app.pursue(&mut bot));
    assert_eq!(bot.mode.as_ref().unwrap().resume_room(), None);
    match bot.goal {
        Some(Goal::Talk { target, .. }) => assert_eq!(target.npc, 203),
        other => panic!("a walk to NPC 203 was expected: {other:?}"),
    }
}

/// Options of a route run from a new game (`--new-game --fight --seed 7 --route 2750`).
fn route_options() -> crate::app::Options {
    let mut options = options("M01", (1, 1, 1));
    options.map = None;
    options.new_game = true;
    options.play = crate::app::BattlePlay {
        fight: true,
        seed: Some(7),
    };
    options
}

/// The Web session steps the same logic as the native loop: with the frame's `speed` in fixed
/// steps, the checkpoints of a route run match those of `play_route` for the same number of
/// steps, scripted battle 26 (played by the engine at 123 s) included. The browser's only
/// difference is who calls the step.
#[test]
fn a_session_plays_the_same_run_as_the_native_route_loop() {
    let Some(rom) = testrom::load() else {
        return;
    };
    const STEPS: u64 = 8_000;
    let mut native = App::new_headless(route_options(), Some(rom.to_vec()));
    native.play_route(STEPS, 2750);

    let mut web = App::new_headless(route_options(), None);
    web.set_session(session::SessionSpec {
        route: Some(2750),
        speed: Some(500),
        budget_ms: 60_000,
        render: false,
        show_battles: false,
    });
    web.set_rom(rom.to_vec());
    assert!(web.session_running(), "the new game starts the session");
    while web.tick < STEPS {
        web.session_frame();
    }
    assert_eq!(web.tick, STEPS, "whole frames of 500 fixed steps");
    assert!(
        native
            .checkpoints
            .iter()
            .any(|line| line.contains("\"kind\":\"battle\"")),
        "battle 26 is inside the compared steps: {:?}",
        native.checkpoints.last()
    );
    assert_eq!(native.checkpoints_jsonl(), web.checkpoints_jsonl());
    assert_eq!(native.party().members[0].hp, web.party().members[0].hp);
}

/// A shown battle changes the clock, not the game: the headless session that presents its
/// battles (`battles=show`: transition, scene, windows) reaches the scripted battle 26 with the
/// state the instant session has after it (party HP, experience and gold, story, flags, map),
/// whatever the step it ends at. Only the step counts differ, 110 more for the transition and the
/// rounds of the fight.
#[test]
fn a_shown_battle_leaves_the_game_in_the_state_an_instant_one_does() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let run = |show_battles: bool| {
        let mut app = App::new_headless(route_options(), None);
        app.set_session(session::SessionSpec {
            route: Some(2750),
            speed: Some(500),
            budget_ms: 60_000,
            render: false,
            show_battles,
        });
        app.set_rom(rom.to_vec());
        while app.tick < 12_000 {
            app.session_frame();
        }
        // The checkpoint of the settled battle, from `"map"` on: the clock fields come before.
        let line = app
            .checkpoints
            .iter()
            .find(|line| line.contains("\"kind\":\"battle\""))
            .expect("battle 26 is inside the compared steps");
        let (_, state) = line
            .split_once("\"map\":")
            .expect("the state follows the clock");
        state.to_owned()
    };
    let instant = run(false);
    let shown = run(true);
    assert_eq!(instant, shown);
}

/// The SHA-256 of the route's checkpoint JSONL, pinned on `main` before the batch: the
/// acceptance file `build/evidence/j4-item-spend/route-before-checkpoints.jsonl` (12-char
/// prefix `8ce4af30e497`; `j4-field-menu-rules` re-ran the route byte-identically), re-pinned
/// by `j4-parity-regress` after the original message pacing (`d068809`, `f516d79`): the same
/// 972 checkpoints with **every non-tick field identical** and the game time at 3627.8 s
/// instead of 3035.5 s (`build/evidence/j4-parity-regress/route-2750-checkpoints.jsonl`,
/// 970 of the 972 lines differ only in `tick`/`seconds`/`vblank`).
const ROUTE_CHECKPOINTS_SHA256: &str =
    "3d27e89d9737c61f7596cf99714002ea5038fe6c97f6bf1351ab0249a8c5c02a";

/// The prologue and chapter 2 played by the pilot with the real battle engine, from a new
/// game to event 2750 (`--new-game --pilot --fight --route 2750 --seed 7 --headless`): the
/// Hexagoon is lost at Lv 4, 5, 6 and 7 and won at Lv 8, the story ends at 2.6.5, and the
/// checkpoints are the acceptance run's 972 lines, digest and all, at 3627.8 s of game time
/// (the original message pacing; 3057.2 s in the acceptance run). The digest is the whole
/// game state the run went through, so a changed draw, step, flag or map fails it.
///
/// `#[ignore = "long: ~17 min in release"]`: `cargo test` stays fast; `scripts/verify.ps1
/// -Only long` runs it before a milestone. Set `DQ9_ROUTE_REFERENCE` to a previous JSONL to
/// have a failure name the first checkpoint and field that differ.
#[test]
#[ignore = "long: ~19 min in release; run scripts/verify.ps1 -Only long"]
fn the_route_reaches_event_2750_with_the_pinned_checkpoints() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let started = std::time::Instant::now();
    let mut app = App::new_headless(route_options(), Some(rom.to_vec()));
    let stop = app.play_route(300_000, 2750);
    let elapsed = started.elapsed();
    let track = &app.state.tracks[0];
    // `route_summary` repeats the attempts at the end of the journal: the first five are the
    // fights as they were observed.
    let attempts: Vec<&str> = app
        .journal
        .steps
        .iter()
        .filter_map(|(_, step)| match step {
            Step::Autopilot(what) => what.strip_prefix("route: set battle 2 "),
            _ => None,
        })
        .take(5)
        .collect();
    let jsonl = app.checkpoints_jsonl();
    let digest = sha256_hex(jsonl.as_bytes());
    eprintln!(
        "route 2750: {stop:?} in {:.1} s of wall time, story {}.{}.{}, {} checkpoints, \
         sha256 {digest}",
        elapsed.as_secs_f64(),
        track.chapter,
        track.step,
        track.substep,
        app.checkpoints.len()
    );
    assert_eq!(
        stop,
        Stop::Reached { event: 2750 },
        "the route must end on the event it was asked to reach"
    );
    assert_eq!(
        (track.chapter, track.step, track.substep),
        (2, 6, 5),
        "the story must end chapter 2 at 2.6.5"
    );
    assert_eq!(
        attempts,
        [
            "lost at Lv 4: 3 rounds, 0 experience, 0 gold",
            "lost at Lv 5: 5 rounds, 0 experience, 0 gold",
            "lost at Lv 6: 6 rounds, 0 experience, 0 gold",
            "lost at Lv 7: 4 rounds, 0 experience, 0 gold",
            "won at Lv 8: 7 rounds, 175 experience, 100 gold, HP 5/53 left",
        ],
        "the Hexagoon (set battle 2) as the acceptance run fought it"
    );
    assert_eq!(
        jsonl.lines().count(),
        972,
        "the acceptance run's checkpoint count (sha256 {digest})"
    );
    let reference = std::env::var_os("DQ9_ROUTE_REFERENCE").map(|path| {
        let path = std::path::PathBuf::from(path);
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("reading DQ9_ROUTE_REFERENCE {}: {error}", path.display())
        })
    });
    let against = reference.as_deref().map_or_else(String::new, |reference| {
        first_checkpoint_difference(&jsonl, reference).map_or_else(
            || {
                format!(
                    "; identical to DQ9_ROUTE_REFERENCE ({} checkpoints)",
                    reference.lines().count()
                )
            },
            |what| format!("; first difference against DQ9_ROUTE_REFERENCE: {what}"),
        )
    });
    assert_eq!(
        digest, ROUTE_CHECKPOINTS_SHA256,
        "the checkpoint JSONL left the pinned route{against}"
    );
}

/// The first difference between two checkpoint JSONLs, for `DQ9_ROUTE_REFERENCE`: the
/// checkpoint index and the top-level field that differs, or the line counts.
fn first_checkpoint_difference(actual: &str, reference: &str) -> Option<String> {
    let actual: Vec<&str> = actual.lines().collect();
    let reference: Vec<&str> = reference.lines().collect();
    for (index, (line, pinned)) in actual.iter().zip(&reference).enumerate() {
        if line == pinned {
            continue;
        }
        let fields = checkpoint_fields(line);
        let expected = checkpoint_fields(pinned);
        if fields.len() != expected.len() {
            return Some(format!(
                "checkpoint {index} has {} fields, the reference {}: {line} against {pinned}",
                fields.len(),
                expected.len()
            ));
        }
        for ((key, value), (pinned_key, pinned_value)) in fields.iter().zip(&expected) {
            if key != pinned_key || value != pinned_value {
                return Some(format!(
                    "checkpoint {index} field \"{key}\" is {value}, the reference's \"{pinned_key}\" \
                     is {pinned_value}"
                ));
            }
        }
        return Some(format!(
            "checkpoint {index} differs in its layout: {line} against {pinned}"
        ));
    }
    (actual.len() != reference.len()).then(|| {
        format!(
            "{} checkpoints against the reference's {}",
            actual.len(),
            reference.len()
        )
    })
}

/// The top-level members of a checkpoint line as `(key, value)` slices of it.
fn checkpoint_fields(line: &str) -> Vec<(&str, &str)> {
    let mut fields = Vec::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut begin = 0usize;
    for (index, byte) in line.bytes().enumerate() {
        if quoted {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 1 => {
                fields.push(split_field(&line[begin..index]));
                begin = index + 1;
            }
            _ => {}
        }
    }
    fields.push(split_field(&line[begin..]));
    fields
}

/// One top-level member, `"key":value`, as its key and its value; the writer quotes every key.
fn split_field(field: &str) -> (&str, &str) {
    let field = field.trim();
    let Some(rest) = field.strip_prefix('"') else {
        return ("", field);
    };
    let Some((key, rest)) = rest.split_once('"') else {
        return ("", field);
    };
    (key, rest.strip_prefix(':').unwrap_or(rest))
}

/// The failure aid of the long test: it names the first checkpoint and field against
/// `DQ9_ROUTE_REFERENCE`, and the line counts when one run stopped earlier.
#[test]
fn the_reference_difference_names_the_first_checkpoint_and_field_that_moved() {
    let before = "{\"tick\":1,\"kind\":\"map\",\"event\":null,\"hero\":{\"x\":1.000,\"y\":2.000,\
                  \"z\":3.000,\"yaw\":0.000}}";
    let after = before.replace("\"event\":null", "\"event\":2750");
    assert_eq!(first_checkpoint_difference(before, before), None);
    let what = first_checkpoint_difference(&after, before).expect("a changed field");
    assert!(
        what.contains("checkpoint 0") && what.contains("\"event\""),
        "the field that moved is named: {what}"
    );
    let what = first_checkpoint_difference(&format!("{before}\n{after}\n"), &format!("{before}\n"))
        .expect("a changed line count");
    assert!(
        what.contains("2 checkpoints against the reference's 1"),
        "a shorter reference is named: {what}"
    );
}

/// SHA-256 (`FIPS 180-4`) of `bytes` as 64 lowercase hex digits. The pin needs one digest and
/// `dq9-app` has no hash crate (adding one would also change the wasm build's surface), so the
/// test spells it here; [`the_checkpoint_digest_matches_the_fips_vectors`] checks it.
fn sha256_hex(bytes: &[u8]) -> String {
    #[rustfmt::skip]
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = bytes.to_vec();
    let bits = (bytes.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut words = [0u32; 64];
        for (index, word) in block.as_chunks::<4>().0.iter().enumerate() {
            words[index] = u32::from_be_bytes(*word);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// The digest the checkpoint pin relies on, against the FIPS 180-4 vectors.
#[test]
fn the_checkpoint_digest_matches_the_fips_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}
