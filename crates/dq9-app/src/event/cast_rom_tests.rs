//! The map's cast while a scene plays and after it (real-ROM regressions; they return early
//! without a ROM): which NPCs the scene keeps off the screen, and where an NPC it moved stands
//! when it is over. `docs/re/characters.md`, § "The map's cast during a scene".

use super::*;
use crate::load::{self, LoadedMap};
use crate::report;
use crate::talk::{Button, Talk};
use dq9_core::field::initial_state;
use dq9_formats::cast::Story;
use dq9_formats::hero::HeroLook;

/// A scene being played on its map by the fixed steps of the application.
struct Run<'a> {
    rom: &'a [u8],
    state: State,
    loaded: LoadedMap,
    hero: Hero,
    camera: Camera,
    talk: Talk,
    player: EventPlayer,
    tick: u32,
    /// The next event of a chain that is on another map: the playback stops there, as
    /// `report::play` does, and the map is left with the scene's state.
    chained: Option<u16>,
}

impl<'a> Run<'a> {
    /// The map of `event` at `story` with the event queued, the cast as the map load built it.
    fn start(rom: &'a [u8], event: u16, story: &Story) -> Self {
        let place = crate::event::place(rom, event).expect("event record");
        let loaded = load::load(
            rom,
            &place.map.to_string(),
            0,
            story,
            crate::event::spawn(&place),
            None,
            &HeroLook::new_game(),
        )
        .expect("map of the event");
        let mut talk = Talk::default();
        talk.set_rom(rom);
        let mut player = EventPlayer::new();
        player.queue(event);
        Self {
            rom,
            state: initial_state(story),
            hero: loaded.hero,
            loaded,
            camera: Camera::default(),
            talk,
            player,
            tick: 0,
            chained: None,
        }
    }

    /// One fixed step; the action key is pressed every 40 ticks to read the messages.
    fn step(&mut self) {
        if self.tick.is_multiple_of(40) {
            self.talk.press(Button::Confirm);
        }
        self.chained = report::step_frame(
            self.rom,
            &mut self.player,
            &mut self.loaded,
            &mut self.hero,
            &mut self.camera,
            &mut self.talk,
            &self.state,
            true,
        )
        .expect("scene animation");
        self.tick += 1;
    }

    /// The scene has played to its end, or its chain goes on another map.
    fn over(&self) -> bool {
        self.chained.is_some()
            || (self.tick > 10 && !self.player.is_active() && self.player.pending_start().is_none())
    }

    /// The ids of the map's NPCs that an image shows now, by instance.
    fn drawn_npcs(&self) -> Vec<u16> {
        self.loaded
            .runtime
            .scene
            .instances
            .iter()
            .filter(|i| i.block == usize::MAX && i.instance != usize::MAX && i.drawn())
            .filter_map(|i| u16::try_from(i.instance).ok())
            .collect()
    }

    /// How many of the models the script loaded an image shows now (the scene's own
    /// characters).
    fn drawn_actors(&self) -> usize {
        self.player
            .bindings
            .actors
            .values()
            .filter_map(|&actor| self.loaded.runtime.actor_instance(actor))
            .filter(|&instance| self.loaded.runtime.scene.instances[instance].drawn())
            .count()
    }

    /// The world position of map NPC `id` (a sprite's instance is a translation).
    fn npc_position(&self, id: u16) -> Vec3 {
        let runtime = &self.loaded.runtime;
        let instance = runtime.npc_instance(id).expect("NPC of the cast");
        runtime.scene.instances[instance].world.w_axis.truncate()
    }
}

fn story(chapter: u8, step: u8, substep: u8, daytime: bool) -> Story {
    Story {
        chapter,
        step,
        substep,
        daytime,
        ..Story::default()
    }
}

/// Plays `run` until its scene is over, calling `each` after every step that leaves it
/// running.
fn play(run: &mut Run, mut each: impl FnMut(&Run)) {
    loop {
        assert!(run.tick < 6_000, "the scene does not end");
        run.step();
        if run.over() {
            break;
        }
        each(run);
    }
}

