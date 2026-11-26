use dq9_formats::font::Font;
use dq9_formats::message::{self, Condition, LeadingTags, Name, Token};

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
    pub(super) fn markers(self) -> [u16; 2] {
        match self {
            Self::YesNo | Self::NoYes => [0xFF14, 0xFF15],
            Self::AcceptDecline => [0xFF16, 0xFF17],
        }
    }
}

/// How a message ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndKind {
    /// `<END>`: the text stays until the action key closes the window.
    Wait,
    /// `<END_R_TURN>` (`0xFF02`): like [`EndKind::Wait`], and the key that closes the window
    /// gives the partner the heading the conversation found (`func_020440cc` case 0,
    /// [`Dialogue::take_turn_release`](super::Dialogue::take_turn_release)).
    Turn,
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

/// Which balloon `<EXC>`/`<QES>` raises over the speaker (`+0x195c`, the byte
/// `*(u8 *)(0x020e7e18 + kind)` of `func_020675fc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalloonKind {
    /// `<EXC>` (`0xFF2E`): the "!" (`ev_mark.spr`, field sprite 13).
    Exclamation,
    /// `<QES>` (`0xFF2F`): the "?" (`field_qu.spr`, field sprite 14).
    Question,
}

/// Ticks a balloon stays up: `+0x1959 = 0x3c` set by both forms of the tag, then counted down
/// by `func_02065758` and cleared at zero (`+0x19b7`). The field's update is the 60 Hz loop,
/// so the port counts one step per tick **[hypothesis: `GetTickCount` deltas of 1]**.
pub const BALLOON_TICKS: u32 = 60;

/// The balloon raised over the speaker: what `Talk` draws and counts down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Balloon {
    pub kind: BalloonKind,
    /// Ticks left before the window clears it (`+0x1959`).
    pub ticks: u32,
}

impl Balloon {
    /// A balloon raised by its tag: [`BALLOON_TICKS`] ticks left, like `+0x1959 = 0x3c`.
    pub fn new(kind: BalloonKind) -> Self {
        Self {
            kind,
            ticks: BALLOON_TICKS,
        }
    }
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
    pub(super) fn plays(self, choice: u8) -> bool {
        matches!((self, choice), (Self::Both, _) | (Self::First, 0))
    }
}

/// Characters typed between two blips: the game blips when its counter passes 3 and takes 4
/// off (`0x020659a0`) **[hypothesis: one character per count]**.
pub const BLIP_EVERY: u32 = 4;

// Fixed steps per `MsgWin_Update` call of the field (`speed.rs`). The counters of `<TIME=n>`
// and `<PAD_T=n>` drop by one per call, those of `<PAGE_T=n>` by the tick count.
use super::speed::STEPS_PER_UPDATE;

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
    /// A space inside a line (`0xFF19`): nothing is drawn, the message unit is spent. A space
    /// where the line breaks becomes the break instead ([`Op::Line`]).
    Space,
    /// Speaker of the page (`//Name//`).
    Name(Plate),
    /// `<PAGE>`, `<PAGE_T=n>`: wait for the action key, then clear the box.
    Page,
    /// `<PAGE_T=n>`, `<PAD_T=n>`: the next wait also ends by itself after this many steps
    /// (`func_020440cc` counts `<PAGE_T>` in ticks, `0x020659a0` counts `<PAD_T>` in updates).
    Timeout(u32),
    /// `<PAD_WAIT>`, `<PAD_T=n>` (with the cursor), `<PAD_WAIT_NOCUR>` (without).
    Wait {
        cursor: bool,
    },
    /// `<TIME=n>`: pause for `n` updates of the window, 2 steps each (`0x020659a0`
    /// decrements the counter once per call).
    Pause(u32),
    /// `<EXC>`, `<QES>`: a sound and a [`Balloon`] over the speaker, no wait (`0x020659a0`,
    /// which raises the same 60-tick timer as the leading form, `0x0206a3d0`).
    Balloon(BalloonKind),
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
    /// `<R_TURN>` (`0xFF2C`): the partner is given the heading the conversation found while the
    /// rest of the page still shows (`0x020659a0`, the code runner: the requested angle
    /// `+0x1844` becomes the saved `+0x183c` and `+0x19b6` keeps handing it over). Reported by
    /// [`Dialogue::take_turn_release`](super::Dialogue::take_turn_release).
    ReleaseTurn,
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
                            ops.push(Op::Space);
                        }
                        text = &text[1..];
                        continue;
                    };
                    let g = font.glyphs[usize::from(index)];
                    let kerning = i32::from(font.kerning(previous, index));
                    if let Some((at, bx)) = break_at
                        && x + kerning + i32::from(g.width) > max_width
                    {
                        // A break on a space replaces it: the original's stream holds one
                        // `0xFF18` where the line wraps, and no space.
                        if matches!(ops.get(at), Some(Op::Space)) {
                            ops[at] = Op::Line;
                        } else {
                            ops.insert(at, Op::Line);
                        }
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
                // `<PAGE_T=n>` (0xFF0C) copies `n` ticks into `+0x1961` and `func_020440cc`
                // counts them down while the page waits; `<PAD_T=n>` (0xFF1E) counts `n`
                // updates. Both then act as the action key. `n` = 0 never times out on a page.
                match *code {
                    0xFF0C if n() > 0 => ops.push(Op::Timeout(n())),
                    0xFF1E => ops.push(Op::Timeout(n().max(1).saturating_mul(STEPS_PER_UPDATE))),
                    _ => {}
                }
                let op = match *code {
                    0xFF01 => Some(Op::End(EndKind::Wait)),
                    0xFF02 => Some(Op::End(EndKind::Turn)),
                    0xFF03 => Some(Op::End(EndKind::Close)),
                    0xFF0A => Some(Op::End(EndKind::Add)),
                    0xFF2C => Some(Op::ReleaseTurn),
                    0xFF0C | 0xFF0D => Some(Op::Page),
                    0xFF1D | 0xFF1E => Some(Op::Wait { cursor: true }),
                    0xFF2E => Some(Op::Balloon(BalloonKind::Exclamation)),
                    0xFF2F => Some(Op::Balloon(BalloonKind::Question)),
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
