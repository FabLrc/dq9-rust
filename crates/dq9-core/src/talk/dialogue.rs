use dq9_formats::message::LeadingTags;

use super::layout::{AnswerSound, BLIP_EVERY, EndKind, Op, Plate, PromptKind, Script, Sound};

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
