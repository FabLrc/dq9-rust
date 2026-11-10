//! Regressions of the field menu's state machine that need no ROM data: the notice's return,
//! the item list's order (bag first, then the held slots), the held slot's compaction, and the
//! shop's buy/sell switch. Item movement and equipment are tested with their own rules in
//! `transfer.rs`; the prices and cures in `dq9_core::facility`.

use super::*;
use dq9_core::battle::Random;
use dq9_core::party::{BAG_CAPACITY, Bag, Party};

fn party() -> Party {
    let mut party = Party::new_game();
    party.members[0].held_items = [None; HELD_ITEMS];
    party
}

/// A one-member party whose bag is loaded from a synthetic save. A save can carry two
/// stacks of the same item, which `Bag::add` cannot create; the equip screen must keep both.
fn party_with_bag(slots: &[(u16, u8)]) -> Party {
    use dq9_formats::save::{Block, FILE_SIZE, Save};
    let mut bytes = vec![0u8; FILE_SIZE];
    bytes[7577] = 1; // party count
    let bag = Block::Main.start() + 0x1d5c; // Main+0x1d5c, `Party::from_slot`'s bag
    bytes[bag + 8..bag + 10].copy_from_slice(&(slots.len() as u16).to_le_bytes());
    let quantities = bag + 12 + 2 * slots.len();
    for (index, &(item, quantity)) in slots.iter().enumerate() {
        let at = bag + 12 + 2 * index;
        bytes[at..at + 2].copy_from_slice(&item.to_le_bytes());
        bytes[quantities + index] = quantity;
    }
    let save = Save::parse(&bytes).expect("the synthetic save");
    Party::from_slot(&save.slot(0).expect("slot 0")).expect("the synthetic party")
}

#[test]
fn a_notice_returns_to_its_screen_and_a_final_one_closes_the_facility() {
    let mut services = Services {
        screen: Screen::Notice,
        notice: Some(Notice {
            text: b"ok".to_vec(),
            values: Vec::new(),
            target: None,
            vocation: None,
            next: NoticeNext::Return,
            return_screen: Screen::Menu,
        }),
        ..Services::default()
    };
    let mut party = party();
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    assert_eq!(
        services.press(Button::Confirm, &mut party, &mut rng, &mut clock),
        Effect::None
    );
    assert_eq!(services.screen, Screen::Menu);
    assert!(services.is_open());

    services.screen = Screen::Notice;
    services.notice = Some(Notice {
        text: b"bye".to_vec(),
        values: Vec::new(),
        target: None,
        vocation: None,
        next: NoticeNext::Close,
        return_screen: Screen::Inn,
    });
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert!(!services.is_open());
}

#[test]
fn the_item_list_keeps_bag_order_then_the_held_slots() {
    let mut party = party();
    assert!(party.bag.add_copies(22000, 2));
    assert!(party.bag.add_copies(22004, 1));
    party.members[0].held_items[1] = Some(22019);
    party.members[0].held_items[6] = Some(22000);
    let services = Services::default();
    assert_eq!(
        services.item_sources(&party),
        [
            Source::Bag(22000),
            Source::Bag(22004),
            Source::Held(1),
            Source::Held(6),
        ]
    );
}

#[test]
fn a_used_held_item_compacts_the_holes_down() {
    let mut party = party();
    party.members[0].held_items = [
        Some(22000),
        Some(22004),
        Some(22019),
        None,
        None,
        None,
        None,
        None,
    ];
    consume_held(&mut party, 0);
    assert_eq!(
        party.members[0].held_items[..3],
        [Some(22004), Some(22019), None]
    );
    // A later slot leaves the earlier ones in place.
    consume_held(&mut party, 1);
    assert_eq!(party.members[0].held_items[0], Some(22004));
    assert_eq!(party.members[0].held_items[1], None);
}

#[test]
fn the_shop_switches_between_buying_and_selling() {
    let mut services = Services {
        screen: Screen::Shop,
        ..Services::default()
    };
    assert_eq!(services.shop_mode, ShopMode::Buy);
    services.side(true);
    assert_eq!(services.shop_mode, ShopMode::Sell);
    services.side(false);
    assert_eq!(services.shop_mode, ShopMode::Buy);
    services.side(true);
    assert_eq!(services.shop_mode, ShopMode::Sell);
}

