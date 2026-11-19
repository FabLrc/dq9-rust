//! Helpers shared by the application's tests.

use crate::app::App;
use crate::app::Options;
use crate::app::encounter::BattleChange;
use crate::app::fight::BattlePlay;
use crate::battle;
use crate::screens::Layout;
use crate::talk::Button;
use dq9_core::scenario::Step;

/// Start-up options of a ROM-backed regression on an explicit map and story point, with
/// no save and no pinned time (`--time`/`--clock` absent: the field may set the night).
pub(super) fn map_options(map: &str, story: (u8, u8, u8, bool)) -> Options {
    Options {
        map: Some(map.into()),
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: story.0,
            step: story.1,
            substep: story.2,
            daytime: story.3,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: false,
        battle: None,
        play: Default::default(),
    }
}

/// The window's `Entering` phase one fixed step before the scene is built: the swirl's clock
/// has stopped, only its last tick of black cover is missing.
pub(super) fn entering_at_the_end(request: crate::battle::Resolved) -> BattleChange {
    let mut swirl = battle::transition::Swirl::new();
    for _ in 0..battle::transition::entry_ticks() - 1 {
        swirl.step();
    }
    BattleChange::Entering {
        swirl,
        request,
        auto_leave: false,
    }
}

/// Number of `Step::Map` entries for `map` in the journal.
pub(super) fn map_loads(app: &App, map: u16) -> usize {
    app.journal
        .steps
        .iter()
        .filter(|(_, step)| matches!(step, Step::Map { id, .. } if *id == map))
        .count()
}

/// Plays the pending event until it ends, pressing the action key to read the messages.
pub(super) fn play_pending_event(app: &mut App, event: u16) {
    let mut played = false;
    for frame in 0..60_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        played |= app.events.started().contains(&event);
        if played && !app.events.is_active() && app.events.pending_start().is_none() {
            return;
        }
    }
    panic!(
        "event {event} did not end: started {:?}, current {:?}",
        app.events.started(),
        app.events.current()
    );
}

/// A headless run that plays its battles with the engine, the world's generators on `seed`.
pub(super) fn fighting(mut options: Options, seed: u64) -> Options {
    options.play = BattlePlay {
        fight: true,
        seed: Some(seed),
    };
    options
}

/// Options for the roaming-monster tests: a field map, day, battles played by the engine.
pub(super) fn roaming_options(map: &str) -> Options {
    let mut options = map_options(map, (1, 1, 1, true));
    options.play = BattlePlay {
        fight: true,
        seed: Some(7),
    };
    options
}

/// A party that wins the level-1 monsters of F01 whatever the company draw: the level tables
/// are skipped, the attributes are low enough that the monsters attack (a stronger party makes
/// them flee and pay nothing) and the HP survives every round.
pub(super) fn a_winning_party(app: &mut App) {
    app.party.members[0].stats.strength = 20;
    app.party.members[0].stats.resilience = 20;
    app.party.members[0].max_hp = 9999;
    app.party.members[0].hp = 9999;
    app.party_ready = true;
}
