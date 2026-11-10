//! The played fight in the application: the party, the world's generators, the fight state
//! machine driven by the window or by the headless policy, what it does to the scene, and
//! what its end does to the story (`docs/app.md`, § "Battle").
//!
//! Windows and the Web always fight; a headless run keeps the J4 stand-in (battle 26 "won at
//! once", the recorded traces' timeline) unless `--fight` asks for the engine.

use dq9_core::battle::{BattleResult, Surprise};
use dq9_core::party::Party;
use dq9_core::scenario::Step;

use super::{App, status};
use crate::battle::data::Data;
use crate::battle::fight::{Fight, Outcome, Pad, Setup, World};
use crate::battle::{self, BattleScene};

/// Where a new game's party comes round after a wipe-out: Angel Falls' church (`GameState+
/// 0x5698` at a new game, wiki *Travel*, "A wipe-out"). A save's revival map (a church saved
/// in, Stornway's) is not read **[hypothesis]**.
pub const NEW_GAME_REVIVAL_MAP: u16 = 1106;

/// How the last fight went, for the report and the tests (the Web has neither).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone)]
pub struct FightSummary {
    /// The scripted battle's number, `None` for an encounter or an explicit stage.
    pub number: Option<u16>,
    pub result: BattleResult,
    pub rounds: u32,
    pub experience: u32,
    pub gold: u32,
    pub drops: Vec<u16>,
    /// Every window shown, in order (lines joined by `\n`).
    pub log: Vec<String>,
}

/// How battles are played: `--fight` and `--seed`.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattlePlay {
    /// Play with the engine in a headless run (the window and the Web always do).
    pub fight: bool,
    /// Seed of the world's generators; the day clock and the step count seed them otherwise.
    pub seed: Option<u64>,
}

impl App {
    /// Seeds the world's generators at the first battle: `--seed`, else the game's clock and
    /// the fixed-step count at entry.
    pub(super) fn ensure_world(&mut self) {
        if self.world_seeded {
            return;
        }
        let seed = self.options.play.seed.unwrap_or_else(|| {
            (self.tick << 24) ^ u64::from(self.day.seconds().to_bits()) ^ 0x9e37_79b9
        });
        self.world = World::seeded(seed);
        self.world_seeded = true;
        status(&format!("battle: world generators seeded with {seed}"));
    }

