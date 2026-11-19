use dq9_formats::message::LeadingTags;

use super::layout::{AnswerSound, BLIP_EVERY, EndKind, Op, Plate, PromptKind, Script, Sound};
use super::speed::{STEPS_PER_UPDATE, TextSpeed};

// --- The dialogue state machine -------------------------------------------------------------

/// Steps a finished line waits before the box scrolls up: `+0x1868 += ticks` per update until
/// it reaches the line height `+0x1864 = 20`, i.e. one pixel per 60 Hz step (`0x020659a0`,
/// 10 updates of 2 ticks).
pub const SCROLL_STEPS: u32 = 20;

/// Steps between the key that ends an `<END>` message and the window closing: `MsgWin_Update`
/// (`0x020659a0`) sees the key through `MsgWin_PollWaitKey` (`0x020440dc`, state 0 returns 1),
/// sets `+0x19d1` and `+0x1958 = 5`, then takes the update's 2 ticks off each call and closes
/// (`0x02043214`, which clears `+0x19bd`) when the count goes negative: 3 updates, 6 steps.
/// Measured in melonDS: the key at frame 61 clears `+0x998` and `+0x19bd` at frame 67
/// (`docs/re/melonds-state.md`, § "State resume"); 61 and 67 are absolute frame numbers of
/// that capture, the delay between the two is the 6 steps. `<ADD>` has no such delay (case 3
/// clears `+0x19bd` at once) and `<CLOSE>` calls the close itself.
pub const CLOSE_STEPS: u32 = 6;

// --- The wait arrow's blink ---
//
// Measured on the original (melonDS, `build/evidence/j4-message-arrow/` and the challenger's
// states of `build/scratch/challenger-msg/`): in wait states 1, 2 and 3 the down triangle of
// the message window is drawn 32 frames and hidden 32 frames, the first visible frame being
// the state's frame 10 (`docs/re/dialogue.md` § "Message pacing"). States 0 (`<END>`) and 4
// (`<AUTO>`) draw none.

/// Frames the wait arrow stays hidden when its wait state starts: the first visible frame.
const ARROW_BLINK_START: u32 = 10;
/// Frames the wait arrow is drawn, then hidden (`+0x912` 3 then 5).
const ARROW_BLINK_ON: u32 = 32;
const ARROW_BLINK_OFF: u32 = 32;

/// Whether the wait arrow is drawn on `frame` of its wait state: hidden for the first
/// [`ARROW_BLINK_START`] frames, then on for 32 frames and off for 32.
fn arrow_visible(frame: u32) -> bool {
    frame >= ARROW_BLINK_START
        && (frame - ARROW_BLINK_START) % (ARROW_BLINK_ON + ARROW_BLINK_OFF) < ARROW_BLINK_ON
}

/// Buttons pressed during one step (edges, not levels).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    /// A: answer a prompt. It does nothing while the text types: the original ignores the key
    /// in that state (`func_020440cc` has no case for state 5; measured, `docs/re/dialogue.md`
    /// § "Text speed").
    pub confirm: bool,
    /// B: answer "No" to a prompt.
    pub cancel: bool,
    pub up: bool,
    pub down: bool,
    /// A key that leaves a wait-for-key state on its press edge beyond A, B and up/down: X, L,
    /// R or the left/right d-pad (`MsgWin_PollWaitKey` `0x020440dc`: mask `0x403`, `0x200`
    /// and `0x100` for L/R set by `MsgWin_Show`, and `0xf0` for the d-pad). Y, Start and
    /// Select do not advance, and the test is an edge (`TestKeysPressed` `0x02012454`:
    /// `cur & m && !(prev & m)`), so a held key acts once. The application maps its keys onto
    /// this flag; A, B and up/down also leave a wait through their own fields
    /// ([`Input::leaves_wait`]).
    pub advance: bool,
}

impl Input {
    /// Whether a key that leaves a wait-for-key state was pressed: A, B, X, L, R or any
    /// d-pad key. A caller that only models A (`confirm`) still leaves a wait.
    pub fn leaves_wait(self) -> bool {
        self.advance || self.confirm || self.cancel || self.up || self.down
    }
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
    /// `<END>`: the text stays until the action key. State 0 draws no wait arrow.
    WaitEnd,
    /// The key was pressed on an `<END>`: the window closes when the count ends.
    Closing(u32),
    /// `<ADD>` (state 3): the text stays until the action key, with the blinking wait arrow;
    /// the script's next message then continues in the same window.
    WaitAdd,
    Closed(EndKind),
}