/// The Observatory scenes of the prologue (`21522` at the entrance `X01M01`; `21591`, `21596`
/// and `21597` in the hall `X01M05`) call native 568 with `0x34` in their prepare entry. Bit
/// `0x20` sets bit 1 of `NpcTable+0x98` when the scene is built (`ov_001::021539d8`), and the
/// NPC table's draw (`0x0203de94`) then skips every entry without flag `0x10000`, which only
/// the entries a script binds get (cast modes 3, 4, 5). These scripts bind none: the sprite
/// NPCs of the two rooms (fifteen in the hall, nine at the entrance) are not drawn while they
/// play (user report: two winged NPCs behind a table, behind Aquila, that the game does not
/// show) and stand again when they are over. The rooms' casts are the same by day and by
/// night (`dq9re cast X01M05 --chapter 1`), so the time of day is not what hides them; the
/// record of `21522` also tears the cast down (value 21 = 4), the others have no such bit.
#[test]
fn the_observatory_scenes_draw_no_map_npc_behind_their_actors() {
    let Some(rom) = testrom::load() else { return };
    for (event, flags) in [(21522u16, 0x4u16), (21591, 0), (21596, 0), (21597, 0)] {
        let place = crate::event::place(rom, event).expect("event record");
        assert_eq!(place.flags, flags, "record value 21 of event {event}");
    }
    for daytime in [false, true] {
        for (event, npcs) in [(21522u16, 9usize), (21591, 15), (21596, 15), (21597, 15)] {
            let mut run = Run::start(rom, event, &story(1, 1, 1, daytime));
            let room = run.drawn_npcs();
            assert_eq!(room.len(), npcs, "event {event}: the room's cast at 1.1.1");
            let mut checked = 0;
            play(&mut run, |run| {
                // The scene is built after its prepare entry (one update, two ticks).
                if run.tick > 8 {
                    let drawn = run.drawn_npcs();
                    assert!(
                        drawn.is_empty(),
                        "event {event} (day {daytime}) tick {}: map NPCs drawn {drawn:?}",
                        run.tick
                    );
                    checked += 1;
                }
            });
            assert!(checked > 100, "event {event} checked {checked} ticks only");
            // `21522` chains to `21596`, which is in the hall: that scene is another map's.
            if run.chained.is_none() {
                assert_eq!(
                    run.drawn_npcs(),
                    room,
                    "event {event}: the room's NPCs are back when the scene is over"
                );
            }
        }
    }
}

/// The arrival scene of Angel Falls (`21592`, story 1.2, the camera looks down on the village
/// from high above) has the same flag 568(`0x34`) and binds no NPC: the village's sprite and
/// model NPCs are not drawn, only the two characters the script loads (Ivor and the second
/// speaker, native 200: Chara objects, not entries of the NPC table). The map's NPCs are
/// drawn again at the end.
#[test]
fn the_angel_falls_arrival_draws_the_two_speakers_and_no_village_npc() {
    let Some(rom) = testrom::load() else { return };
    let place = crate::event::place(rom, 21592).expect("event record");
    assert_eq!(place.flags, 0, "no teardown bit in the record of 21592");
    for daytime in [false, true] {
        let mut run = Run::start(rom, 21592, &story(1, 2, 1, daytime));
        let village = run.drawn_npcs();
        assert_eq!(village.len(), 11, "the village's cast at 1.2.1");
        let mut checked = 0;
        let mut speakers = 0;
        play(&mut run, |run| {
            if run.tick > 8 {
                let drawn = run.drawn_npcs();
                assert!(
                    drawn.is_empty(),
                    "tick {}: village NPCs drawn {drawn:?}",
                    run.tick
                );
                checked += 1;
                speakers = speakers.max(run.drawn_actors());
            }
        });
        assert!(checked > 500, "{checked} ticks checked");
        assert!(
            speakers >= 2,
            "the scene's own speakers are drawn: {speakers}"
        );
        assert_eq!(
            run.drawn_npcs(),
            village,
            "the village is back when the scene is over"
        );
    }
}

/// Native 568's `0x20` hides the map's NPCs the script does not bind; the ones it does bind
/// (566 modes 4 and 5) carry flag `0x10000` and stay drawn. `14760` (map `5704` at 14.6.2,
/// mask `0x34`) binds NPCs 203, 230 and 231 of the seven the map has: while it plays the other
/// four are not drawn and the bound ones are, as far as the script shows them (it hides 203
/// until its last moments, which a bound NPC allows, a slot being the script's to hide).
#[test]
fn a_scene_keeps_drawn_the_map_npcs_it_binds() {
    let Some(rom) = testrom::load() else { return };
    let mut run = Run::start(rom, 14760, &story(14, 6, 2, true));
    let map = run.drawn_npcs();
    assert_eq!(
        map,
        [231, 230, 229, 223, 222, 203, 201],
        "the map's cast at 14.6.2"
    );
    let bound = [203, 230, 231];
    let mut bound_seen = BTreeSet::new();
    play(&mut run, |run| {
        if run.tick > 8 {
            for id in run.drawn_npcs() {
                assert!(
                    bound.contains(&id),
                    "tick {}: NPC {id} is not bound",
                    run.tick
                );
                bound_seen.insert(id);
            }
        }
    });
    assert_eq!(
        bound_seen.into_iter().collect::<Vec<_>>(),
        bound,
        "every bound NPC is drawn at some time of the scene"
    );
    assert_eq!(run.drawn_npcs(), map, "the four others are back at the end");
}

