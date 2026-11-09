//! Conversations with NPCs: whom the hero talks to, how a line is prepared and laid out, and
//! the dialogue state machine driven by explicit inputs. No I/O; evidence and open points in
//! `docs/re/dialogue.md`.
//!
//! - [`nearest`] chooses the partner like `Talk_NearestNpc` (`ov_017::021a4e88`);
//! - [`prepare`] runs the message pipeline of a line (`dq9_formats::message`) with the
//!   application's names and builds a [`Script`]: glyphs placed on lines (word wrap),
//!   pages, waits and prompts;
//! - [`Dialogue`] plays a script one 60 Hz step at a time ([`Dialogue::step`]) and exposes
//!   what the window shows ([`Dialogue::view`]).
//!
//! No port marker: `Talk_NearestNpc` is modelled without the object flags and the
//! hero-busy tests, and the window state machine is rebuilt from the observed behaviour of
//! the message interpreter (`0x020659a0`) rather than translated.

use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};

use dq9_formats::cast::{Npc, TalkZone};
use dq9_formats::font::Font;
use dq9_formats::message::{self, Condition, LeadingTags, Name, Token};
use glam::Vec3;

use crate::field::{angle_of_yaw, to_fx};

// --- Who the hero talks to ------------------------------------------------------------------

/// Half-side of the square around an NPC where the hero can talk to it (`fx32`, 1.5).
pub const RANGE: i32 = 0x1800;
/// Distance under which a free (not in a talk zone) partner is accepted (`fx32`, 1.75).
pub const REACH: i32 = 0x1c00;
/// Added to the range and reach of an NPC with a record-20 offset (`+0x1000`, `+0x1400`).
const EXTRA_RANGE: i32 = 0x1000;
const EXTRA_REACH: i32 = 0x1400;

/// What the partner search needs of an NPC of the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Talkable {
    /// NPC id (`<map>npc.bin` field 0, the dialogue file number).
    pub id: u16,
    /// Cast kind: 0 sprite, 1 thing to examine (talked to only from a zone), 2 model…
    pub kind: u8,
    /// Position, `fx32`.
    pub position: [i32; 3],
    pub zones: Vec<TalkZone>,
    /// Record 20 of the placement script: `[dx, 0, dz, extra range]` (`fx32`), the offset of
    /// the talk centre and the extra range **[unknown: exact use]**.
    pub extra: Option<[i32; 4]>,
}

impl Talkable {
    pub fn from_cast(npc: &Npc<'_>) -> Self {
        Self {
            id: npc.member.id,
            kind: npc.member.kind,
            position: npc.placement.position,
            zones: npc.placement.zones.clone(),
            extra: npc.placement.extra,
        }
    }
}

/// A conversation partner: the NPC and the context the talk asks with (id of the talk zone
/// holding the hero, 0 outside any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub npc: u16,
    pub context: u16,
}

/// Talk zone test of `Talk_NearestNpc`: strictly inside the rectangle on x and z.
fn zone_holds(zone: &TalkZone, x: i32, z: i32) -> bool {
    let [x_max, z_max, x_min, z_min] = zone.rect;
    x < x_max && z < z_max && x > x_min && z > z_min
}

/// The first talk zone of `npc` that holds the hero.
fn zone_of(npc: &Talkable, hero: [i32; 3]) -> Option<&TalkZone> {
    npc.zones.iter().find(|z| zone_holds(z, hero[0], hero[2]))
}

/// For every NPC, whether the hero stands in one of its talk zones: the `leader_in_zone_of`
/// condition of the triggers (`crate::trigger::State::zones`).
pub fn zones_containing(npcs: &[Talkable], hero: Vec3) -> BTreeMap<u32, bool> {
    let hero = to_fx(hero);
    npcs.iter()
        .map(|n| (u32::from(n.id), zone_of(n, hero).is_some()))
        .collect()
}

/// Absolute angle between the hero's facing and the direction to a point (0 to π), like
/// `ov_017::021a4700` followed by `fix32abs`.
fn facing_error(hero: Vec3, yaw: f32, to: [i32; 3]) -> f32 {
    let (dx, dz) = (
        to[0] as f32 / 4096.0 - hero.x,
        to[2] as f32 / 4096.0 - hero.z,
    );
    let bearing = dx.atan2(dz);
    let d = (bearing - angle_of_yaw(yaw)).rem_euclid(TAU);
    if d > PI { TAU - d } else { d }
}