#[test]
fn a_bag_full_of_other_items_still_takes_no_held_one() {
    let mut party = party();
    party.members[0].held_items[0] = Some(22000);
    for item in 22001..22001 + 152 {
        assert!(party.bag.add(item), "item {item}");
    }
    assert_eq!(
        party
            .bag
            .slots()
            .iter()
            .filter(|s| s.item.is_some())
            .count(),
        152
    );
    assert!(!held_to_bag(&mut party, 0));
    assert_eq!(party.members[0].held_items[0], Some(22000));
}

/// A tagged record table, as `dq9_re` reads the ROM's `.bin` files (the encoder of
/// `dq9_formats::test_util`, repeated here because that helper is crate-private).
fn encode_table(records: &[dq9_formats::table::Record<'_>]) -> Vec<u8> {
    let (mut body, mut pool) = (Vec::new(), Vec::new());
    for record in records {
        body.extend(record.kind.to_le_bytes());
        body.push(record.values.len() as u8);
        let mut types = vec![0u8; record.values.len().div_ceil(4)];
        for (index, value) in record.values.iter().enumerate() {
            let kind = match value {
                dq9_formats::table::Value::Str(_) => 0,
                dq9_formats::table::Value::Int(_) => 1,
                dq9_formats::table::Value::Float(_) => 2,
            };
            types[index / 4] |= kind << (2 * (index % 4));
        }
        body.extend(types);
        body.resize(body.len().next_multiple_of(4), 0xff);
        for value in &record.values {
            let raw = match *value {
                dq9_formats::table::Value::Str(Some(s)) => {
                    let at = pool.len() as u32;
                    pool.extend(s);
                    pool.push(0);
                    at
                }
                dq9_formats::table::Value::Str(None) => u32::MAX,
                dq9_formats::table::Value::Int(i) => i,
                dq9_formats::table::Value::Float(f) => f.to_bits(),
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

/// One `itemdt_<category>` member: a 12-byte head and 32-byte records.
fn item_member(items: &[(u16, u16, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((items.len() as u16).to_le_bytes());
    out.extend(0x8000u16.to_le_bytes());
    out.extend([0u8; 8]);
    for &(id, given, asked) in items {
        let mut record = [0u8; 32];
        record[0x18..0x1a].copy_from_slice(&(id as i16).to_le_bytes());
        record[0x1a..0x1c].copy_from_slice(&given.to_le_bytes());
        record[0x1c..0x1e].copy_from_slice(&asked.to_le_bytes());
        out.extend(record);
    }
    out
}

fn shop_services(party: &mut Party) -> Services {
    use dq9_formats::facility::shop::ShopTable;
    use dq9_formats::table::{Record, Value};
    let mut values = vec![Value::Int(32), Value::Int(3)];
    values.push(Value::Int(22000));
    for _ in 1..18 {
        values.push(Value::Int(0));
    }
    values.push(Value::Int(100));
    values.push(Value::Int(3));
    let table = encode_table(&[
        Record {
            kind: 0x66,
            values: vec![Value::Int(1)],
        },
        Record { kind: 0x67, values },
    ]);
    let member = item_member(&[(22000, 4, 0xFFFF)]);
    let prices = dq9_formats::facility::shop::ItemPrices::from_members(&[&member]).unwrap();
    let shops = ShopTable::parse(&table).unwrap();
    party.gold = 100;
    Services {
        shops: Some(shops),
        prices: Some(prices),
        shop_number: 32,
        shop_mode: ShopMode::Buy,
        screen: Screen::Shop,
        ..Services::default()
    }
}

#[test]
fn the_shop_buys_at_the_items_ask_word_and_takes_the_gold() {
    let mut party = party();
    let mut services = shop_services(&mut party);
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(party.bag.count(22000), 1);
    assert_eq!(party.gold, 92, "the herb asks twice the four it gives");
    assert_eq!(services.screen, Screen::Notice);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Shop);
}

#[test]
fn the_shop_sells_what_a_shop_gives_and_holds_the_gold() {
    let mut party = party();
    assert!(party.bag.add_copies(22000, 2));
    let mut services = shop_services(&mut party);
    services.side(true);
    services.cursor = 0;
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(party.bag.count(22000), 1);
    assert_eq!(party.gold, 104, "a herb gives four");
}

#[test]
fn an_inn_stay_pays_the_living_heals_them_and_sets_the_day() {
    use dq9_core::facility::inn::Choice;
    let mut party = party();
    let mut member = dq9_core::party::Character::hero();
    member.max_hp = 40;
    member.max_mp = 9;
    member.hp = 3;
    member.mp = 1;
    party.members[0] = member;
    let mut down = dq9_core::party::Character::hero();
    down.max_hp = 20;
    down.hp = 0;
    party.members.push(down);
    party.gold = 100;
    let mut messages = std::collections::BTreeMap::new();
    messages.insert(0, b"Stay Overnight".to_vec());
    messages.insert(1, b"Rest".to_vec());
    messages.insert(2, b"Cancel".to_vec());
    messages.insert(1010, b"Resting".to_vec());
    messages.insert(1011, b"Rested".to_vec());
    let mut services = Services {
        inn: Some(InnState {
            messages,
            per_head: 5,
            choices: vec![Choice::Stay, Choice::Rest, Choice::Cancel],
            labels: vec![
                b"Stay Overnight".to_vec(),
                b"Rest".to_vec(),
                b"Cancel".to_vec(),
            ],
        }),
        screen: Screen::Inn,
        ..Services::default()
    };
    let mut clock = DayClock::new(400.0, true);
    let mut rng = Random::new(1);
    // Stay is the first row by day.
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(party.gold, 95, "5 a living head, the dead need no bed");
    assert_eq!(party.members[0].hp, 40);
    assert_eq!(party.members[0].mp, 9);
    assert_eq!(party.members[1].hp, 0, "the dead are not raised");
    assert_eq!(clock.seconds(), 210.0);
    assert_eq!(clock.phase(), dq9_core::state::TimeOfDay::Day);
    assert_eq!(services.screen, Screen::Notice);
}

#[test]
fn a_church_resurrection_raises_the_dead_member_and_charges_its_level_price() {
    use dq9_core::facility::church::Service;
    let mut party = party();
    let mut member = dq9_core::party::Character::hero();
    member.vocation = 6;
    member.levels[6] = 10;
    member.max_hp = 40;
    member.hp = 0;
    party.members[0] = member;
    party.gold = 100;
    let mut messages = std::collections::BTreeMap::new();
    messages.insert(Service::Resurrection.label(), b"Resurrection".to_vec());
    messages.insert(1030, b"Whose soul?".to_vec());
    messages.insert(1031, b"Prayer".to_vec());
    let mut services = Services {
        church: Some(ChurchState {
            messages,
            services: vec![Service::Resurrection],
            cursor: 0,
            selected: None,
            cure: None,
            member: 0,
            confirm: 0,
        }),
        statuses: vec![dq9_core::facility::church::MemberStatus::default()],
        screen: Screen::Church,
        ..Services::default()
    };
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::ChurchMembers);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::ChurchConfirm);
    assert_eq!(
        services.church.as_ref().and_then(|church| church.cure),
        Some((Service::Resurrection, 0, 60)),
        "level 10 asks floor((100 + 20) / 20) * 10"
    );
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(party.members[0].hp, 40);
    assert_eq!(party.gold, 40);
    assert_eq!(services.screen, Screen::Notice);
}

/// Fingerprint of the hero's posed geometry, observed from the runtime the renderer draws.
#[cfg(not(target_arch = "wasm32"))]
fn hero_shape(app: &crate::app::App) -> (usize, u64) {
    let runtime = app.runtime.as_ref().expect("a map is loaded");
    let vertices = runtime.hero_vertices().expect("the hero is in the scene");
    let hash = vertices.iter().fold(0u64, |hash, vertex| {
        let mut next = hash;
        for value in vertex.position {
            next = next
                .wrapping_mul(0x100000001b3)
                .wrapping_add(u64::from(value.to_bits()));
        }
        next
    });
    (vertices.len(), hash)
}

/// The field state a same-map reload would reset: the map's own load journal, the per-map
/// talk counters, a global flag set after the load, the world generator and the hero's pose.
/// The visible gear must change without touching any of them.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn changing_gear_rebuilds_the_hero_without_reloading_the_field() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let options = crate::app::Options {
        map: Some("M01M05".into()),
        save: None,
        time: None,
        clock: None,
        layout: crate::screens::Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: 2,
            step: 1,
            substep: 1,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: false,
        battle: None,
        play: Default::default(),
    };
    let mut app = crate::app::App::new_headless(options, Some(rom.to_vec()));
    let map = app.current.as_ref().expect("a map").0.id;
    // Live field state that a reload would replace: a talk counter and a global flag set
    // after the map was entered.
    app.state.talk_counts.record(7);
    app.state.flags.insert(0x7f01);
    let counts = app.state.talk_counts.get(7);
    let flags = app.state.flags.clone();
    let steps = app.journal.steps.len();
    let loads = app
        .journal
        .steps
        .iter()
        .filter(|(_, step)| matches!(step, dq9_core::scenario::Step::Map { id, .. } if *id == map))
        .count();
    let random = app.world.random.state();
    let started = app.events.started().len();
    let pose = app.hero.expect("a hero").position();
    let yaw = app.hero.expect("a hero").yaw();
    let before = hero_shape(&app);

    // Change a visible piece through the real menu, then close it: Equip is the second row,
    // Torso the fourth slot, and Close the fifth menu row. The close calls `after_service`,
    // whose `look_dirty` path used to refresh through `load_map`.
    assert!(app.party.bag.add(13021), "a spare torso for the menu");
    assert!(app.open_field_menu());
    app.service_button(Button::Down);
    app.service_button(Button::Confirm);
    for _ in 0..3 {
        app.service_button(Button::Down);
    }
    app.service_button(Button::Confirm);
    app.service_button(Button::Confirm);
    app.service_button(Button::Cancel);
    // The equipment screen leaves its slot index as the menu cursor: Torso (3) → Close (4).
    app.service_button(Button::Down);
    app.service_button(Button::Confirm);
    assert!(!app.services.is_open(), "the menu closed");
    assert_eq!(
        app.party.members[0].equipment.torso,
        Some(13021),
        "the menu put the spare torso on"
    );

    assert_ne!(hero_shape(&app), before, "the drawn gear changed");
    assert_eq!(
        app.journal.steps.len(),
        steps,
        "no map or entry step was journaled"
    );
    assert_eq!(
        app.journal
            .steps
            .iter()
            .filter(
                |(_, step)| matches!(step, dq9_core::scenario::Step::Map { id, .. } if *id == map)
            )
            .count(),
        loads,
        "the map was not loaded again"
    );
    assert_eq!(app.state.talk_counts.get(7), counts, "talk counters kept");
    assert_eq!(app.state.flags, flags, "field flags kept");
    assert_eq!(app.world.random.state(), random, "generator untouched");
    assert_eq!(app.events.started().len(), started, "no event replayed");
    assert!(app.events.pending_start().is_none());
    let hero = app.hero.expect("a hero");
    assert_eq!(hero.position(), pose, "hero pose kept");
    assert_eq!(hero.yaw(), yaw, "hero facing kept");

    // Repeated alternating outfits: the visible geometry must follow each change while the
    // hero's model and instance slots and the scene's resource storage stay bounded. The old
    // rig is not left hidden beside the new one.
    let instance = app.hero_instance.expect("a hero instance");
    let hero_model = app.runtime.as_ref().unwrap().scene.instances[instance].model;
    let npc = app
        .runtime
        .as_ref()
        .unwrap()
        .npc_instance(7)
        .expect("the innkeeper is in the cast");
    let npc_world = app.runtime.as_ref().unwrap().scene.instances[npc].world;
    let other_texture_refs: Vec<Option<usize>> = app
        .runtime
        .as_ref()
        .unwrap()
        .scene
        .models
        .iter()
        .enumerate()
        .filter(|(model, _)| *model != hero_model)
        .flat_map(|(_, model)| model.materials.iter().map(|material| material.texture))
        .collect();
    let resource_counts = |app: &crate::app::App| {
        let scene = &app.runtime.as_ref().unwrap().scene;
        (
            scene.models.len(),
            scene.instances.len(),
            scene.textures.len(),
            scene
                .textures
                .iter()
                .map(|texture| texture.rgba.len())
                .sum::<usize>(),
        )
    };
    let sample = |app: &mut crate::app::App, torso: u16| {
        app.party.members[0].equipment.torso = Some(torso);
        app.refresh_hero_look();
        assert_eq!(
            app.hero_instance,
            Some(instance),
            "the hero instance is stable"
        );
        hero_shape(app)
    };
    let first = sample(&mut app, 13007);
    let first_storage = resource_counts(&app);
    let second = sample(&mut app, 13021);
    let second_storage = resource_counts(&app);
    sample(&mut app, 13007);
    sample(&mut app, 13021);
    let bounded = resource_counts(&app);
    let bytes_bound = first_storage.3.max(second_storage.3);
    assert_ne!(first, second, "the two outfits draw differently");
    for round in 0..20 {
        let torso = if round % 2 == 0 { 13007 } else { 13021 };
        let shape = sample(&mut app, torso);
        assert_eq!(
            shape,
            if torso == 13007 { first } else { second },
            "the same outfit draws the same geometry"
        );
        let storage = resource_counts(&app);
        assert_eq!(storage.0, bounded.0, "the scene model count stayed bounded");
        assert_eq!(
            storage.1, bounded.1,
            "the scene instance count stayed bounded"
        );
        assert_eq!(
            storage.2, bounded.2,
            "the scene texture count stayed bounded"
        );
        assert!(
            storage.3 <= bytes_bound,
            "texture storage {storage:?} stayed under {bytes_bound}"
        );
        let scene = &app.runtime.as_ref().unwrap().scene;
        assert_eq!(
            scene.instances[npc].world, npc_world,
            "the other actor kept its pose"
        );
        let refs: Vec<Option<usize>> = scene
            .models
            .iter()
            .enumerate()
            .filter(|(model, _)| *model != hero_model)
            .flat_map(|(_, model)| model.materials.iter().map(|material| material.texture))
            .collect();
        assert_eq!(refs, other_texture_refs, "other texture references kept");
    }

    // A weapon-only change is not drawn by the rig: no rebuild, no allocation.
    let shape = hero_shape(&app);
    let storage = resource_counts(&app);
    let pointer = app
        .runtime
        .as_ref()
        .unwrap()
        .hero_vertices()
        .unwrap()
        .as_ptr();
    app.party.members[0].equipment.weapon = Some(20005);
    app.refresh_hero_look();
    assert_eq!(hero_shape(&app), shape, "the drawn hero is unchanged");
    assert_eq!(resource_counts(&app), storage, "nothing was allocated");
    assert_eq!(
        app.runtime
            .as_ref()
            .unwrap()
            .hero_vertices()
            .unwrap()
            .as_ptr(),
        pointer,
        "the existing rig was kept"
    );

    // A failed replacement is transactional: the old rig, storage and instance survive.
    let shape = hero_shape(&app);
    let storage = resource_counts(&app);
    let look = app.look.clone();
    let runtime = app.runtime.as_mut().unwrap();
    assert!(
        runtime.replace_hero(&look, |_| None).is_err(),
        "a missing hero resource fails"
    );
    assert_eq!(hero_shape(&app), shape, "the old hero is still drawn");
    assert_eq!(resource_counts(&app), storage, "nothing was committed");
    assert_eq!(app.hero_instance, Some(instance));
    assert_eq!(
        app.journal.steps.len(),
        steps,
        "the repeated swaps did not reload or re-enter the field"
    );
    assert_eq!(app.state.talk_counts.get(7), counts, "talk counters kept");
    assert_eq!(app.state.flags, flags, "field flags kept");
    assert_eq!(app.world.random.state(), random, "generator untouched");
}

/// The night inn menu is Stay/Cancel; X on the paid notice must not return to a now-different
/// day menu where the displayed Cancel is Rest.
#[test]
fn the_night_paid_notice_cannot_charge_rest_from_a_stale_cancel_row() {
    use dq9_core::facility::inn::Choice;
    use dq9_core::state::TimeOfDay;
    let mut party = party();
    party.gold = 100;
    party.members[0].max_hp = 40;
    party.members[0].hp = 10;
    party.members[0].max_mp = 9;
    party.members[0].mp = 1;
    let mut messages = std::collections::BTreeMap::new();
    messages.insert(0, b"Stay Overnight".to_vec());
    messages.insert(1, b"Rest".to_vec());
    messages.insert(2, b"Cancel".to_vec());
    messages.insert(1010, b"Resting".to_vec());
    messages.insert(1011, b"Rested".to_vec());
    let mut services = Services {
        inn: Some(InnState {
            messages,
            per_head: 5,
            choices: vec![Choice::Stay, Choice::Cancel],
            labels: vec![b"Stay Overnight".to_vec(), b"Cancel".to_vec()],
        }),
        screen: Screen::Inn,
        ..Services::default()
    };
    let mut clock = DayClock::new(0.0, true);
    let mut rng = Random::new(1);
    assert_eq!(clock.phase(), TimeOfDay::Night);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(party.gold, 95, "5 for the one living member");
    assert_eq!(clock.seconds(), 210.0);
    assert_eq!(services.screen, Screen::Notice);
    assert_eq!(
        services.inn.as_ref().unwrap().choices,
        vec![Choice::Stay, Choice::Rest, Choice::Cancel],
        "the stored choices follow the clock"
    );
    assert_eq!(
        services.inn.as_ref().unwrap().labels.len(),
        services.inn.as_ref().unwrap().choices.len(),
        "labels and accepted rows stay together"
    );
    // X advances to the 1011 line, it does not re-open the menu.
    services.press(Button::Cancel, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Notice);
    assert_eq!(party.gold, 95, "no second charge");
    assert_eq!(clock.seconds(), 210.0, "the clock is not moved back");
    services.press(Button::Cancel, &mut party, &mut rng, &mut clock);
    assert!(!services.is_open());
    assert_eq!(party.gold, 95);
}

/// The inn file must carry the retail labels and price line; an explicit 0 price (inn 15)
/// is still valid. Missing or empty messages are an error, not a free inn with debug labels.
#[test]
fn a_malformed_inn_file_is_refused_but_a_free_price_is_not() {
    let mut messages = std::collections::BTreeMap::new();
    for (line, text) in [(0u16, "Stay"), (1, "Rest"), (2, "Cancel"), (10000, "0")] {
        messages.insert(line, text.as_bytes().to_vec());
    }
    assert!(
        validate_inn_messages(&messages).is_ok(),
        "the free inn passes"
    );
    let mut missing = messages.clone();
    missing.remove(&10000);
    assert!(validate_inn_messages(&missing).is_err());
    let mut empty = messages.clone();
    empty.insert(0, Vec::new());
    assert!(validate_inn_messages(&empty).is_err());
}

/// Divination must show its notice, return to the service row it was chosen from, and name
/// the member and vocation when the church file has the label.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn divination_shows_its_result_with_the_member_and_vocation() {
    use dq9_core::facility::church::Service;
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let data = crate::battle::data::Data::load(rom).expect("ROM tables");
    let mut party = party();
    let mut member = dq9_core::party::Character::hero();
    member.vocation = 6;
    member.levels[6] = 99;
    member.max_hp = 40;
    member.hp = 40;
    party.members[0] = member;
    let mut messages = std::collections::BTreeMap::new();
    messages.insert(36, b"Minstrel".to_vec());
    messages.insert(1022, b"<TARGET> has mastered <str_2>.".to_vec());
    let mut services = Services {
        data: Some(Box::new(data)),
        church: Some(ChurchState {
            messages,
            services: vec![Service::Divination],
            cursor: 3,
            selected: Some(Service::Divination),
            cure: None,
            member: 0,
            confirm: 0,
        }),
        statuses: vec![dq9_core::facility::church::MemberStatus::default()],
        screen: Screen::ChurchMembers,
        ..Services::default()
    };
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Notice, "the result is drawn");
    let notice = services.notice.clone().expect("a divination notice");
    assert_eq!(notice.target, Some(services.name(0)));
    assert_eq!(notice.vocation.as_deref(), Some(&b"Minstrel"[..]));
    let text = services.resolved_notice_text(&notice);
    let text = String::from_utf8_lossy(&text);
    assert!(!text.contains("[Target0]"), "{text}");
    assert!(!text.contains("[STR_2]"), "{text}");
    assert!(text.contains("Member 1"), "{text}");
    assert!(text.contains("Minstrel"), "{text}");
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Church);
    assert_eq!(services.cursor, 3, "back on the Divination row");

    // The Guardian's message 30 is in no European file: the tag stays unknown rather than
    // borrowing another vocation's label.
    let mut guardian = party;
    guardian.members[0].vocation = 0;
    guardian.members[0].levels[0] = 99;
    services.screen = Screen::ChurchMembers;
    services.cursor = 0;
    services.press(Button::Confirm, &mut guardian, &mut rng, &mut clock);
    let notice = services.notice.clone().expect("a guardian divination");
    assert_eq!(notice.vocation, None, "the Guardian label is not known");
    let text = String::from_utf8_lossy(&services.resolved_notice_text(&notice)).into_owned();
    assert!(text.contains("<str_2>"), "{text}");
}