    /// The party's names as the font draws them: the protagonist's, then the other members'
    /// saved glyphs.
    fn member_names(&self) -> Vec<Vec<u8>> {
        let hero = self.talk.hero_name();
        self.party
            .members
            .iter()
            .enumerate()
            .map(|(i, member)| {
                if i == 0 {
                    return hero.clone();
                }
                self.talk
                    .with_ui(|_, font| {
                        let mut name = Vec::new();
                        for &index in member.name().iter() {
                            match font.glyphs.get(usize::from(index)) {
                                Some(glyph) => name.extend_from_slice(glyph.text),
                                None => name.push(b'?'),
                            }
                        }
                        name
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Builds the fight of `resolved` against the party: the tables from the ROM, the
    /// monsters' HP drawn on the world's generator unless the battle is a set one.
    pub(super) fn make_fight(&mut self, resolved: &battle::Resolved) -> Option<Box<Fight>> {
        self.ensure_world();
        let rom = self.rom.as_deref()?;
        let data = match Data::load(rom) {
            Ok(data) => data,
            Err(e) => {
                status(&format!("battle: {e:#}"));
                return None;
            }
        };
        if !self.party_ready {
            self.party.apply_level_tables(&data.levels);
            self.party_ready = true;
        }
        let setup = Setup {
            groups: resolved.monsters.clone(),
            set_battle: resolved.number.is_some(),
            surprise: resolved.surprise,
            names: self.member_names(),
        };
        // The hero moves by the motion set of the weapon it carries (`0x02072c9c`): the
        // party's, or bare-handed when it has none.
        let weapon = self
            .party
            .members
            .first()
            .and_then(|member| member.equipment.weapon);
        if let Some(scene) = self.battle.as_mut()
            && let Err(e) = scene.set_hero_weapon(rom, weapon)
        {
            status(&format!("battle motion: hero weapon: {e:#}"));
        }
        match Fight::new(data, &self.party, &setup, &mut self.world) {
            Ok(fight) => Some(Box::new(fight)),
            Err(e) => {
                status(&format!("battle: {e:#}"));
                None
            }
        }
    }

    /// A headless `--fight`: the automatic policy plays the battle to its end in the same
    /// fixed step (no step is spent on a presentation) and the outcome settles the story.
    pub(super) fn play_headless_fight(&mut self, resolved: &battle::Resolved) {
        let Some(mut fight) = self.make_fight(resolved) else {
            self.settle_battle(resolved, None);
            return;
        };
        let outcome = match fight.play_out(&mut self.world) {
            Ok(outcome) => outcome.clone(),
            Err(e) => {
                status(&format!("battle: {e:#}"));
                self.settle_battle(resolved, None);
                return;
            }
        };
        self.last_fight = Some(summary(resolved.number, &outcome, fight.log()));
        self.settle_battle(resolved, Some(outcome));
    }

    /// One fixed step of the fight in play: the pad of the window, the scene effects, the
    /// outcome once its last window has closed. While the scene presents the previous command
    /// the window does not advance and the pad is ignored (`battle::advance_fight`).
    pub(super) fn drive_fight(&mut self) -> Option<Outcome> {
        let pad = std::mem::take(&mut self.battle_pad);
        let fight = self.fight.as_mut()?;
        let joined = match &mut self.battle {
            Some(scene) => {
                battle::advance_fight(fight, scene, &mut self.world, pad, self.rom.as_deref()).1
            }
            None => {
                fight.step(pad, &mut self.world);
                fight.take_cues();
                false
            }
        };
        let outcome = if fight.is_done() {
            let outcome = fight.take_outcome();
            if let Some(outcome) = &outcome {
                self.last_fight = Some(summary(None, outcome, fight.log()));
            }
            outcome
        } else {
            None
        };
        if outcome.is_some() {
            self.fight = None;
            if let Some(scene) = &mut self.battle {
                // The fight is over: nothing is left displaced or half-way through a motion.
                scene.clear_gestures();
            }
        }
        if joined {
            // The renderer is built from the scene's instances: build it again.
            self.upload();
        }
        outcome
    }

    /// A wandering monster's contact encounter: `groups`
    /// lists `(monster number, count)`, `stage` the ground's battle stage (0 for the
    /// default), `opening` who has the jump. The monsters' HP are drawn and the party may
    /// flee. Settled with no trigger (`BattleTask` has no battle number to report).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn start_encounter(
        &mut self,
        groups: Vec<(u16, u16)>,
        stage: u16,
        opening: Surprise,
    ) {
        self.start_battle(
            battle::Request {
                record: None,
                stage,
                monsters: groups,
                track: None,
                surprise: Some(opening),
            },
            false,
            false,
        );
    }

    /// The end of a fight (or the stand-in's declaration, `outcome` `None`): the journal,
    /// the party, the items, trigger event 15 or 16 and, for a lost fight, the wipe-out.
    pub(super) fn settle_battle(&mut self, request: &battle::Resolved, outcome: Option<Outcome>) {
        let won = match &outcome {
            Some(outcome) => outcome.result == BattleResult::Won,
            None => request.number.is_none_or(dq9_core::scenario::stand_in),
        };
        let result = outcome.as_ref().map(|o| o.result);
        match outcome {
            Some(outcome) => {
                if let Some(last) = &mut self.last_fight {
                    last.number = request.number;
                }
                for &item in &outcome.drops {
                    self.state.obtain(i32::from(item), 1);
                }
                if let Some(number) = request.number {
                    status(&format!(
                        "battle {number}: {:?} after {} rounds, {} experience, {} gold, \
                         drops {:?}, {} level up(s)",
                        outcome.result,
                        outcome.rounds,
                        outcome.experience,
                        outcome.gold,
                        outcome.drops,
                        outcome.level_ups.len()
                    ));
                    self.journal.record(
                        self.tick,
                        Step::Battle {
                            number,
                            record: request.record.clone(),
                            won,
                            fought: Some(dq9_core::scenario::Fought {
                                rounds: outcome.rounds,
                                experience: outcome.experience,
                                gold: outcome.gold,
                                result: format!("{:?}", outcome.result),
                            }),
                        },
                    );
                } else {
                    status(&format!(
                        "encounter: {:?} after {} rounds, {} experience, {} gold, drops {:?}, \
                         stage {}",
                        outcome.result,
                        outcome.rounds,
                        outcome.experience,
                        outcome.gold,
                        outcome.drops,
                        request.stage
                    ));
                }
                self.party = outcome.party;
            }
            None => {
                if let Some(number) = request.number {
                    status(&format!(
                        "battle {number}: {won} (stand-in), stage {}",
                        request.stage
                    ));
                    self.journal.record(
                        self.tick,
                        Step::Battle {
                            number,
                            record: request.record.clone(),
                            won,
                            fought: None,
                        },
                    );
                }
            }
        }
        // A flight ends a battle without a verdict: only a win (15) or a loss (16) fires a
        // trigger. The engine ends a set battle in a flight only when a rule it does not
        // model stops it (`Fight::stall`).
        if let (Some(number), Some(field)) = (request.number, &mut self.field)
            && result != Some(BattleResult::Fled)
        {
            let events = field.battle_ended(number, won, &mut self.state);
            self.apply_events(events);
        }
        if result == Some(BattleResult::Lost) {
            self.wipe_out();
        }
        self.settle_roaming(result);
    }

    /// A wiped-out party (`func_02010604`, wiki *Travel*, "A wipe-out"): every fallen member
    /// gets up with full HP and MP, the purse is halved (rounding down; the bank is spared)
    /// and the party comes round on the revival map, at its start point. What the priest
    /// says there (`chur_messet.bin`) and a set battle's own revival map (trigger action 180)
    /// are not modelled.
    fn wipe_out(&mut self) {
        for member in &mut self.party.members {
            if member.hp == 0 {
                member.hp = member.max_hp;
                member.mp = member.max_mp;
            }
        }
        self.party.gold /= 2;
        status(&format!(
            "wipe-out: the purse is halved to {}, the party comes round on map {}",
            self.party.gold, self.revival_map
        ));
        // A preview (`--battle`) has no field to leave: nothing to load.
        if self.options.battle.is_some() || self.rom.is_none() {
            return;
        }
        if self.events.pending_start().is_some() || self.events.is_active() {
            // A scene of the lost-record is running: the wipe-out's own arrival would cut it.
            status("wipe-out: an event is queued; the party stays on this map [unknown]");
            return;
        }
        let from = self.current.as_ref().map(|(e, _)| e.id);
        self.journal.record(
            self.tick,
            Step::MapChange {
                from,
                to: self.revival_map,
                cause: "wipe-out".to_owned(),
            },
        );
        let map = self.revival_map.to_string();
        if !self.load_map(&map, true, None, None, true) {
            status(&format!("wipe-out: map {map} did not load"));
        }
    }

    /// The party's line of the report: level, experience, gold, HP and MP.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn party_report(&self) -> String {
        let mut out = String::new();
        for (i, member) in self.party.members.iter().enumerate() {
            out.push_str(&format!(
                "party {i}: Lv {} experience {} HP {}/{} MP {}/{} gold {}\n",
                member.level(),
                member.current_experience(),
                member.hp,
                member.max_hp,
                member.mp,
                member.max_mp,
                self.party.gold
            ));
        }
        if let Some(fight) = &self.last_fight {
            out.push_str(&format!(
                "last fight: {:?} in {} rounds, {} experience, {} gold, drops {:?}\n",
                fight.result, fight.rounds, fight.experience, fight.gold, fight.drops
            ));
            for line in &fight.log {
                out.push_str(&format!("  | {}\n", line.replace('\n', " / ")));
            }
        }
        out
    }

    /// The party the next battle is fought with.
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn party(&self) -> &Party {
        &self.party
    }

    /// How the last fight went.
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn last_fight(&self) -> Option<&FightSummary> {
        self.last_fight.as_ref()
    }
}

fn summary(number: Option<u16>, outcome: &Outcome, log: &[String]) -> FightSummary {
    FightSummary {
        number,
        result: outcome.result,
        rounds: outcome.rounds,
        experience: outcome.experience,
        gold: outcome.gold,
        drops: outcome.drops.clone(),
        log: log.to_vec(),
    }
}

/// The key presses of the window while a fight is on, as the fight's pad.
pub(super) fn pad_press(pad: &mut Pad, code: winit::keyboard::KeyCode) {
    use winit::keyboard::KeyCode;
    match code {
        KeyCode::Enter | KeyCode::Space | KeyCode::KeyZ => pad.confirm = true,
        KeyCode::Backspace | KeyCode::KeyX | KeyCode::Escape => pad.cancel = true,
        KeyCode::ArrowUp | KeyCode::KeyW => pad.up = true,
        KeyCode::ArrowDown | KeyCode::KeyS => pad.down = true,
        KeyCode::ArrowLeft | KeyCode::KeyA => pad.left = true,
        KeyCode::ArrowRight | KeyCode::KeyD => pad.right = true,
        _ => {}
    }
}

/// The quads of the fight's windows by region of the frame: the message window and the damage
/// numbers on the top screen, the party's status and the menus on the bottom one. With the
/// `top` layout the bottom screen is not shown: its windows are laid over the top one so that
/// the fight stays playable.
pub(crate) fn regions(
    talk: &crate::talk::Talk,
    fight: &Fight,
    scene: &BattleScene,
    screens: &crate::screens::Screens,
    tick: u32,
) -> Vec<(dq9_render::Viewport, Vec<dq9_render::ui::Quad>)> {
    let view = fight.view();
    let frame = talk.with_ui(|atlas, font| {
        battle::ui::frame(atlas, font, &view, &|f| scene.fighter_screen_pos(f), tick)
    });
    let Some(frame) = frame else {
        return Vec::new();
    };
    let top = screens.field(false);
    if screens.wide {
        // One 16:9 field: the DS windows keep their 4:3 shape, anchored top left and bottom
        // left with a margin, rather than stretched over the whole width.
        let margin = (top.height / 30).max(8);
        let height = (top.height / 3).max(1);
        let width = (height * 4 / 3).min(top.width.saturating_sub(2 * margin).max(1));
        let upper = dq9_render::Viewport {
            x: margin,
            y: margin,
            width,
            height,
        };
        let lower = dq9_render::Viewport {
            x: margin,
            y: top.height.saturating_sub(height + margin),
            width,
            height,
        };
        return vec![(upper, frame.top), (lower, frame.bottom)];
    }
    match screens.overlay(false) {
        Some(bottom) => vec![(top, frame.top), (bottom, frame.bottom)],
        None => {
            let mut all = frame.bottom;
            all.extend(frame.top);
            vec![(top, all)]
        }
    }
}

/// Draws the fight's windows over the frame ([`regions`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_ui(
    talk: &mut crate::talk::Talk,
    fight: &Fight,
    scene: &BattleScene,
    gpu: &crate::gpu::Gpu,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    screens: &crate::screens::Screens,
    tick: u32,
) {
    let regions = regions(talk, fight, scene, screens, tick);
    talk.draw_screens(
        &gpu.device,
        &gpu.queue,
        gpu.config.format,
        encoder,
        target,
        &regions,
    );
}

/// Plays `fight` on `scene` with the confirm key held, from the opening until `phase` shows:
/// `menu` (the first command menu), `swing` (a blow landed and still shown), `damage` (a
/// damage number), `levelup` (a level line), `victory` (the experience line) or `lost` (the
/// wipe-out line). For the captures of `--battle-capture`; `rom` and `weapon` are the caller's
/// party's, so the scene's hero plays its own motion set and a call for backup can add its
/// model.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_to_phase(
    fight: &mut Fight,
    scene: &mut BattleScene,
    world: &mut World,
    phase: &str,
    rom: &[u8],
    weapon: Option<u16>,
) -> anyhow::Result<()> {
    use crate::battle::fight::lines_plain;
    use crate::battle::narrate::PopupKind;
    const LIMIT: u32 = 60_000;
    anyhow::ensure!(
        ["menu", "swing", "damage", "levelup", "victory", "lost"].contains(&phase),
        "unknown battle phase {phase:?} (menu, swing, damage, levelup, victory, lost)"
    );
    scene.set_hero_weapon(rom, weapon)?;
    let held = Pad {
        confirm: true,
        ..Pad::default()
    };
    for _ in 0..LIMIT {
        let view = fight.view();
        let text = view.message.map(lines_plain).unwrap_or_default();
        let reached = match phase {
            "menu" => view.menu.is_some(),
            // The blow landed and is still being shown: the actor is at its target.
            "swing" => scene.presenting() && !scene.pending_hit(),
            "damage" => view
                .popups
                .iter()
                .any(|p| p.kind == PopupKind::Damage && p.age > 6),
            "levelup" => text.contains("level increases"),
            "victory" => text.contains("experience"),
            _ => text.contains("wiped out"),
        };
        if reached {
            return Ok(());
        }
        anyhow::ensure!(
            !fight.is_done(),
            "the fight ended before the {phase} phase; log: {:?}",
            fight.log()
        );
        crate::battle::advance_fight(fight, scene, world, held, Some(rom));
        scene.step()?;
    }
    anyhow::bail!("the {phase} phase did not come")
}