/// The record of `3090` (value 21 = `0x141`, bit 0) makes the event start tear the cast down
/// (`ov_001::02154f24` → `ov_017::021a316c`, which also empties the NPC table): the scene
/// plays on an empty table and the cast is built again at its end. Its mask (`0x14`) has no
/// `0x20`, so the teardown alone keeps the map's NPCs away.
#[test]
fn a_scene_that_sets_the_cast_aside_draws_no_map_npc() {
    let Some(rom) = testrom::load() else { return };
    let mut run = Run::start(rom, 3090, &story(3, 3, 1, true));
    let before = run.drawn_npcs();
    assert!(before.len() >= 3, "the map has NPCs at 3.3.1: {before:?}");
    let mut checked = 0;
    play(&mut run, |run| {
        if run.tick > 8 {
            let drawn = run.drawn_npcs();
            assert!(drawn.is_empty(), "tick {}: {drawn:?}", run.tick);
            checked += 1;
        }
    });
    assert!(checked > 50);
    assert_eq!(run.drawn_npcs(), before, "the NPCs are back");
}

/// Event `1170` (`X01M01`, the way to Yggdrasil): the guard NPC 127 (cast mode 5) walks
/// aside, along a path to `(3.72, 11.25, 16.87)`, to let the hero pass. The scene binds his
/// object, so the scene moves the NPC itself: nothing resets it at the end
/// (`ov_001::02154974` only clears his flag `0x10000`; the record has no teardown bit and the
/// trigger no `0x8d`), and the placement script puts him at that same spot from 1.1.2, where the
/// end of the event advances the story. User report: he walked back to his place.
#[test]
fn an_npc_the_scene_moves_aside_stays_aside() {
    let Some(rom) = testrom::load() else { return };
    let mut run = Run::start(rom, 1170, &story(1, 1, 1, false));
    let home = run.npc_position(127);
    assert!(
        home.distance(Vec3::new(0.68, 11.36, 17.56)) < 0.01,
        "the guard's place at 1.1.1: {home}"
    );
    let aside = Vec3::new(3.72, 11.25, 16.87);
    let mut moved = false;
    play(&mut run, |run| {
        moved |= run.npc_position(127).distance(aside) < 0.01;
    });
    assert!(moved, "the scene walks the guard to {aside}");
    let after = run.npc_position(127);
    assert!(
        after.distance(aside) < 0.01,
        "the guard stands at {after} when the scene is over: the scene left him at {aside}, \
         his place was {home}"
    );
}

/// A sprite NPC faces the angle its sheet keeps (`set_npc_angle`), which its instance matrix
/// does not carry. The scene's node for the NPC started from the matrix, at angle 0, and
/// `present` wrote it back every tick: after any scene the NPCs it left alone faced down.
/// `1170` leaves the village's guards alone, and only 127 turns.
#[test]
fn the_sprite_npcs_a_scene_leaves_alone_keep_the_angle_they_face() {
    let Some(rom) = testrom::load() else { return };
    let mut run = Run::start(rom, 1170, &story(1, 1, 1, false));
    let angles = |run: &Run| -> Vec<(u16, Option<f32>)> {
        run.drawn_npcs()
            .into_iter()
            .filter(|&id| id != 127)
            .map(|id| (id, run.loaded.runtime.npc_sprite_angle(id)))
            .collect()
    };
    let before = angles(&run);
    assert!(
        before
            .iter()
            .any(|&(_, angle)| angle.is_some_and(|angle| angle.abs() > 0.1)),
        "some NPC of the map faces another way than down: {before:?}"
    );
    play(&mut run, |_| {});
    assert_eq!(
        angles(&run),
        before,
        "the NPCs the scene did not move or turn face as before"
    );
}