/// A cure refused because the member needs nothing must still resolve `<TARGET>`.
#[test]
fn a_not_needed_cure_names_the_member() {
    use dq9_core::facility::church::Service;
    let mut party = party();
    party.members[0].max_hp = 40;
    party.members[0].hp = 40;
    let mut messages = std::collections::BTreeMap::new();
    messages.insert(1032, b"I can see that <TARGET> is alive and well.".to_vec());
    let mut services = Services {
        church: Some(ChurchState {
            messages,
            services: vec![Service::Resurrection],
            cursor: 2,
            selected: Some(Service::Resurrection),
            cure: None,
            member: 0,
            confirm: 0,
        }),
        statuses: vec![dq9_core::facility::church::MemberStatus::default()],
        screen: Screen::ChurchMembers,
        ..Services::default()
    };
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    let notice = services.notice.clone().expect("a refusal notice");
    assert_eq!(notice.target, Some(services.name(0)));
    let text = String::from_utf8_lossy(&services.resolved_notice_text(&notice)).into_owned();
    assert!(!text.contains("[Target0]"), "{text}");
    assert!(text.contains("Member 1"), "{text}");
}

/// Multiheal (action 34, reach All) heals every living member once, skips the dead and pays
/// its 16 MP once.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn multiheal_heals_all_living_members_for_one_cost() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let data = crate::battle::data::Data::load(rom).expect("ROM tables");
    let mut party = party();
    let mut leader = dq9_core::party::Character::hero();
    leader.vocation = 10;
    leader.levels[10] = 24;
    leader.max_hp = 60;
    leader.hp = 20;
    leader.max_mp = 100;
    leader.mp = 100;
    let mut second = dq9_core::party::Character::hero();
    second.max_hp = 30;
    second.hp = 10;
    let mut dead = dq9_core::party::Character::hero();
    dead.max_hp = 20;
    dead.hp = 0;
    party.members = vec![leader, second, dead];
    let mut services = Services {
        data: Some(Box::new(data)),
        screen: Screen::Spells,
        ..Services::default()
    };
    services.apply_levels(&mut party);
    let spells = services.spells(&party);
    let at = spells
        .iter()
        .position(|(action, _, _)| *action == 34)
        .expect("Multiheal is learned at vocation 10 level 24");
    assert!(spells[at].2, "the spell is usable");
    services.cursor = at;
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert!(party.members[0].hp > 20, "the leader is healed");
    assert!(party.members[1].hp > 10, "the second member is healed");
    assert_eq!(party.members[2].hp, 0, "the dead stay dead");
    assert_eq!(party.members[0].mp, 84, "16 MP once");
    assert_eq!(services.screen, Screen::Spells, "no single-target selector");
}

