//! What a battle says: the engine's events turned into the game's messages, one [`Beat`] per
//! window, each with the scene cues that go with it (`docs/re/battle-text.md`; wiki
//! *Battle-Text*, *Ability-Handlers* "The record's lines", *Battle-Stages* "Out").
//!
//! The rules (**[hypothesis]** where the wiki marks them INFERRED, and for the grouping of
//! lines into windows, which the code was not read for):
//!
//! - an action says its opening line (`actmsg`, by the target's side), then for each target
//!   the critical line, the line of what it did (or its failure line on a dodge or a block)
//!   and the kill line when the target fell;
//! - the opening of a fight says `strbtl` 6 for each kind of monster (8 for a boss), then the
//!   surprise line;
//! - a victory says `str_bres` 1 (50 for several) then 25 or 26 for the experience, a level
//!   line (10, and 13 for skill points) for each level gained, gold 16 and a chest for each
//!   drop (17, 18); a wipe-out says `str_bres` 20; a flight `actmsg` 45, or `str_bres` 24
//!   when the enemy block the way.

use anyhow::{Context as _, Result};
use dq9_core::battle::{Event, RoundReport, Surprise, Who};
use dq9_core::party::LevelUp;
use dq9_formats::battle::monster::Names;
use dq9_formats::battle::text::{BattleText, Context, File, ItemNames, monster_name};
use dq9_formats::message::NameRecord;

use super::Fighter;
use super::data::{ActionLines, Data};

/// Ticks a window stays up once its line is shown (the opening closes itself after 45,
/// `<TIME=45>`, wiki *Battle-Stages*; the rest is **[hypothesis]**).
pub const HOLD: u32 = 40;
/// A flight's line closes itself 30 frames after it appears (wiki *Battle-Stages*, "Out").
pub const HOLD_FLEE: u32 = 30;
/// A monster's flight: Flee `0xE1` and `0x395` (`docs/re/battle.md`, "Flee `0xE1`"), and the
/// `actmsg` line of its record's opening when it has none (the party's flight says 45).
const FLEE_ACTIONS: [u16; 2] = [0xe1, 0x395];
const FLEE_LINE: u16 = 45;
/// Action 3, Defend, and its `actmsg` line ("defends").
const DEFEND_ACTION: u16 = 3;
const DEFEND_LINE: u16 = 10;

/// A scene effect that goes with a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cue {
    /// The defeat fade: the `death` motion, then hidden once.
    Remove(Fighter),
    /// A flight: the `escape` motion, then hidden.
    Escape(Fighter),
    /// A monster of this number joined the battle (its model is added to the scene,
    /// playing `appear`), and its caller plays `call`.
    Summon { caller: Fighter, number: u16 },
    /// The action's presentation: the actor plays its script's motion (the blow and its
    /// lunge), and the `damaged` targets play their `damage` motion when it lands.
    Act {
        actor: Fighter,
        action: u16,
        targets: Vec<Fighter>,
        damaged: Vec<Fighter>,
    },
    /// The party member's HP the status window shows from now on.
    Hp(usize, i32),
    /// The party member's MP the status window shows from now on.
    Mp(usize, i32),
    /// A number (or a word) over the fighter: damage, healing, a miss.
    Popup {
        fighter: Fighter,
        text: String,
        kind: PopupKind,
    },
}

/// What a popup says, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Damage,
    Heal,
    Miss,
}

/// One window of the battle: its lines, how long it stays, what it does to the scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beat {
    /// The text as the font draws it (`<1>` is the apostrophe glyph), one entry a line.
    pub lines: Vec<Vec<u8>>,
    /// Ticks it stays once shown; ignored when `wait` is set.
    pub hold: u32,
    /// The window waits for a key instead (the results, wiki *Battle-Stages*).
    pub wait: bool,
    pub cues: Vec<Cue>,
}

impl Beat {
    fn new(text: &[u8], hold: u32) -> Self {
        Self {
            lines: text.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect(),
            hold,
            wait: false,
            cues: Vec::new(),
        }
    }