/// The partner the action key picks (`Talk_NearestNpc(1)`, `ov_017::021a4e88`).
///
/// An NPC is a candidate when a talk zone holds the hero, or when the hero is within 1.5
/// units on x and z of it (offset and enlarged by its record-20 values) and, for anything
/// but a thing to examine, nearer than 1.75 on the ground plane **[deviation: the game measures
/// the distance in 3D, here the NPC heights are those of the placements, not readjusted]**. Of the
/// candidates the one the hero faces most directly wins; a zone partner is accepted whatever
/// its angle, a free one only when strictly better than the best free one so far. The
/// object flags and the "hero busy" tests are not modelled.
pub fn nearest(npcs: &[Talkable], hero: Vec3, yaw: f32) -> Option<Target> {
    let hero_fx = to_fx(hero);
    let mut best = PI;
    let mut best_free = PI;
    let mut chosen = None;
    for npc in npcs {
        let zone = zone_of(npc, hero_fx);
        let (centre, range, reach) = match npc.extra {
            Some([dx, _, dz, extra]) => (
                [npc.position[0] + dx, npc.position[1], npc.position[2] + dz],
                extra + EXTRA_RANGE,
                extra + EXTRA_REACH,
            ),
            None => (npc.position, RANGE, REACH),
        };
        let near =
            (hero_fx[0] - centre[0]).abs() <= range && (hero_fx[2] - centre[2]).abs() <= range;
        if zone.is_none() && !near {
            continue;
        }
        let angle = facing_error(hero, yaw, npc.position);
        if angle > best {
            continue;
        }
        if let Some(zone) = zone {
            best = angle;
            chosen = Some(Target {
                npc: npc.id,
                context: zone.label as u16,
            });
        } else if npc.kind != 1 {
            let dx = (hero_fx[0] - centre[0]) as f32 / 4096.0;
            let dz = (hero_fx[2] - centre[2]) as f32 / 4096.0;
            let distance = dx.hypot(dz);
            if distance < reach as f32 / 4096.0 && angle < best_free {
                best = angle;
                best_free = angle;
                chosen = Some(Target {
                    npc: npc.id,
                    context: 0,
                });
            }
        }
    }
    chosen
}

// --- Conversation counters ------------------------------------------------------------------

/// Per-NPC conversation counters (`NpcTalkCount_Get/Inc`, `0x0206ec2c`/`0x0206ec74`): two
/// nibbles, both raised after each conversation and saturating at 15. The first is the
/// "area" count, the second the "map" count, cleared when a map is entered
/// (`docs/re/dialogue.md`). The game keeps them in two 0x80-byte tables of the game
/// resources (`gameres+0x44c4`), held here by [`crate::trigger::State`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TalkCounts {
    area: BTreeMap<u16, u8>,
    map: BTreeMap<u16, u8>,
}

impl TalkCounts {
    /// The two counters of `npc`, as `dialogue::TalkState::talk_count` wants them.
    pub fn get(&self, npc: u16) -> [i32; 2] {
        let read = |m: &BTreeMap<u16, u8>| i32::from(m.get(&npc).copied().unwrap_or(0));
        [read(&self.area), read(&self.map)]
    }

    /// A conversation with `npc` took place.
    pub fn record(&mut self, npc: u16) {
        for counts in [&mut self.area, &mut self.map] {
            let n = counts.entry(npc).or_insert(0);
            *n = (*n + 1).min(15);
        }
    }

    /// A map is entered: the map counters are cleared.
    pub fn enter_map(&mut self) {
        self.map.clear();
    }

    /// A track moves to a new chapter or step (`Track_Advance`, `0x020703d8`, calls
    /// `0x0206ec04`, which zeroes both tables): every counter is cleared.
    pub fn clear(&mut self) {
        self.area.clear();
        self.map.clear();
    }