/// A dead leader cannot use an item or cast, and a dead target is refused visibly with the
/// pending action kept and nothing spent.
#[test]
fn dead_actors_and_targets_are_refused_visibly() {
    let mut down = party();
    down.members[0].max_hp = 40;
    down.members[0].hp = 0;
    down.members[0].mp = 10;
    assert!(down.bag.add(22000));
    let mut services = Services {
        screen: Screen::Items,
        ..Services::default()
    };
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut down, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Notice, "the refusal is visible");
    assert_eq!(down.bag.count(22000), 1, "nothing was spent");

    // A living leader, a dead second member: the target cursor skips it, and a forced
    // confirm keeps the pending spell with no MP or HP change.
    let mut party = party();
    party.members[0].max_hp = 40;
    party.members[0].hp = 20;
    party.members[0].max_mp = 20;
    party.members[0].mp = 20;
    let mut dead = dq9_core::party::Character::hero();
    dead.max_hp = 30;
    dead.hp = 0;
    party.members.push(dead);
    let mut services = Services {
        screen: Screen::Targets,
        target_return: Screen::Spells,
        pending: Some(Pending::Spell {
            action: 30,
            cost: 2,
        }),
        cursor: 1,
        ..Services::default()
    };
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Notice, "the refusal is visible");
    assert!(services.pending.is_some(), "the pending action is kept");
    assert_eq!(party.members[0].mp, 20, "no MP spent");
    assert_eq!(party.members[1].hp, 0, "the dead target stays dead");
}