    /// The window's text as a string, lines joined by `\n` and the apostrophe glyph written
    /// as one (what the tests and the log compare).
    pub fn plain(&self) -> String {
        plain(&self.lines.join(&b'\n'))
    }
}

/// A message's bytes as a string: `<1>` (the apostrophe glyph) becomes `'`.
pub fn plain(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("<1>", "'")
}

/// Who is in the fight, as the messages name them.
#[derive(Debug, Clone)]
pub struct Roster {
    /// The party's names, as the font draws them.
    pub party: Vec<Vec<u8>>,
    /// The monsters' numbers, in the battle's order.
    pub monsters: Vec<u16>,
    /// The item each member chose this round (`Command::Item`), for the line that names it.
    pub items: Vec<Option<u16>>,
}

/// The builder of one set of messages: the text tables, the names and the roster.
pub struct Narrator<'a> {
    data: &'a Data,
    text: BattleText<'a>,
    names: Names<'a>,
    items: ItemNames<'a>,
    roster: &'a Roster,
}

impl<'a> Narrator<'a> {
    pub fn new(data: &'a Data, roster: &'a Roster) -> Result<Self> {
        Ok(Self {
            data,
            text: data.text()?,
            names: data.monster_names()?,
            items: data.item_names()?,
            roster,
        })
    }