    /// Trigger action 0x6a (`Trig_RunAction` case 6, `0x02061c14`): both counters of `npc`
    /// are cleared (`0x0206ecf8` on each table). The game ignores numbers past 255 (byte
    /// `npc / 2` of a 0x80-byte table, error path otherwise).
    pub fn reset(&mut self, npc: u16) {
        if npc < 0x100 {
            self.area.remove(&npc);
            self.map.remove(&npc);
        }
    }
}

// --- Preparing a line -----------------------------------------------------------------------

/// Names and conditions the application knows for the message pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    /// `<HERO>`, `<LEADER>`, `<ADDRESSEE>`.
    pub hero: Vec<u8>,
    pub hero_male: bool,
}

impl Default for Names {
    fn default() -> Self {
        Self {
            hero: b"Hero".to_vec(),
            hero_male: true,
        }
    }
}

/// Branch of a conditional block: the first when the condition holds. Only the hero's sex
/// and the size of the party (a party of one) are known; every other rule takes its first
/// branch, like the `dq9re msg` preview **[hypothesis]**.
fn branch(rule: &Condition, names: &Names) -> u8 {
    match rule.start {
        "IF_HERO_MALE" | "IF_MALE" | "IF_HOST_MALE" | "IF_ADDRESSEE_MALE" => {
            u8::from(!names.hero_male)
        }
        "IF_FEMALE_PARTY" => 1,
        _ => 0,
    }
}

fn resolve_name(name: Name, names: &Names) -> Vec<u8> {
    match name {
        Name::Substitution { tag, index } => match tag {
            "HERO" | "LEADER" | "ADDRESSEE" => names.hero.clone(),
            _ => format!("[{tag}{index}]").into_bytes(),
        },
        Name::Inflected { entity, index, .. } => format!("[{entity:?}{index}]").into_bytes(),
        Name::Weapon => b"[weapon]".to_vec(),
        Name::Actor => b"[actor]".to_vec(),
    }
}

/// The six passes of `Msg_Prepare` (`0x02067ea8`) then the leading layout tags and the
/// compilation of `Msg_Compile`.
pub fn compile_message(font: &Font, text: &[u8], names: &Names) -> (LeadingTags, Vec<Token>) {
    let text = message::expand_values(text, |i| format!("{}", i + 1).into_bytes());
    let text = message::normalize(font, &text);
    let text = message::eval_conditionals(&text, |rule| branch(rule, names));
    let text = message::expand_names(&text, |name| resolve_name(name, names));
    let text = message::apply_cap(font, &text);
    let (tags, rest) = message::leading_tags(&text);
    (tags, message::compile(rest))
}

/// What a prompt asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// `<YESNO>` and its variants `<YESNO_NOTSE>`, `<YESNO_NOTSE_IIE>` (same choices).
    YesNo,
    /// `<NOYES>`: the cursor starts on No.
    NoYes,
    /// `<UKEYAME>`: Accept / Decline.
    AcceptDecline,
}

impl PromptKind {
    /// English labels (system strings 0x1b to 0x1e **[hypothesis: numbers]**).
    pub fn labels(self) -> [&'static str; 2] {
        match self {
            Self::YesNo | Self::NoYes => ["Yes", "No"],
            Self::AcceptDecline => ["Accept", "Decline"],
        }
    }

    /// Codes of the markers the answers jump to: first and second choice.
    fn markers(self) -> [u16; 2] {
        match self {
            Self::YesNo | Self::NoYes => [0xFF14, 0xFF15],
            Self::AcceptDecline => [0xFF16, 0xFF17],
        }
    }
}

/// How a message ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndKind {
    /// `<END>`, `<END_R_TURN>`: the text stays until the action key closes the window.
    Wait,
    /// `<CLOSE>`: the window closes at once.
    Close,
    /// `<ADD>`: the message ends and the window stays for the next message of the scene.
    Add,
}

/// A speaker's name laid out for its plate (`0x0206ad84` measures the name the same way).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plate {
    pub text: Vec<u8>,
    /// Glyphs and offsets, as `Font::layout` places them.
    pub glyphs: Vec<(u8, i32)>,
    /// Advance of the whole name, trailing spacing included (`+0x1944`).
    pub advance: i32,
}