/// Fullheal (action 33, learned by vocation 2 at level 47) heals the chosen member to its
/// maximum through the field menu, paying its 24 MP once.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fullheal_heals_to_the_maximum_from_the_field_menu() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let data = crate::battle::data::Data::load(rom).expect("ROM tables");
    let mut party = party();
    let mut leader = dq9_core::party::Character::hero();
    leader.vocation = 2;
    leader.levels[2] = 47;
    leader.max_hp = 80;
    leader.hp = 20;
    leader.max_mp = 100;
    leader.mp = 100;
    party.members[0] = leader;
    let mut services = Services {
        data: Some(Box::new(data)),
        screen: Screen::Spells,
        ..Services::default()
    };
    services.apply_levels(&mut party);
    let spells = services.spells(&party);
    let at = spells
        .iter()
        .position(|(action, _, _)| *action == 33)
        .expect("Fullheal is learned at vocation 2 level 47");
    assert!(spells[at].2);
    services.cursor = at;
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(
        services.screen,
        Screen::Targets,
        "Fullheal aims at one member"
    );
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(party.members[0].hp, 80, "Fullheal fills the target");
    assert_eq!(party.members[0].mp, 76, "24 MP once");
}

/// Equipping a different weapon while the worn piece already has a 99 stack in the bag: the
/// old equipment stays worn and the chosen copy stays in the bag, through the real Equip
/// screen rather than `transfer::equip`.
#[test]
fn equipping_into_a_full_old_stack_keeps_both_pieces() {
    const OLD: u16 = 20005;
    const NEW: u16 = 20004;
    let mut party = party();
    party.bag = Bag::empty(BAG_CAPACITY);
    assert!(party.bag.add(NEW));
    assert!(party.bag.add_copies(OLD, 99));
    party.members[0].equipment.weapon = Some(OLD);
    party.members[0].held_items[2] = Some(22019);
    party.members[0].max_mp = 20;
    party.members[0].mp = 7;
    party.gold = 321;
    let mut services = Services {
        screen: Screen::Menu,
        cursor: 1,
        ..Services::default()
    };
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Equip);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(
        services.screen,
        Screen::EquipChoice(dq9_formats::save::EquipSlot::Weapon)
    );
    assert_eq!(services.cursor, 0, "the new weapon is the first row");
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);

    assert_eq!(services.screen, Screen::Equip);
    assert_eq!(
        party.members[0].equipment.weapon,
        Some(OLD),
        "the worn sword stays worn"
    );
    assert_eq!(party.bag.count(OLD), 99);
    assert_eq!(party.bag.count(NEW), 1, "the new sword stays in the bag");
    assert_eq!(party.bag.count(OLD) + party.bag.count(NEW), 100);
    assert_eq!(
        party
            .bag
            .slots()
            .iter()
            .filter(|s| s.item.is_some())
            .count(),
        2
    );
    assert_eq!(party.members[0].held_items[2], Some(22019));
    assert_eq!((party.members[0].mp, party.gold), (7, 321));
    assert!(!services.look_dirty(), "no equip, no hero rebuild");
}

