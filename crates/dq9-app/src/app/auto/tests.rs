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
    app.play_auto(4_000, true);

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

    app.play_auto(3_000, true);

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
    for _ in 0..30 {
        app.play_loop(&mut bot, 2_000, true);
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