/// A sound of the message window (`docs/re/sdat.md`: base archive effects 10 to 12, 6, 28, 5,
/// 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// A typed character (`0x02045608`); the voice is the application's.
    Blip,
    /// `<EXC>` (`0xFF2E`).
    Exclamation,
    /// `<QES>` (`0xFF2F`).
    Question,
    /// A choice opens (`<YESNO>`, `<UKEYAME>`...).
    Choice,
    /// An answer is chosen (`MsgWin_AnswerYesNo` `0x020678cc`, `MsgWin_AnswerAcceptDecline`
    /// `0x020679e4`): base effect 1, except on the `NOTSE` prompts (see [`AnswerSound`]).
    ChoiceConfirm,
}

/// Which answers of a prompt play [`Sound::ChoiceConfirm`], as the answer handlers decide
/// from the prompt code: `<YESNO>`, `<NOYES>` and `<UKEYAME>` always,
/// `<YESNO_NOTSE>` (0xFF06) never, `<YESNO_NOTSE_IIE>` (0xFF07) only for the first answer.
/// Cancel (B) is silent whatever the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerSound {
    Both,
    First,
    None,
}

impl AnswerSound {
    /// Whether the answer `choice` (0 first, 1 second) plays the confirmation effect.
    fn plays(self, choice: u8) -> bool {
        matches!((self, choice), (Self::Both, _) | (Self::First, 0))
    }
}

/// Characters typed between two blips: the game blips when its counter passes 3 and takes 4
/// off (`0x020659a0`) **[hypothesis: one character per count]**.
pub const BLIP_EVERY: u32 = 4;

/// One step of a prepared message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// A glyph of the font at `x` on the current line.
    Glyph {
        glyph: u8,
        x: i32,
    },
    /// Start of the next line.
    Line,
    /// Speaker of the page (`//Name//`).
    Name(Plate),
    /// `<PAGE>`, `<PAGE_T=n>`: wait for the action key, then clear the box.
    Page,
    /// `<PAD_WAIT>`, `<PAD_T=n>` (with the cursor), `<PAD_WAIT_NOCUR>` (without).
    Wait {
        cursor: bool,
    },
    /// `<TIME=n>`: pause for `n` steps.
    Pause(u32),
    /// `<EXC>`, `<QES>`: a sound, no wait (`0x020659a0`).
    Sound(Sound),
    /// A choice: its labels and which answers play the confirmation effect.
    Prompt {
        kind: PromptKind,
        sound: AnswerSound,
    },
    /// Branch marker `<YES>` (0xFF14), `<NO>`, `<UKE>`, `<YAME>`.
    Marker(u16),
    /// `<LB_x>`: target of the jumps to the same letter (`0xFFD0 + n`).
    Label(u16),
    /// `<JP_x>`: continue after the label `0xFFD0 + n` (the code kept is the label's).
    Jump(u16),
    End(EndKind),
}

/// A message ready to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub ops: Vec<Op>,
    pub tags: LeadingTags,
    /// Lines the box holds (`<GYOU=n>`, 3 by default).
    pub max_lines: usize,
}

/// Default number of lines of the box (`win+0x9a4 = 3`, `0x0204501c`).
pub const DEFAULT_LINES: usize = 3;