impl State {
    /// The wait states that draw the wait arrow: page (1), pad (2) and `<ADD>` (3). State 0
    /// (`<END>`) and state 4 (`<AUTO>`) draw none (melonDS, `docs/re/dialogue.md`
    /// § "Message pacing").
    fn draws_arrow(self) -> bool {
        matches!(
            self,
            State::WaitPage | State::WaitPad { .. } | State::WaitAdd
        )
    }
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
    /// The wait arrow is drawn this frame: states 1 (page), 2 (pad) and 3 (`<ADD>`), hidden
    /// for the state's first 10 frames then 32 frames on and 32 off (measured).
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
    /// Message units typed, for the blip.
    typed: u32,
    blip: bool,
    speed: TextSpeed,
    /// Fraction of a unit carried between steps (`TextSpeed::units`).
    carry: u32,
    /// A `<PAGE_T>`/`<PAD_T>` limit read, until the wait it belongs to starts.
    limit: Option<u32>,
    /// Steps left before the current wait ends by itself.
    wait_left: Option<u32>,
    /// Frames spent in the current wait state, for the arrow's blink. Zero on the frame the
    /// state starts.
    arrow_frames: u32,
    /// An answer's branch started and has typed nothing yet.
    empty_branch: bool,
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
            speed: TextSpeed::default(),
            carry: 0,
            limit: None,
            wait_left: None,
            arrow_frames: 0,
            empty_branch: false,
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