    /// The name record of a combatant.
    fn record(&self, who: Who) -> Option<NameRecord<'_>> {
        match who {
            Who::Party(i) => self.roster.party.get(i).map(|name| NameRecord {
                singular: name,
                plural: name,
                // No article: a party member's name stands bare (**[hypothesis]**: the party
                // records carry no grammar word, the tags then print the name alone).
                grammar: 0,
            }),
            Who::Monster(j) => {
                let number = *self.roster.monsters.get(j)?;
                Some(monster_name(self.names.find(number)?))
            }
        }
    }

    /// A message's text for `actor` and `target` with the values `values`.
    pub fn say(
        &self,
        file: File,
        number: u16,
        actor: Option<Who>,
        target: Option<Who>,
        values: &[u32],
    ) -> Result<Vec<u8>> {
        self.say_with(file, number, actor, target, values, None, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn say_with(
        &self,
        file: File,
        number: u16,
        actor: Option<Who>,
        target: Option<Who>,
        values: &[u32],
        action: Option<&ActionLines>,
        item: Option<NameRecord<'_>>,
    ) -> Result<Vec<u8>> {
        let context = Context {
            actor: actor.and_then(|w| self.record(w)),
            target: target.and_then(|w| self.record(w)),
            monster: target
                .filter(|w| matches!(w, Who::Monster(_)))
                .and_then(|w| self.record(w))
                .or_else(|| {
                    actor
                        .filter(|w| matches!(w, Who::Monster(_)))
                        .and_then(|w| self.record(w))
                }),
            item,
            action: action.map(|a| NameRecord {
                singular: &a.name,
                plural: &a.name,
                grammar: 0,
            }),
            hero: self.roster.party.first().cloned().unwrap_or_default(),
            values: values.to_vec(),
            solo: self.roster.party.len() == 1,
            actor_party: matches!(actor, Some(Who::Party(_))),
            target_party: matches!(target, Some(Who::Party(_))),
            target_count: 1,
            ..Context::default()
        };
        self.text
            .message(file, number, &context)
            .with_context(|| format!("{file:?} message {number}"))
    }

    /// `str_btl`'s label `number` (the menus' words).
    pub fn label(&self, number: u16) -> Vec<u8> {
        self.say(File::StrBtl, number, None, None, &[])
            .unwrap_or_default()
    }

    fn beat(&self, text: Result<Vec<u8>>, hold: u32) -> Option<Beat> {
        match text {
            Ok(text) if !text.is_empty() => Some(Beat::new(&text, hold)),
            Ok(_) => None,
            Err(e) => {
                crate::app::status(&format!("battle text: {e:#}"));
                None
            }
        }
    }

    /// The opening: each kind of monster draws near, then the surprise line
    /// (`func_ov026_021dd8a8`, `strbtl` 6, 7, 8 or 9 by the monsters' kinds).
    pub fn opening(&self, surprise: Surprise) -> Vec<Beat> {
        let mut out = Vec::new();
        let mut seen: Vec<u16> = Vec::new();
        for (j, &number) in self.roster.monsters.iter().enumerate() {
            if seen.contains(&number) {
                continue;
            }
            seen.push(number);
            let count = self
                .roster
                .monsters
                .iter()
                .filter(|&&n| n == number)
                .count() as u32;
            let boss = self.names.find(number).is_some_and(|r| r.kind >= 0x101);
            let line = self.say(
                File::Strbtl,
                if boss { 8 } else { 6 },
                Some(Who::Monster(j)),
                Some(Who::Monster(j)),
                &[count],
            );
            out.extend(self.beat(line, HOLD));
        }
        let first = Who::Monster(0);
        match surprise {
            Surprise::None => {}
            Surprise::Monsters => out.extend(self.beat(
                self.say(File::Strbtl, 14, Some(first), Some(first), &[1]),
                HOLD,
            )),
            Surprise::Party => out.extend(self.beat(
                self.say(File::Strbtl, 11, Some(first), Some(first), &[1]),
                HOLD,
            )),
        }
        out
    }

    /// One round's windows. `hp` and `mp` are what the status windows show before it; the
    /// cues keep them current.
    pub fn round(&self, report: &RoundReport) -> Vec<Beat> {
        let mut out = Vec::new();
        let mut item: Option<u16> = None;
        for event in &report.events {
            match event {
                Event::Flee { success, .. } => {
                    let hero = Some(Who::Party(0));
                    if *success {
                        let beat =
                            self.beat(self.say(File::Actmsg, 45, hero, hero, &[]), HOLD_FLEE);
                        if let Some(mut beat) = beat {
                            beat.cues.push(Cue::Escape(fighter_of(Who::Party(0))));
                            out.push(beat);
                        }
                    } else {
                        out.extend(self.beat(self.say(File::StrBres, 24, hero, hero, &[]), HOLD));
                    }
                }
                Event::Defended(who) => {
                    // Action 3 says its opening line (`actmsg` 10, "defends") and the script
                    // is `default.bact`'s `guard`.
                    let line = self
                        .data
                        .lines
                        .get(&DEFEND_ACTION)
                        .map_or(DEFEND_LINE, |l| l.opening[0]);
                    let beat = self.beat(
                        self.say(File::Actmsg, line, Some(*who), Some(*who), &[]),
                        HOLD,
                    );
                    if let Some(mut beat) = beat {
                        beat.cues.push(Cue::Act {
                            actor: fighter_of(*who),
                            action: DEFEND_ACTION,
                            targets: Vec::new(),
                            damaged: Vec::new(),
                        });
                        out.push(beat);
                    }
                }
                Event::ItemUsed { who, .. } => {
                    // The window of the action that follows names the item the member chose.
                    item = match who {
                        Who::Party(i) => self.roster.items.get(*i).copied().flatten(),
                        Who::Monster(_) => None,
                    };
                }
                Event::Acted {
                    who,
                    action,
                    outcomes,
                } => {
                    let record = item
                        .take()
                        .and_then(|id| self.items.find(id))
                        .map(|i| i.name_record());
                    let summoned = report.events.iter().find_map(|e| match e {
                        Event::Summoned {
                            who: caller,
                            number,
                        } if caller == who => Some(*number),
                        _ => None,
                    });
                    let start = out.len();
                    let kind = self.data.lines.get(action).map_or(0, |l| l.kind);
                    self.acted(*who, *action, outcomes, record, summoned, &mut out);
                    // The presentation of the action. A flight and a call for backup say what
                    // they do themselves (`Escape`, `Summon`), and a kind-34 action has no
                    // script (`func_ov025_021d8a40`).
                    if !FLEE_ACTIONS.contains(action) && kind != 12 && kind != 34 {
                        let cue = Cue::Act {
                            actor: fighter_of(*who),
                            action: *action,
                            targets: outcomes.iter().map(|(w, _)| fighter_of(*w)).collect(),
                            damaged: outcomes
                                .iter()
                                .filter(|(_, o)| {
                                    o.landed && !o.dodged && !o.blocked && o.hp_after < o.hp_before
                                })
                                .map(|(w, _)| fighter_of(*w))
                                .collect(),
                        };
                        match out.get_mut(start) {
                            Some(beat) => beat.cues.push(cue),
                            None => {
                                let mut beat = Beat::new(b"", 0);
                                beat.cues.push(cue);
                                out.push(beat);
                            }
                        }
                    }
                }
                Event::Summoned { .. }
                | Event::Initiative(_)
                | Event::Chose { .. }
                | Event::Skipped(_)
                | Event::End(_) => {}
            }
        }
        out
    }

    fn acted(
        &self,
        who: Who,
        number: u16,
        outcomes: &[(Who, dq9_core::battle::TargetOutcome)],
        item: Option<NameRecord<'_>>,
        summoned: Option<u16>,
        out: &mut Vec<Beat>,
    ) {
        let Some(lines) = self.data.lines.get(&number) else {
            crate::app::status(&format!("battle text: action {number} has no record"));
            return;
        };
        let side = |w: Who| usize::from(matches!(w, Who::Monster(_)));
        let first = outcomes.first().map(|(w, _)| *w);
        // The opening, by the first target's side.
        let opening = first.map_or(lines.opening[0], |w| lines.opening[side(w)]);
        if FLEE_ACTIONS.contains(&number) {
            // A monster's flight (`func_0x021554f4` with reason 2): it says so and leaves, with
            // no damage line and no reward.
            let line = if opening != 0 { opening } else { FLEE_LINE };
            let text = self.say_with(File::Actmsg, line, Some(who), first, &[], Some(lines), None);
            if let Some(mut beat) = self.beat(text, HOLD) {
                beat.cues.push(Cue::Escape(fighter_of(who)));
                out.push(beat);
            }
            return;
        }
        if opening != 0 {
            let text = self.say_with(
                File::Actmsg,
                opening,
                Some(who),
                first,
                &[],
                Some(lines),
                item,
            );
            if let Some(beat) = self.beat(text, HOLD) {
                out.push(beat);
            }
        }
        if lines.kind == 12 {
            // A call for backup: the record says what came of it, at the caller itself.
            let line = if summoned.is_some() {
                lines.done[1]
            } else {
                lines.failed[1]
            };
            if line != 0 {
                let text = self.say_with(
                    File::Actmsg,
                    line,
                    Some(who),
                    Some(who),
                    &[],
                    Some(lines),
                    None,
                );
                if let Some(mut beat) = self.beat(text, HOLD) {
                    beat.cues.extend(summoned.map(|number| Cue::Summon {
                        caller: fighter_of(who),
                        number,
                    }));
                    out.push(beat);
                }
            }
            return;
        }
        for (target, o) in outcomes {
            let target = *target;
            let side = side(target);
            let tf = fighter_of(target);
            if o.critical && lines.critical != 0 {
                let text = self.say_with(
                    File::Actmsg,
                    lines.critical,
                    Some(who),
                    Some(target),
                    &[],
                    Some(lines),
                    None,
                );
                out.extend(self.beat(text, HOLD));
            }
            let missed = !o.landed || o.dodged || o.blocked;
            // `failed` and `done` are indexed [party, monster].
            let line = if missed {
                lines.failed[side]
            } else {
                lines.done[side]
            };
            let mut popup = None;
            if missed {
                popup = Some(Cue::Popup {
                    fighter: tf,
                    text: "Miss".to_owned(),
                    kind: PopupKind::Miss,
                });
            } else if o.hp_after > o.hp_before {
                popup = Some(Cue::Popup {
                    fighter: tf,
                    text: (o.hp_after - o.hp_before).to_string(),
                    kind: PopupKind::Heal,
                });
            } else if o.amount > 0 || o.hp_after < o.hp_before {
                popup = Some(Cue::Popup {
                    fighter: tf,
                    text: o.amount.to_string(),
                    kind: PopupKind::Damage,
                });
            }
            let values = [o.amount.max(0) as u32];
            let text = if line == 0 {
                Ok(Vec::new())
            } else {
                self.say_with(
                    File::Actmsg,
                    line,
                    Some(who),
                    Some(target),
                    &values,
                    Some(lines),
                    None,
                )
            };
            let mut beat = self.beat(text, HOLD).unwrap_or_else(|| Beat::new(b"", 0));
            beat.cues.extend(popup);
            if let Who::Party(i) = target {
                beat.cues.push(Cue::Hp(i, o.hp_after));
            }
            if beat.lines.iter().all(Vec::is_empty) {
                // No line: its cues play with the previous window (or alone, with none).
                match out.last_mut() {
                    Some(previous) => previous.cues.extend(beat.cues),
                    None => out.push(beat),
                }
            } else {
                out.push(beat);
            }
            if o.hp_before > 0 && o.hp_after <= 0 {
                let text = if lines.killed[side] == 0 {
                    Ok(Vec::new())
                } else {
                    self.say_with(
                        File::Actmsg,
                        lines.killed[side],
                        Some(who),
                        Some(target),
                        &values,
                        Some(lines),
                        None,
                    )
                };
                if let Some(mut beat) = self.beat(text, HOLD) {
                    if matches!(target, Who::Monster(_)) {
                        beat.cues.push(Cue::Remove(tf));
                    }
                    out.push(beat);
                }
            }
        }
    }

    /// The end of a won battle: the defeat line, the experience, the levels, the gold and
    /// the drops. Every window waits for a key (wiki *Battle-Stages*, "Out").
    pub fn victory(
        &self,
        shared: bool,
        gold: u32,
        level_ups: &[LevelUp],
        drops: &[(u16, Option<usize>)],
        items: &dq9_formats::battle::text::ItemNames<'_>,
    ) -> Vec<Beat> {
        let mut out = Vec::new();
        let several = self.roster.monsters.len() > 1;
        let last = Who::Monster(0);
        out.extend(self.beat(
            self.say(
                File::StrBres,
                if several { 50 } else { 1 },
                None,
                Some(last),
                &[],
            ),
            HOLD,
        ));
        let lead = Who::Party(0);
        out.extend(self.beat(
            self.say(
                File::StrBres,
                if shared { 26 } else { 25 },
                None,
                Some(lead),
                &[],
            ),
            HOLD,
        ));
        for up in level_ups {
            let who = Who::Party(up.member);
            out.extend(self.beat(
                self.say(File::StrBres, 10, None, Some(who), &[u32::from(up.level)]),
                HOLD,
            ));
            if up.skill_points > 0 {
                out.extend(self.beat(
                    self.say(
                        File::StrBres,
                        13,
                        None,
                        Some(who),
                        &[u32::from(up.skill_points)],
                    ),
                    HOLD,
                ));
            }
        }
        if gold > 0 {
            out.extend(self.beat(self.say(File::StrBres, 16, None, None, &[gold]), HOLD));
        }
        for &(item, monster) in drops {
            let record = items.find(item).map(|i| i.name_record());
            let from = monster.map(Who::Monster);
            let chest = self.say_with(File::StrBres, 17, from, Some(lead), &[], None, record);
            out.extend(self.beat(chest, HOLD));
            let found = self.say_with(File::StrBres, 18, from, Some(lead), &[], None, record);
            out.extend(self.beat(found, HOLD));
        }
        for beat in &mut out {
            beat.wait = true;
        }
        out
    }

    /// The end of a lost battle: the wipe-out line (wiki *Battle-Stages*, "Out").
    pub fn wipe_out(&self) -> Vec<Beat> {
        let mut out = Vec::new();
        let lead = Some(Who::Party(0));
        out.extend(self.beat(self.say(File::StrBres, 20, lead, lead, &[]), HOLD));
        for beat in &mut out {
            beat.wait = true;
        }
        out
    }
}

/// The scene's fighter for a combatant.
pub fn fighter_of(who: Who) -> Fighter {
    match who {
        Who::Party(i) => Fighter::party(i),
        Who::Monster(j) => Fighter::monster(j),
    }
}