/// Builds the script of compiled `tokens`, wrapping lines at `max_width` pixels the way
/// `0x0206b984` does: a line breaks at the last space (or after a `-`) when the next glyph
/// would pass the width, the space being dropped; a word without break point overflows.
pub fn layout(tokens: &[Token], tags: LeadingTags, font: &Font, max_width: i32) -> Script {
    const NO_GLYPH: u8 = 0xFF;
    let mut ops: Vec<Op> = Vec::new();
    let (mut x, mut previous) = (0i32, NO_GLYPH);
    // Index in `ops` and pen position just after the last place a line may break.
    let mut break_at: Option<(usize, i32)> = None;
    let space = i32::from(font.space_width()) + 1;
    let mut page_start = true;
    let reset = |x: &mut i32, previous: &mut u8, break_at: &mut Option<(usize, i32)>| {
        *x = 0;
        *previous = NO_GLYPH;
        *break_at = None;
    };
    for token in tokens {
        match token {
            Token::Text(bytes) => {
                let mut text = bytes.as_slice();
                if page_start {
                    // `//Name// ` opens the page; the name is drawn in its own plate.
                    if let Some(after) = text.strip_prefix(b"//")
                        && let Some(end) = after.windows(2).position(|w| w == b"//")
                    {
                        let (placed, advance) = font.layout(&after[..end]);
                        ops.push(Op::Name(Plate {
                            text: after[..end].to_vec(),
                            glyphs: placed,
                            advance,
                        }));
                        text = &after[end + 2..];
                        text = text.strip_prefix(b" ").unwrap_or(text);
                    }
                    page_start = false;
                }
                while !text.is_empty() {
                    let Some(index) = font.find(text) else {
                        x += space;
                        previous = NO_GLYPH;
                        if text[0] == b' ' {
                            break_at = Some((ops.len(), x));
                        }
                        text = &text[1..];
                        continue;
                    };
                    let g = font.glyphs[usize::from(index)];
                    let kerning = i32::from(font.kerning(previous, index));
                    if let Some((at, bx)) = break_at
                        && x + kerning + i32::from(g.width) > max_width
                    {
                        ops.insert(at, Op::Line);
                        for op in &mut ops[at + 1..] {
                            if let Op::Glyph { x, .. } = op {
                                *x -= bx;
                            }
                        }
                        x -= bx;
                        break_at = None;
                    }
                    x += kerning;
                    ops.push(Op::Glyph { glyph: index, x });
                    x += i32::from(g.width) + 1;
                    previous = index;
                    if g.text.first() == Some(&b'-') {
                        break_at = Some((ops.len(), x));
                    }
                    text = &text[g.text.len()..];
                }
            }
            Token::NewLine => {
                ops.push(Op::Line);
                reset(&mut x, &mut previous, &mut break_at);
            }
            Token::Control { code, arg } => {
                let n = || {
                    arg.as_deref()
                        .and_then(|a| std::str::from_utf8(a).ok())
                        .and_then(|a| a.trim().parse::<u32>().ok())
                        .unwrap_or(0)
                };
                let op = match *code {
                    0xFF01 | 0xFF02 => Some(Op::End(EndKind::Wait)),
                    0xFF03 => Some(Op::End(EndKind::Close)),
                    0xFF0A => Some(Op::End(EndKind::Add)),
                    0xFF0C | 0xFF0D => Some(Op::Page),
                    0xFF1D | 0xFF1E => Some(Op::Wait { cursor: true }),
                    0xFF2E => Some(Op::Sound(Sound::Exclamation)),
                    0xFF2F => Some(Op::Sound(Sound::Question)),
                    0xFF1F => Some(Op::Wait { cursor: false }),
                    0xFF1A => Some(Op::Pause(n())),
                    0xFF04 => Some(Op::Prompt {
                        kind: PromptKind::YesNo,
                        sound: AnswerSound::Both,
                    }),
                    0xFF05 => Some(Op::Prompt {
                        kind: PromptKind::NoYes,
                        sound: AnswerSound::Both,
                    }),
                    0xFF06 => Some(Op::Prompt {
                        kind: PromptKind::YesNo,
                        sound: AnswerSound::None,
                    }),
                    0xFF07 => Some(Op::Prompt {
                        kind: PromptKind::YesNo,
                        sound: AnswerSound::First,
                    }),
                    0xFF08 => Some(Op::Prompt {
                        kind: PromptKind::AcceptDecline,
                        sound: AnswerSound::Both,
                    }),
                    0xFF14..=0xFF17 => Some(Op::Marker(*code)),
                    0xFFD1..=0xFFDF => Some(Op::Label(*code)),
                    0xFFE1..=0xFFEF => Some(Op::Jump(*code - 0x10)),
                    _ => None,
                };
                if let Some(op) = op {
                    let boundary = matches!(
                        op,
                        Op::End(_) | Op::Page | Op::Marker(_) | Op::Label(_) | Op::Jump(_)
                    );
                    ops.push(op);
                    if boundary {
                        reset(&mut x, &mut previous, &mut break_at);
                        page_start = true;
                    }
                }
            }
        }
    }
    Script {
        ops,
        max_lines: tags.lines.map_or(DEFAULT_LINES, |n| n.max(1) as usize),
        tags,
    }
}

/// The whole preparation of a talk line: [`compile_message`] then [`layout`].
pub fn prepare(font: &Font, text: &[u8], names: &Names, max_width: i32) -> Script {
    let (tags, tokens) = compile_message(font, text, names);
    layout(&tokens, tags, font, max_width)
}