    /// Sets the typing speed (the port's option; [`TextSpeed::Normal`] is the original's rate).
    pub fn set_speed(&mut self, speed: TextSpeed) {
        self.speed = speed;
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

    /// The window waits for the player: a page (`<PAGE>`/`<PAGE_T>`), a pad wait, an `<ADD>`,
    /// an `<END>` or a prompt. Whether it also draws the wait arrow is [`View::arrow`].
    pub fn waiting(&self) -> bool {
        matches!(
            self.state,
            State::WaitPage
                | State::WaitPad { .. }
                | State::WaitEnd
                | State::WaitAdd
                | State::Prompt { .. }
        )
    }

    /// The waits the application's test probes (`Talk::trace`) log as "window shows:
    /// arrow": states 0, 1 and 2 with the cursor, the class those probes were written against.
    /// What the window actually draws is [`View::arrow`] (states 1, 2 and 3, blinking).
    pub fn probe_arrow(&self) -> bool {
        matches!(
            self.state,
            State::WaitPage | State::WaitEnd | State::WaitPad { cursor: true }
        )
    }

    pub fn view(&self) -> View<'_> {
        View {
            name: self.name.as_ref(),
            lines: &self.lines,
            arrow: self.state.draws_arrow() && arrow_visible(self.arrow_frames),
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

    /// Spends one message unit: every code of the original's stream that does not stop the
    /// interpreter costs one (`iVar2 -= 1` in `0x020659a0`), and the blip counts them.
    fn spend(&mut self, budget: &mut usize) {
        *budget -= 1;
        self.typed += 1;
        self.blip |= self.typed.is_multiple_of(BLIP_EVERY);
    }

    /// Starts a wait: the `<PAGE_T>`/`<PAD_T>` limit read before it, if any, now runs.
    fn begin_wait(&mut self, state: State) {
        self.state = state;
        self.wait_left = self.limit.take();
    }

    /// Whether the wait's own limit ran out this step (it then acts as the action key).
    fn wait_expired(&mut self) -> bool {
        match &mut self.wait_left {
            Some(left) => {
                *left = left.saturating_sub(1);
                *left == 0
            }
            None => false,
        }
    }

    /// Runs the script for `budget` message units or up to the next wait.
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
                    self.empty_branch = false;
                    if let Some(line) = self.lines.last_mut() {
                        line.push((*glyph, *x));
                    }
                    self.spend(&mut budget);
                }
                Op::Space => self.spend(&mut budget),
                Op::Line => {
                    if self.lines.len() >= self.script.max_lines {
                        self.state = State::Scroll(SCROLL_STEPS);
                        return;
                    }
                    self.lines.push(Vec::new());
                    self.spend(&mut budget);
                }
                Op::Name(plate) => self.name = Some(plate.clone()),
                Op::Page => {
                    self.at += 1;
                    self.begin_wait(State::WaitPage);
                    return;
                }
                Op::Timeout(steps) => self.limit = Some(*steps),
                Op::Wait { cursor } => {
                    self.begin_wait(State::WaitPad { cursor: *cursor });
                    self.at += 1;
                    return;
                }
                Op::Pause(n) => {
                    self.at += 1;
                    self.state = State::Pause(n.max(&1).saturating_mul(STEPS_PER_UPDATE));
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
                Op::Sound(sound) => {
                    self.sounds.push(*sound);
                    self.spend(&mut budget);
                }
                Op::Marker(_) | Op::Label(_) => self.spend(&mut budget),
                Op::Jump(label) => {
                    let target = self
                        .script
                        .ops
                        .iter()
                        .position(|op| *op == Op::Label(*label));
                    self.at = target.unwrap_or(self.at);
                }
                // An answer whose branch is only `<END>` (`<YES><END>`): the marker exists
                // (`0x0206b2d8` moves `+0x54/+0x58` to it and sets `+0x6c = 4`), then the
                // page-preparation function (`0x0206aa38`, `func_0206aa28` in the
                // decompiler) reaches a terminator before any glyph and calls `0x02043214`
                // and `0x02043134`: an `<END>` on an empty page closes the window with the
                // answer, no key and no wait. melonDS, NPC 130 of `X01M07`: the answer key at
                // frame 795, `W+0x9a0` 6 to 0 and `W+0x998` 1 to 0 at frame 797, against 6
                // steps after the key for a typed `<END>` (`docs/re/dialogue.md`).
                Op::End(EndKind::Wait) if self.empty_branch => {
                    self.state = State::Closed(EndKind::Wait);
                    return;
                }
                Op::End(EndKind::Wait) => {
                    self.at += 1;
                    self.state = State::WaitEnd;
                    return;
                }
                Op::End(EndKind::Add) => {
                    self.at += 1;
                    self.state = State::WaitAdd;
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

    /// One 60 Hz step: the text reveals [`TextSpeed::units`] message units (one at the
    /// original's rate), whatever key is pressed.
    pub fn step(&mut self, input: Input) {
        let before = self.state;
        self.step_state(input);
        if self.state.draws_arrow() {
            // Frame 0 is the frame the wait starts on: the arrow's blink is counted from it.
            self.arrow_frames = if before == self.state {
                self.arrow_frames.wrapping_add(1)
            } else {
                0
            };
        }
        // One blip a step at most, however many characters the action key typed.
        if std::mem::take(&mut self.blip) {
            self.sounds.push(Sound::Blip);
        }
    }

    fn step_state(&mut self, input: Input) {
        match self.state {
            State::Closed(_) => {}
            State::Typing => {
                let units = self.speed.units(&mut self.carry);
                self.run(units);
            }
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
                if input.leaves_wait() | self.wait_expired() {
                    self.clear_box();
                    self.state = State::Typing;
                }
            }
            State::WaitPad { .. } => {
                if input.leaves_wait() | self.wait_expired() {
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
                    match found {
                        Some(i) => {
                            self.clear_box();
                            self.at += i + 1;
                            self.empty_branch = true;
                            self.state = State::Typing;
                        }
                        // No marker for this answer: the callback (`0x020678cc`) asks
                        // `0x0206b2d8` for the branch, finds none and closes the window
                        // (`0x02043214`) at once, with no key of its own.
                        None => self.state = State::Closed(EndKind::Wait),
                    }
                }
            }
            State::WaitEnd => {
                if input.leaves_wait() {
                    self.state = State::Closing(CLOSE_STEPS);
                }
            }
            State::Closing(left) => {
                self.state = if left > 1 {
                    State::Closing(left - 1)
                } else {
                    State::Closed(EndKind::Wait)
                };
            }
            State::WaitAdd => {
                if input.leaves_wait() {
                    self.state = State::Closed(EndKind::Add);
                }
            }
        }
    }
}