/// A 99 stack of the chosen weapon plus a single copy in an earlier slot; equipping the single
/// copy empties its slot, which then stores the old piece. A full bag stays full, and both
/// items are conserved.
#[test]
fn equipping_a_single_copy_beside_a_99_stack_reuses_the_freed_slot() {
    const OLD: u16 = 20005;
    const NEW: u16 = 20004;
    const OTHER: u16 = 13007;
    let mut party = party_with_bag(&[(NEW, 1), (NEW, 99), (OTHER, 1)]);
    assert_eq!(party.bag.capacity(), 3);
    assert_eq!(party.bag.count(NEW), 100, "a 99 stack plus the copy");
    assert_eq!(
        party
            .bag
            .slots()
            .iter()
            .filter(|s| s.item.is_none())
            .count(),
        0,
        "the bag starts full"
    );
    party.members[0].equipment.weapon = Some(OLD);
    party.members[0].held_items[4] = Some(22019);
    party.members[0].max_mp = 20;
    party.members[0].mp = 7;
    party.gold = 321;
    let mut services = Services {
        screen: Screen::Menu,
        cursor: 1,
        ..Services::default()
    };
    let mut rng = Random::new(1);
    let mut clock = DayClock::new(0.0, true);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(services.screen, Screen::Equip);
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);
    assert_eq!(
        services.screen,
        Screen::EquipChoice(dq9_formats::save::EquipSlot::Weapon)
    );
    assert_eq!(services.cursor, 0, "the duplicate weapon is one row");
    services.press(Button::Confirm, &mut party, &mut rng, &mut clock);

    assert_eq!(services.screen, Screen::Equip);
    assert_eq!(party.members[0].equipment.weapon, Some(NEW));
    assert_eq!(party.bag.count(NEW), 99, "the 99 stack is untouched");
    assert_eq!(
        party.bag.count(OLD),
        1,
        "the old piece is in the freed slot"
    );
    assert_eq!(party.bag.count(OTHER), 1);
    assert_eq!(
        party.bag.slots()[0].item,
        Some(OLD),
        "the emptied first slot took the old piece"
    );
    assert_eq!(
        party
            .bag
            .slots()
            .iter()
            .filter(|s| s.item.is_none())
            .count(),
        0,
        "the full bag stayed full"
    );
    assert_eq!(
        party.bag.count(NEW) + u32::from(party.members[0].equipment.weapon == Some(NEW)),
        100
    );
    assert_eq!(
        party.bag.count(OLD) + u32::from(party.members[0].equipment.weapon == Some(OLD)),
        1
    );
    assert_eq!(party.members[0].held_items[4], Some(22019));
    assert_eq!((party.members[0].mp, party.gold), (7, 321));
    assert!(
        services.look_dirty(),
        "the successful equip rebuilds the hero"
    );
}