// --- The dialogue state machine -------------------------------------------------------------

/// Steps a finished line waits before the box scrolls up (`0x1864 = 20` pixels, one per
/// tick **[hypothesis: one pixel per step]**).
pub const SCROLL_STEPS: u32 = 20;

/// Buttons pressed during one step (edges, not levels).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    /// A: advance, answer, skip the typing.
    pub confirm: bool,
    /// B: answer "No" to a prompt.
    pub cancel: bool,
    pub up: bool,
    pub down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Typing,
    /// A line past the box's last one is pending: the box scrolls when the count ends.
    Scroll(u32),
    WaitPage,
    WaitPad {
        cursor: bool,
    },
    Pause(u32),
    Prompt {
        kind: PromptKind,
        cursor: u8,
        sound: AnswerSound,
    },
    WaitEnd,
    Closed(EndKind),
}

/// A prompt on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptView {
    pub kind: PromptKind,
    pub cursor: u8,
}

/// What the message window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View<'a> {
    /// Speaker of the page, if any.
    pub name: Option<&'a Plate>,
    /// Typed lines: glyph and x offset in the line.
    pub lines: &'a [Vec<(u8, i32)>],
    /// The wait arrow is shown (page, end, or pad wait with cursor).
    pub arrow: bool,
    pub prompt: Option<PromptView>,
    pub tags: &'a LeadingTags,
}

/// How a conversation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub end: EndKind,
    /// Choice (0 first, 1 second) of the last prompt answered.
    pub answer: Option<u8>,
}

/// Plays a [`Script`].
#[derive(Debug, Clone)]
pub struct Dialogue {
    script: Script,
    at: usize,
    state: State,
    name: Option<Plate>,
    lines: Vec<Vec<(u8, i32)>>,
    answer: Option<u8>,
    hold_at_end: bool,
    /// Sounds made and not yet taken ([`Dialogue::take_sounds`]).
    sounds: Vec<Sound>,
    /// Characters typed, for the blip.
    typed: u32,
    blip: bool,
}

impl Dialogue {
    pub fn new(script: Script) -> Self {
        Self {
            script,
            at: 0,
            state: State::Typing,
            name: None,
            lines: vec![Vec::new()],
            answer: None,
            hold_at_end: false,
            sounds: Vec::new(),
            typed: 0,
            blip: false,
        }
    }

    /// A message of an event script (native 400): when its text ends without an end tag it
    /// waits for the action key instead of closing at once **[hypothesis]**.
    pub fn for_event(script: Script) -> Self {
        Self {
            hold_at_end: true,
            ..Self::new(script)
        }
    }

    /// `Some` once the window closed.
    pub fn outcome(&self) -> Option<Outcome> {
        match self.state {
            State::Closed(end) => Some(Outcome {
                end,
                answer: self.answer,
            }),
            _ => None,
        }
    }

    pub fn view(&self) -> View<'_> {
        let waiting = matches!(
            self.state,
            State::WaitPage | State::WaitEnd | State::WaitPad { cursor: true }
        );
        View {
            name: self.name.as_ref(),
            lines: &self.lines,
            arrow: waiting,
            prompt: match self.state {
                State::Prompt { kind, cursor, .. } => Some(PromptView { kind, cursor }),
                _ => None,
            },
            tags: &self.script.tags,
        }
    }

    /// The sounds the window made since the last call, for the application to play.
    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    fn clear_box(&mut self) {
        self.lines = vec![Vec::new()];
        self.name = None;
    }

    /// Runs the script up to `budget` glyphs or the next wait.
    fn run(&mut self, mut budget: usize) {
        while budget > 0 {
            let Some(op) = self.script.ops.get(self.at) else {
                // An event's message stays until the action key (or the script) closes it.
                self.state = if self.hold_at_end {
                    State::WaitEnd
                } else {
                    State::Closed(EndKind::Wait)
                };
                return;
            };
            match op {
                Op::Glyph { glyph, x } => {
                    if let Some(line) = self.lines.last_mut() {
                        line.push((*glyph, *x));
                    }
                    budget -= 1;
                    self.typed += 1;
                    self.blip |= self.typed.is_multiple_of(BLIP_EVERY);
                }
                Op::Line => {
                    if self.lines.len() >= self.script.max_lines {
                        self.state = State::Scroll(SCROLL_STEPS);
                        return;
                    }
                    self.lines.push(Vec::new());
                }
                Op::Name(plate) => self.name = Some(plate.clone()),
                Op::Page => {
                    self.at += 1;
                    self.state = State::WaitPage;
                    return;
                }
                Op::Wait { cursor } => {
                    self.state = State::WaitPad { cursor: *cursor };
                    self.at += 1;
                    return;
                }
                Op::Pause(n) => {
                    self.at += 1;
                    self.state = State::Pause(*n);
                    return;
                }
                Op::Prompt { kind, sound } => {
                    let cursor = u8::from(*kind == PromptKind::NoYes);
                    self.state = State::Prompt {
                        kind: *kind,
                        cursor,
                        sound: *sound,
                    };
                    self.sounds.push(Sound::Choice);
                    return;
                }
                Op::Sound(sound) => self.sounds.push(*sound),
                Op::Marker(_) | Op::Label(_) => {}
                Op::Jump(label) => {
                    let target = self
                        .script
                        .ops
                        .iter()
                        .position(|op| *op == Op::Label(*label));
                    self.at = target.unwrap_or(self.at);
                }
                Op::End(EndKind::Wait) => {
                    self.at += 1;
                    self.state = State::WaitEnd;
                    return;
                }
                Op::End(end) => {
                    self.state = State::Closed(*end);
                    return;
                }
            }
            self.at += 1;
        }
    }

    /// One 60 Hz step: one glyph is typed (the action key types up to the next wait).
    pub fn step(&mut self, input: Input) {
        self.step_state(input);
        // One blip a step at most, however many characters the action key typed.
        if std::mem::take(&mut self.blip) {
            self.sounds.push(Sound::Blip);
        }
    }

    fn step_state(&mut self, input: Input) {
        match self.state {
            State::Closed(_) => {}
            State::Typing => self.run(if input.confirm { usize::MAX } else { 1 }),
            State::Scroll(left) => {
                if left > 1 {
                    self.state = State::Scroll(left - 1);
                } else {
                    self.lines.remove(0);
                    self.lines.push(Vec::new());
                    self.at += 1;
                    self.state = State::Typing;
                }
            }
            State::WaitPage => {
                if input.confirm {
                    self.clear_box();
                    self.state = State::Typing;
                }
            }
            State::WaitPad { .. } => {
                if input.confirm {
                    self.state = State::Typing;
                }
            }
            State::Pause(left) => {
                self.state = if left > 1 {
                    State::Pause(left - 1)
                } else {
                    State::Typing
                };
            }
            State::Prompt {
                kind,
                cursor,
                sound,
            } => {
                let moved = if input.up || input.down {
                    1 - cursor
                } else {
                    cursor
                };
                self.state = State::Prompt {
                    kind,
                    cursor: moved,
                    sound,
                };
                // B selects the second answer without a sound (`0x020678cc` receives -2);
                // A confirms the cursor and plays effect 1 unless the prompt's `NOTSE` code
                // suppresses it.
                let choice = if input.cancel {
                    Some((1, false))
                } else {
                    input.confirm.then_some((moved, true))
                };
                if let Some((choice, confirmed)) = choice {
                    if confirmed && sound.plays(choice) {
                        self.sounds.push(Sound::ChoiceConfirm);
                    }
                    self.answer = Some(choice);
                    let marker = kind.markers()[usize::from(choice)];
                    let found = self.script.ops[self.at..]
                        .iter()
                        .position(|op| *op == Op::Marker(marker));
                    self.at = match found {
                        Some(i) => {
                            self.clear_box();
                            self.at + i + 1
                        }
                        // An answer without branch ends the message **[hypothesis]**.
                        None => self.script.ops[self.at..]
                            .iter()
                            .position(|op| matches!(op, Op::End(_)))
                            .map_or(self.script.ops.len(), |i| self.at + i),
                    };
                    self.state = State::Typing;
                }
            }
            State::WaitEnd => {
                if input.confirm {
                    self.state = State::Closed(EndKind::Wait);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
