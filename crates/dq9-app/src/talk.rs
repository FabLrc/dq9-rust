//! Conversations in the application: the action key starts a talk with the NPC in front of
//! the hero (`dq9_core::talk`), the line is chosen and prepared from the ROM, and the game's
//! message window is drawn over the map with the ROM's font (`dq9_render::ui`).
//!
//! Layout numbers come from `MessageShow` (`0x0204501c`): window rectangle `(2, 0x74, 0xfc,
//! 0x4a)`, three lines of 20 pixels, name plate `advance + 26 - (advance & 7)` wide and 22 high.
//! The margins of the text, the plate and prompt positions, the fill colour and the wait arrow
//! are **[hypotheses]** (`docs/re/dialogue.md`, § "Message window").

use anyhow::{Context as _, Result, bail};
use dq9_core::Hero;
use dq9_core::field::{Event, Field, TalkTrigger};
use dq9_core::talk::{
    self, Balloon, BalloonKind, Dialogue, Input, Names, PromptKind, TalkCounts, Talkable, TextSpeed,
};
use dq9_core::trigger::{Request, State};
use dq9_formats::dialogue::{self, Session, TalkState};
use dq9_formats::spr::Sprite;
use dq9_formats::{Font, FontId, Gpc2, Table};
use dq9_render::ui::{Atlas, Image, Quad, Ui};
use dq9_render::{AnimatedScene, Camera, Viewport};
use nds_rom::Rom;

use crate::app::status;

/// Language of the dialogue files (`NNN_<lg>.bin`) and of the prompt labels.
const LANGUAGE: &str = "en";
/// Chapter letters of the dialogue archives (`Scenario_ChapterLetter`, `ov_017::0218d2c4`).
const CHAPTER_LETTERS: &[u8] = b"ABCDEFGHIJSTKLMNOPQ";

/// `TalkTask_Update` (`ov_017::021b8e8c`) uses the map family and current chapter.
fn archive_path(map_code: &str, chapter: u8) -> Result<String> {
    let letter = *CHAPTER_LETTERS
        .get(usize::from(chapter).wrapping_sub(1))
        .with_context(|| format!("chapter {chapter} has no dialogue archive"))?;
    let area = map_code.get(..3).context("map code")?;
    Ok(format!("data/scenario/{area}{}0.gp2", char::from(letter)))
}

/// The message window on the 256×192 screen (`func_02042b98(win, 2, 0x74, 0xfc, 0x4a)`).
pub(crate) const WINDOW: [i32; 4] = [2, 0x74, 0xfc, 0x4a];
/// Where the text starts, from the window's corner **[hypothesis]** (the game's text origin is
/// `(2, 0x7e)` in its own text layer).
pub(crate) const TEXT_MARGIN: [i32; 2] = [12, 10];
/// Pixels between the lines (`+0x1864 = 0x14`).
pub(crate) const LINE_HEIGHT: i32 = 20;
/// Width the text wraps at **[hypothesis]**.
const TEXT_WIDTH: i32 = 228;
const NAME_HEIGHT: i32 = 22;
/// Window fill **[hypothesis]**.
pub(crate) const FILL: [f32; 4] = [0.02, 0.03, 0.22, 0.9];
pub(crate) const SELECTED: [f32; 4] = [0.25, 0.35, 0.8, 0.95];

/// Field sprite ids of the two balloons, as `event/scan.rs::field_sprite` names them:
/// 13 `ev_mark.spr` (the "!"), 14 `field_qu.spr` (the "?") of `data/bin/icon.nsarc`
/// (`*(u8 *)(0x020e7e18 + kind)` of `func_020675fc`).
const BALLOON_SPRITES: [i32; 2] = [13, 14];
/// The balloon's sheet corner from the partner's projected point: `(−12, −62)` pixels
/// (`0x020675fc`).
const BALLOON_OFFSET: [f32; 2] = [-12.0, -62.0];
/// The DS screen, the canvas of the window pass (`[x, y, w, h]`).
const SCREEN: [f32; 4] = [0.0, 0.0, 256.0, 192.0];

/// Glyphs and width of one prompt label.
#[derive(Debug)]
struct Label {
    glyphs: Vec<(u8, i32)>,
    width: i32,
}

/// What the ROM gives the interface: the font and the atlas.
#[derive(Debug)]
struct Assets {
    font_info: Vec<u8>,
    font_data: Vec<u8>,
    atlas: Atlas,
    /// Labels of [`PromptKind::YesNo`] and [`PromptKind::AcceptDecline`].
    labels: [[Label; 2]; 2],
}

fn sprite_image(rom: &Rom, path: &str) -> Result<Image> {
    let bytes = rom.file_by_path(path)?;
    let sprite = Sprite::parse(bytes).with_context(|| format!("parsing {path}"))?;
    let (width, height, rgba) = sprite
        .frame_rgba(0)
        .with_context(|| format!("{path} has no frame"))?;
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// The first frame of a field sprite's sheet (`data/bin/icon.nsarc`, by its field id):
/// `ev_mark` and `field_qu` for the two balloons. `func_020675fc` draws the animated sprite
/// with `SPRSprite::DrawAnimatedNoRotation(sprite, 1)`; the first frame is what the port
/// shows **[hypothesis: the sheets of the balloons have a single frame]**.
fn field_sprite_image(rom: &[u8], id: i32) -> Result<Image> {
    let name =
        crate::event::scan::field_sprite(id).with_context(|| format!("no field sprite {id}"))?;
    let bytes = crate::event::scan::field_sprite_bytes(rom, id)
        .with_context(|| format!("field sprite {id} ({name}) is not in data/bin/icon.nsarc"))?;
    let sprite = Sprite::parse(&bytes).with_context(|| format!("parsing {name}.spr"))?;
    let (width, height, rgba) = sprite
        .frame_rgba(0)
        .with_context(|| format!("{name}.spr has no frame"))?;
    Ok(Image {
        width,
        height,
        rgba,
    })
}

impl Assets {
    fn load(rom: &[u8]) -> Result<Self> {
        let rom = Rom::parse(rom).context("parsing the ROM")?;
        let name = FontId::Message.name();
        let font_info = rom
            .file_by_path(&format!("data/pack_lv5/fi_{name}.bin"))?
            .to_vec();
        let font_data = rom
            .file_by_path(&format!("data/pack_lv5/fd_{name}.bin"))?
            .to_vec();
        let font =
            Font::parse(&font_info, &font_data, FontId::Message).context("parsing the font")?;
        let mut border = Vec::new();
        for i in 0..8 {
            border.push(sprite_image(&rom, &format!("data/ani/win{i}.spr"))?);
        }
        let border: [Image; 8] = border
            .try_into()
            .map_err(|_| anyhow::anyhow!("eight window pieces"))?;
        // The big arrow of `arrow2.spr` as the wait arrow **[hypothesis]**.
        let arrow = sprite_image(&rom, "data/ani/arrow2.spr")?;
        let label = |text: &str| {
            let (glyphs, advance) = font.layout(text.as_bytes());
            Label {
                glyphs,
                width: advance - 1,
            }
        };
        let [yes, no] = PromptKind::YesNo.labels();
        let [accept, decline] = PromptKind::AcceptDecline.labels();
        // The two balloons over a speaker (`func_020675fc`): a missing sheet leaves the
        // message window working, without balloons.
        let mut sheets = Vec::new();
        let raw = rom.data();
        for id in BALLOON_SPRITES {
            match field_sprite_image(raw, id) {
                Ok(image) => sheets.push(image),
                Err(e) => status(&format!("balloon sprite unavailable: {e:#}")),
            }
        }
        if sheets.len() != BALLOON_SPRITES.len() {
            sheets.clear();
        }
        Ok(Self {
            atlas: Atlas::build_sprites(&font, &border, &arrow, &sheets),
            labels: [[label(yes), label(no)], [label(accept), label(decline)]],
            font_info,
            font_data,
        })
    }

    fn font(&self) -> Result<Font<'_>> {
        Font::parse(&self.font_info, &self.font_data, FontId::Message).context("parsing the font")
    }

    fn labels(&self, kind: PromptKind) -> &[Label; 2] {
        match kind {
            PromptKind::YesNo | PromptKind::NoYes => &self.labels[0],
            PromptKind::AcceptDecline => &self.labels[1],
        }
    }
}

/// BGR555 (`<COLOR=…>`) as a tint.
fn tint(color: u16) -> [f32; 4] {
    let c = |shift: u32| f32::from((color >> shift & 0x1f) as u8) / 31.0;
    [c(0), c(5), c(10), 1.0]
}

/// The protagonist as a save stores him: the name as glyph indices of the message font
/// (`Character::name_raw`, `+0x140` / 320) and the sex bit (`Character::gender`, `+0x174` /
/// 372, bit 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeroIdentity {
    /// Name as stored: indices of message-font glyphs, zero-terminated.
    pub name: [u8; dq9_formats::save::NAME_LEN],
    /// `+0x174` bit 0 is clear: man (1 is a woman, `docs/re/characters.md`).
    pub male: bool,
}

impl HeroIdentity {
    /// The names the message pipeline substitutes: every saved glyph index becomes the
    /// text the font knows it by (the convention of [`dq9_formats::save::Character::name`]),
    /// so a name such as é keeps its glyph. An empty name keeps the documented identity.
    fn names(self, font: &Font<'_>) -> Names {
        let mut hero = Vec::new();
        for &index in self.name.iter().take_while(|&&b| b != 0) {
            match font.glyphs.get(usize::from(index)) {
                Some(glyph) => hero.extend_from_slice(glyph.text),
                None => hero.push(b'?'),
            }
        }
        if hero.is_empty() {
            return Names::default();
        }
        Names {
            hero,
            hero_male: self.male,
        }
    }
}

/// A service tag ending a talk line (`func_0206f550`, wiki *Text events*): `<INN=n>`,
/// `<CHURCH=n>` and `<SHOP=n>`. The compiler drops the tag from the displayed text
/// (`dq9_formats::message::SKIPPED`); the conversation keeps it here so that the application
/// opens the facility once the window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceTag {
    /// `<INN=n>` opens `str_in<n − 1>`.
    Inn(u16),
    /// `<CHURCH=n>` opens `str_ch<n − 1>`.
    Church(u16),
    /// `<SHOP=n>` opens the shop numbered `n`.
    Shop(u16),
}

/// The first service tag of a talk line, as `func_0206f550` strips it. Tags are matched as
/// written on the cartridge (upper case) and their number is read up to `>`.
pub fn service_tag(text: &[u8]) -> Option<ServiceTag> {
    let (at, prefix, kind) = [
        (b"<INN=".as_slice(), 0u8),
        (b"<CHURCH=".as_slice(), 1),
        (b"<SHOP=".as_slice(), 2),
    ]
    .into_iter()
    .find_map(|(prefix, kind)| {
        text.windows(prefix.len())
            .position(|window| window == prefix)
            .map(|at| (at, prefix, kind))
    })?;
    let mut number = 0u32;
    let mut read = false;
    for &byte in &text[at + prefix.len()..] {
        let Some(digit) = char::from(byte).to_digit(10) else {
            break;
        };
        number = number.saturating_mul(10).saturating_add(digit);
        read = true;
    }
    let tag = u16::try_from(number).ok().filter(|_| read)?;
    Some(match kind {
        0 => ServiceTag::Inn(tag),
        1 => ServiceTag::Church(tag),
        _ => ServiceTag::Shop(tag),
    })
}

/// A closed conversation, as trigger event 1 sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended {
    pub npc: u16,
    pub context: u16,
    /// Choice (0 first, 1 second) of the last prompt answered.
    pub answer: Option<u8>,
}

/// Where the field's 3D view of the current pass is drawn, and with which camera: the balloon
/// over the speaker is projected with it (`func_0202ec84` of the original, called with the
/// field camera). The rectangle itself depends on the pass ([`Talk::draw`]: the DS screen;
/// [`Talk::draw_hud`]: the whole canvas), so each computes it and passes it in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FieldView {
    camera: Camera,
    /// The enhanced-reactive presentation: the camera's field of view follows the window's
    /// aspect ([`Camera::reactive_view_proj`]).
    reactive: bool,
}

/// Conversations of the application: the NPCs of the map, the open dialogue. The counters
/// live in the trigger state ([`State::talk_counts`]).
#[derive(Debug, Default)]
pub struct Talk {
    assets: Option<Assets>,
    gpu: Option<(wgpu::TextureFormat, Ui)>,
    npcs: Vec<Talkable>,
    names: Names,
    /// The saved protagonist, once known; `None` keeps the documented default identity.
    identity: Option<HeroIdentity>,
    active: Option<Dialogue>,
    /// Buttons pressed since the last step.
    pending: Input,
    ticks: u32,
    /// How fast the text types (`--text-speed`, `?textspeed=`): the original's rate unless the
    /// player chose another (`dq9_core::talk::TextSpeed`).
    speed: TextSpeed,
    /// An event message is open (see [`Talk::show_event`]).
    event_message: bool,
    /// The player closed it.
    closed: bool,
    /// Native 410 hid the frame of the current event message.
    frameless: bool,
    /// Native 409's timed caption alpha (`None`: 1, no timed caption; `02066d00`).
    caption_alpha: Option<f32>,
    /// Native 411: the caption's text block is centred vertically.
    centered_caption: bool,
    /// Native 413: no blip while the caption types.
    silent_caption: bool,
    /// Natives 417/421 (`0206b804`): the drawn offset of the message window and of its text.
    window_offset: [i32; 2],
    /// Native 409's message-tick freeze: the action key is not processed while the timed
    /// caption runs, so 405 cannot close the message before the alpha envelope ends.
    caption_frozen: bool,
    /// The NPC and context of the open conversation (not an event message).
    conversation: Option<(u16, u16)>,
    /// The partner a conversation holds still and turns to the hero ([`Talk::face_partner`]).
    facing: Option<u16>,
    /// The open conversation gave the partner its heading back (`<R_TURN>` mid-message or the
    /// key on `<END_R_TURN>`): the pivot turns back and is not requested again while that
    /// conversation runs.
    turn_back: bool,
    /// The service tag of the open talk line, if any (`<INN=n>`…): the facility opens when
    /// the window closes.
    service: Option<ServiceTag>,
    /// The balloon over the partner (`+0x1959` ticks left, `+0x19b7` raised): raised by
    /// `<EXC>`/`<QES>` (`0x0206a3d0` leading, `0x020659a0` inside the page), counted down once
    /// per fixed step and cleared at zero (`func_02065758`).
    balloon: Option<Balloon>,
    /// The partner the balloon hangs over: the speaker of the conversation (`+0x1838`), kept
    /// after the window closes, because the original draws the balloon while the timer runs.
    balloon_npc: Option<u16>,
    /// The field's view of the current drawing pass ([`Talk::set_view`]).
    view: Option<FieldView>,
    /// The tag the closed line carried, until [`Talk::take_service`].
    pending_service: Option<ServiceTag>,
    /// The end of a conversation a service has not finished with: trigger event 1 waits for
    /// the facility screen to close (the game's service runs inside the talk task).
    deferred: Option<Ended>,
    /// A conversation the player closed, until [`Talk::take_ended`].
    ended: Option<Ended>,
    fade: Option<[f32; 4]>,
    sounds: Vec<talk::Sound>,
    /// What the window did, one `(step, what)` per change (the probes of `docs/re/dialogue.md`
    /// § "Message pacing").
    #[cfg(test)]
    pub(crate) trace: Vec<(u32, String)>,
    #[cfg(test)]
    traced_state: &'static str,
}

impl Talk {
    /// Sets the message speed of the dialogues opened from now on; `Normal` is the original's
    /// rate and the default.
    pub fn set_text_speed(&mut self, speed: TextSpeed) {
        self.speed = speed;
    }

    fn dialogue(&self, mut dialogue: Dialogue) -> Dialogue {
        dialogue.set_speed(self.speed);
        dialogue
    }

    /// Loads the font and the window pieces of the ROM.
    pub fn set_rom(&mut self, rom: &[u8]) {
        match Assets::load(rom) {
            Ok(assets) => {
                self.assets = Some(assets);
                self.refresh_names();
            }
            Err(e) => {
                status(&format!("message window unavailable: {e:#}"));
                self.names = Names::default();
            }
        }
    }

    /// Ends a replaced game session without rebuilding the ROM assets or the GPU window.
    pub fn reset_session(&mut self) {
        let assets = self.assets.take();
        let gpu = self.gpu.take();
        *self = Self {
            assets,
            gpu,
            ..Self::default()
        };
    }

    /// The protagonist the message window substitutes (`<HERO>`, `<LEADER>`, `<ADDRESSEE>`)
    /// and whose sex the conditions read. `None` keeps the documented default identity ("Hero",
    /// a man), used without a save and by a new game.
    pub fn set_identity(&mut self, identity: Option<HeroIdentity>) {
        self.identity = identity;
        self.refresh_names();
    }

    /// Rebuilds the message names from the font and [`Talk::identity`]: the default until the
    /// ROM's font is loaded, and when the save carries no name.
    fn refresh_names(&mut self) {
        let Some(identity) = self.identity else {
            self.names = Names::default();
            return;
        };
        let names = self
            .assets
            .as_ref()
            .and_then(|assets| assets.font().ok())
            .map_or_else(Names::default, |font| identity.names(&font));
        self.names = names;
    }

    /// A map is loaded: its NPCs, and the map counters cleared.
    pub fn enter_map(&mut self, npcs: Vec<Talkable>, counts: &mut TalkCounts) {
        self.npcs = npcs;
        counts.enter_map();
        self.active = None;
        self.facing = None;
        self.turn_back = false;
        self.balloon = None;
        self.balloon_npc = None;
        self.abort_service();
    }

    /// Trigger action 0x8d built the map's cast again (`ov_017::021a2fa0`): its NPCs. The
    /// counters stay: whether that task clears the map counters is unknown.
    pub fn reload_npcs(&mut self, npcs: Vec<Talkable>) {
        self.npcs = npcs;
    }

    /// Action 0x7c: the NPC leaves the map (flag `0x8000` of its object): nobody to talk to.
    pub fn remove_npc(&mut self, npc: u16) {
        self.npcs.retain(|n| n.id != npc);
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// The partner the open conversation holds: `None` for an event message and once the
    /// window closes.
    fn talking_to(&self) -> Option<u16> {
        self.active.as_ref()?;
        self.conversation.map(|(npc, _)| npc)
    }

    /// Holds the partner still and turns it to the hero while the window is open.
    ///
    /// At the start of every message `Msg_ConsumeLeadingTags` (`0x0206a3d0`) gives the speaker
    /// the angle from it to the addressee and sets its turn flag `+0x19b6`, and
    /// `func_020656f4` hands that angle to the character (`func_02040af0`) while it turns at
    /// the field rate (`Character_SetFacing` `0x02033844` for the target,
    /// `Character_TurnToFacing` `0x02033720` for the step). `<N_TURN>` sets the saved angle
    /// `+0x183c` back and clears the flag: the message does not turn the speaker at all.
    /// `TalkTask_Update` (`ov_017::021b8e8c`) saves that angle with the partner's character id
    /// when the conversation opens. The window reports when the partner is given it back
    /// ([`Dialogue::take_turn_release`]): with `<R_TURN>` (`0xFF2C`) the requested angle
    /// becomes the saved one **in the middle of the message**, the rest of the page still
    /// typing, and with `<END_R_TURN>` (`0xFF02`) on the key that closes the line; a plain
    /// `<END>` gives nothing back, so the partner keeps the heading the turn left it at.
    ///
    /// Call once per step, after [`Talk::step`], with the field's scene and the hero; nothing
    /// happens without a window, and a script that moves the NPC
    /// ([`AnimatedScene::set_npc_angle`]) takes it back from the pivot.
    pub fn face_partner(&mut self, scene: &mut AnimatedScene, hero: Option<&Hero>) {
        let id = self.talking_to();
        if id.is_none() {
            if let Some(previous) = self.facing.take()
                && self.turn_back
            {
                scene.release_npc(previous);
            }
            self.turn_back = false;
            return;
        }
        // The conversation gave the heading back: the pivot turns back and no request is made
        // for the rest of it (`+0x19b6` no longer hands the hero's angle over).
        if self.turn_back {
            if let Some(previous) = self.facing.take() {
                scene.release_npc(previous);
            }
            return;
        }
        if self.facing.is_some() {
            return;
        }
        let no_turn = self
            .active
            .as_ref()
            .is_some_and(|dialogue| dialogue.view().tags.no_turn);
        let (Some(id), Some(hero), false) = (id, hero, no_turn) else {
            return;
        };
        if scene.turn_npc_toward(id, hero.position()) {
            self.facing = Some(id);
        }
    }

    /// Whether the action key was pressed since the last step.
    pub fn confirm_pending(&self) -> bool {
        self.pending.confirm
    }

    /// Records a button press for the next step.
    pub fn press(&mut self, button: Button) {
        // Every key of the message window leaves a wait (`MsgWin_PollWaitKey`); the prompt
        // alone reads A, B and the d-pad as its own.
        self.pending.advance = true;
        match button {
            Button::Confirm => self.pending.confirm = true,
            Button::Cancel => self.pending.cancel = true,
            Button::Up => self.pending.up = true,
            Button::Down => self.pending.down = true,
        }
    }

    /// Records a press of a key that only leaves a wait: DS X, L or R, and the left/right
    /// d-pad (a prompt reads only A, B and up/down).
    pub fn advance(&mut self) {
        self.pending.advance = true;
    }

    /// Forgets the pending presses (nothing is listening to them).
    pub fn discard_input(&mut self) {
        self.pending = Input::default();
    }

    /// The sounds the message window made since the last call (`talk::Sound`).
    pub fn take_sounds(&mut self) -> Vec<talk::Sound> {
        std::mem::take(&mut self.sounds)
    }

    /// One fixed step of the open dialogue.
    pub fn step(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        // `func_02065758`: the balloon's 60 ticks drop once per step, whether or not the
        // window is still open, and `+0x19b7` clears at zero.
        self.age_balloon();
        // Native 409 (`020657c8`): while the timed caption runs, the message tick is suspended:
        // the action key cannot advance or close it before the alpha envelope ends.
        let input = if self.event_message && self.caption_frozen {
            self.pending = Input::default();
            Input::default()
        } else {
            std::mem::take(&mut self.pending)
        };
        let partner = self.conversation.map(|(npc, _)| npc);
        let Some(dialogue) = &mut self.active else {
            return;
        };
        dialogue.step(input);
        // `<EXC>`/`<QES>` met in the page: `0x020659a0` raises the 60-tick timer of
        // `+0x1959` (the leading form's own is taken by `start`).
        let balloon = dialogue.take_balloon();
        // `<R_TURN>` met in the text or the key on an `<END_R_TURN>` line: the partner is given
        // the heading the conversation found, from the step the window reports it.
        if dialogue.take_turn_release() {
            self.turn_back = true;
            #[cfg(test)]
            self.trace
                .push((self.ticks, "partner turns back".to_string()));
        }
        #[cfg(test)]
        {
            // The probe names wait states 0, 1 and 2 with a cursor "arrow", the class the
            // message-flow probes were written against; the drawn triangle blinks and also
            // covers state 3 (`View::arrow`, `docs/re/dialogue.md` § "Message pacing").
            let probe = dialogue.probe_arrow();
            let view = dialogue.view();
            let state = if view.prompt.is_some() {
                "prompt"
            } else if probe {
                "arrow"
            } else {
                "-"
            };
            if state != self.traced_state {
                self.traced_state = state;
                self.trace
                    .push((self.ticks, format!("window shows: {state}")));
            }
        }
        let sounds = dialogue.take_sounds();
        // Native 413: `W+0x19b2 = 0` stops `MsgWin_PlayTextBlip` (`02045608`).
        if self.silent_caption {
            self.sounds.extend(
                sounds
                    .into_iter()
                    .filter(|s| !matches!(s, talk::Sound::Blip)),
            );
        } else {
            self.sounds.extend(sounds);
        }
        if let Some(outcome) = dialogue.outcome() {
            #[cfg(test)]
            self.trace.push((
                self.ticks,
                format!(
                    "window closed ({:?}, answer {:?})",
                    outcome.end, outcome.answer
                ),
            ));
            if self.event_message {
                // The script of the event polls the message (native 405).
                self.event_message = false;
                self.closed = true;
            } else {
                status(&format!(
                    "talk ended ({:?}, answer {:?})",
                    outcome.end, outcome.answer
                ));
                if let Some((npc, context)) = self.conversation.take() {
                    let ended = Ended {
                        npc,
                        context,
                        answer: outcome.answer,
                    };
                    // A line that names a facility opens it instead of ending the
                    // conversation: the game's talk task runs the service before it fires
                    // trigger event 1, so the end waits for the facility screen.
                    match self.service.take() {
                        Some(tag) => {
                            self.pending_service = Some(tag);
                            self.deferred = Some(ended);
                        }
                        None => self.ended = Some(ended),
                    }
                }
            }
            self.active = None;
        }
        // `func_020675fc` draws over the speaker of the conversation (`+0x1838`): the one the
        // step started with, even when this very step closed the window.
        if let (Some(balloon), Some(npc)) = (balloon, partner) {
            self.balloon = Some(balloon);
            self.balloon_npc = Some(npc);
        }
    }

    /// The conversation the player closed since the last call: the application fires the
    /// trigger event 1 with it (`Field::talk_ended`).
    pub fn take_ended(&mut self) -> Option<Ended> {
        self.ended.take()
    }

    /// The facility the talk line that just closed names, if any: the application opens it
    /// and calls [`Talk::finish_service`] once its screen closes.
    pub fn take_service(&mut self) -> Option<ServiceTag> {
        self.pending_service.take()
    }

    /// The facility screen is over: the closed conversation now ends as any other, so trigger
    /// event 1 fires with its NPC, context and answer.
    pub fn finish_service(&mut self) {
        if let Some(ended) = self.deferred.take() {
            self.ended = Some(ended);
        }
    }

    /// A map change or a replaced session interrupted the facility: the conversation it
    /// belonged to ends without firing trigger event 1 (the talk task is gone with the map).
    pub fn abort_service(&mut self) {
        self.pending_service = None;
        self.deferred = None;
        self.service = None;
    }

    /// Native 400: opens the window on an event's text (see [`Dialogue::for_event`]).
    pub fn show_event(&mut self, text: &[u8]) -> Result<()> {
        let Some(assets) = &self.assets else {
            bail!("no message window (the ROM resources failed to load)");
        };
        let font = assets.font()?;
        let script = talk::prepare(&font, text, &self.names, TEXT_WIDTH);
        self.active = Some(self.dialogue(Dialogue::for_event(script)));
        // A new message compiles again and `Msg_ConsumeLeadingTags` zeroes the balloon fields;
        // native 400 also clears the speaker (`+0x1838 = -1`), so a conversation's balloon
        // never shows over an event's text.
        self.balloon = None;
        self.balloon_npc = None;
        #[cfg(test)]
        self.trace.push((
            self.ticks,
            format!(
                "show {}",
                String::from_utf8_lossy(text).replace('\n', "\\n")
            ),
        ));
        self.event_message = true;
        self.frameless = false;
        self.caption_alpha = None;
        self.centered_caption = false;
        self.silent_caption = false;
        self.caption_frozen = false;
        self.closed = false;
        Ok(())
    }

    /// Native 410: the window of the current event message has no frame.
    pub fn hide_event_frame(&mut self) {
        self.frameless = true;
    }

    /// Native 409: alpha of the caption at this tick (one second in, `n` frames held, one
    /// second out; the game writes it to the caption layer's blend registers, `02066d00`).
    pub fn set_event_caption_alpha(&mut self, alpha: f32) {
        self.caption_alpha = Some(alpha.clamp(0.0, 1.0));
    }

    /// Native 411: centre the event text block vertically instead of using the box's own
    /// origin (`0206b6c8`).
    pub fn set_event_centered(&mut self, centered: bool) {
        self.centered_caption = centered;
    }

    /// Native 413: the event text types without its blip sound (`W+0x19b2 = 0`).
    pub fn set_event_silent(&mut self, silent: bool) {
        self.silent_caption = silent;
    }

    /// Natives 417/421: the message window and its text are drawn `offset` DS pixels from
    /// their place (`0206b804`, applied to the window rect at `02043b6c`).
    pub fn set_event_window_offset(&mut self, offset: [i32; 2]) {
        self.window_offset = offset;
    }

    /// Native 409: suspend the message tick (no input, no close) while the timed caption runs
    /// (`020657c8` returns 1 until the flag is cleared by 400/401).
    pub fn set_event_frozen(&mut self, frozen: bool) {
        self.caption_frozen = frozen;
    }

    /// Native 401: closes the event message without reporting it as read.
    pub fn close_event(&mut self) {
        #[cfg(test)]
        self.trace
            .push((self.ticks, "close_event (native 401)".into()));
        if self.event_message {
            self.active = None;
            self.event_message = false;
            self.caption_alpha = None;
            self.centered_caption = false;
            self.silent_caption = false;
            self.caption_frozen = false;
            // 417/421 are fields of the window, not of the text: a window drawn again starts
            // where the field put it until the script shakes it again.
            self.window_offset = [0, 0];
        }
    }

    /// Whether the player closed the event message since the last call.
    pub fn take_closed(&mut self) -> bool {
        std::mem::take(&mut self.closed)
    }

    /// The colour (straight alpha) the fade of the event draws over the screen, if any.
    pub fn set_fade(&mut self, color: Option<[f32; 4]>) {
        self.fade = color;
    }

    /// Whether the action key would find a conversation partner. The field examines a
    /// container only when this is false: a conversation keeps the key (the original's
    /// container is a "thing to examine" partner too, but its zone is not modelled).
    pub fn has_partner(&self, hero: &Hero) -> bool {
        talk::nearest(&self.npcs, hero.position(), hero.yaw()).is_some()
    }

    /// The action key with no dialogue open: talks to the NPC in front of the hero. Returns
    /// the events of a trigger that replaced the conversation (map changes, requests).
    pub fn begin(
        &mut self,
        rom: &[u8],
        map_code: &str,
        hero: &Hero,
        field: &mut Field,
        state: &mut State,
    ) -> Vec<Event> {
        let Some(target) = talk::nearest(&self.npcs, hero.position(), hero.yaw()) else {
            return Vec::new();
        };
        status(&format!(
            "talk: NPC {} (zone {})",
            target.npc, target.context
        ));
        state.zones = talk::zones_containing(&self.npcs, hero.position());
        let context = match field.talk(target.npc, target.context, state) {
            TalkTrigger::NoTrigger => Some(target.context),
            TalkTrigger::Unsupported(code) => {
                status(&format!("trigger condition {code:#x} is not modelled"));
                Some(target.context)
            }
            TalkTrigger::Fired(events) => {
                let mut rest = Vec::new();
                let mut context = None;
                for event in events {
                    match event {
                        Event::Request(Request::Talk { npc, context: c }) if npc == target.npc => {
                            context = Some(c);
                        }
                        other => rest.push(other),
                    }
                }
                if context.is_none() {
                    status("a trigger replaces the conversation");
                }
                if !rest.is_empty() {
                    return rest;
                }
                context
            }
        };
        if let Some(context) = context
            && let Err(e) = self.start(rom, map_code, state, target.npc, context)
        {
            status(&format!("talk: {e:#}"));
        }
        Vec::new()
    }

    /// Chooses the NPC's line (`Talk_SelectLine`) and opens the dialogue.
    pub fn start(
        &mut self,
        rom: &[u8],
        map_code: &str,
        state: &mut State,
        npc: u16,
        context: u16,
    ) -> Result<()> {
        let Some(assets) = &self.assets else {
            bail!("no message window (the ROM resources failed to load)");
        };
        let track = &state.tracks[state.track];
        let archive = archive_path(map_code, track.chapter)?;
        let member = format!("{npc:03}_{LANGUAGE}.bin");
        let rom = Rom::parse(rom).context("parsing the ROM")?;
        let archive_data =
            Gpc2::parse(rom.file_by_path(&archive)?).context("parsing the archive")?;
        let Some(entry) = archive_data.find(&member) else {
            bail!("NPC {npc} has no dialogue file {member}");
        };
        let data = archive_data.member(entry)?;
        let table = Table::parse(&data).context("not a record table")?;
        let lines = dialogue::parse(&table)?;
        let quests = |q: i32| state.quests.get(&q).copied().unwrap_or(0);
        let talk_state = TalkState {
            step: i32::from(track.step),
            substep: i32::from(track.substep),
            context: i32::from(context),
            talk_count: state.talk_counts.get(npc),
            daytime: state.time_of_day != 0,
            session: Session {
                hero_alive: true,
                ..Session::default()
            },
            quest: &quests,
        };
        let Some(line) = dialogue::select(&lines, &talk_state) else {
            bail!("NPC {npc} has nothing to say (context {context})");
        };
        let Some(text) = lines[line].text else {
            bail!("record {line} of NPC {npc} has no text");
        };
        let font = assets.font()?;
        let script = talk::prepare(&font, text, &self.names, TEXT_WIDTH);
        status(&format!(
            "talk: {archive}/{member} record {line}, context {context}"
        ));
        state.talk_counts.record(npc);
        self.conversation = Some((npc, context));
        // The heading is only given back when the window reports it (`<R_TURN>`,
        // `<END_R_TURN>`); the compiled script decides, so a tag of a branch the answer did
        // not take cannot release it.
        self.turn_back = false;
        self.service = service_tag(text);
        // A new conversation: `TalkTask_Update` saves the speaker and its heading, and the
        // leading `<EXC>`/`<QES>` of the line (`0x0206a3d0`) raises its balloon with it.
        self.balloon = None;
        self.balloon_npc = None;
        let mut dialogue = self.dialogue(Dialogue::new(script));
        if let Some(balloon) = dialogue.take_balloon() {
            self.raise_balloon(balloon);
        }
        self.active = Some(dialogue);
        self.caption_alpha = None;
        self.centered_caption = false;
        self.silent_caption = false;
        self.caption_frozen = false;
        Ok(())
    }

    /// The quads of the balloon over the speaker, of the window and of the fade over them on
    /// the 256×192 screen. The balloon outlives the window: the original draws it for its 60
    /// ticks while the speaker (`+0x1838`) is known.
    pub fn quads(&self) -> Vec<Quad> {
        let mut out = Vec::new();
        if let Some(view) = self.view
            && let Some(balloon) = self.balloon_quad(view, SCREEN)
        {
            out.push(balloon);
        }
        out.extend(self.window_and_fade_quads());
        out
    }

    /// The window's quads and the full-screen fade, without the balloon: the DS-space pieces
    /// the enhanced HUD re-anchors and rescales ([`crate::hud::frame`]).
    fn window_and_fade_quads(&self) -> Vec<Quad> {
        let mut out = self.window_quads();
        if let (Some(assets), Some(color)) = (&self.assets, self.fade) {
            out.push(assets.atlas.solid([0, 0, 256, 192], color));
        }
        out
    }

    /// Records the camera the field is drawn with, for the balloon's projection
    /// (`func_0202ec84`): call before drawing a frame ([`Talk::draw`] and [`Talk::draw_hud`]).
    /// `reactive` is the enhanced-reactive layout, whose field of view follows the window's
    /// aspect ([`Camera::reactive_view_proj`]).
    pub fn set_view(&mut self, camera: &Camera, reactive: bool) {
        self.view = Some(FieldView {
            camera: *camera,
            reactive,
        });
    }

    /// Raises the balloon over the conversation's partner (`func_020675fc` needs the
    /// speaker's character `+0x1838`; a message of an event has none).
    fn raise_balloon(&mut self, balloon: Balloon) {
        let Some((npc, _)) = self.conversation else {
            return;
        };
        self.balloon = Some(balloon);
        self.balloon_npc = Some(npc);
    }

    /// Counts the balloon down by one fixed step (`func_02065758`: `+0x1959` by the tick
    /// delta) and clears it at zero (`+0x19b7`). The countdown only runs while the window is
    /// open: `MsgWin_Update` (`0x020659a0`) calls it inside `if (W+0x998 != 0)`
    /// (`arm9.c:82465`), so a closed window freezes it and the next message zeroes it.
    fn age_balloon(&mut self) {
        if self.active.is_none() {
            return;
        }
        let Some(balloon) = &mut self.balloon else {
            return;
        };
        balloon.ticks = balloon.ticks.saturating_sub(1);
        if balloon.ticks == 0 {
            self.balloon = None;
            self.balloon_npc = None;
        }
    }

    /// The screen point the balloon's sheet corner takes (`0x020675fc`): the partner's
    /// position through the field projection (`func_0202ec84` →
    /// `ConvertWorldToScreenCoordinates`), then [`BALLOON_OFFSET`] DS pixels. `rect` is the
    /// field's rectangle in the pass's canvas, `[x, y, w, h]`.
    ///
    /// The DS pass uses the original's viewport `(0, 0, 255, 191)` and its rounding:
    /// `x = round(normX × 255)`, `y = 191 − round(normY × 191)` with `norm = (ndc + 1) / 2`
    /// (`GeometryFifo.cpp`, `ConvertWorldToScreenCoordinates`). The enhanced canvas has no DS
    /// viewport: its rectangle is the same proportion without the 255/191 rounding
    /// **[hypothesis]**.
    fn balloon_corner(&self, view: FieldView, rect: [f32; 4]) -> Option<(f32, f32)> {
        self.balloon?;
        let npc = self.balloon_npc?;
        let at = self.npcs.iter().find(|n| n.id == npc)?.position;
        let aspect = rect[2] / rect[3].max(f32::EPSILON);
        let view_proj = if view.reactive {
            view.camera.reactive_view_proj(aspect)
        } else {
            view.camera.view_proj(aspect)
        };
        let clip = view_proj
            * glam::Vec4::new(
                at[0] as f32 / 4096.0,
                at[1] as f32 / 4096.0,
                at[2] as f32 / 4096.0,
                1.0,
            );
        // Behind the eye or past a clip plane: the original's `SPRSprite` draw would be
        // culled by the projection too.
        if clip.w <= 0.0 || !(0.0..=1.0).contains(&(clip.z / clip.w)) {
            return None;
        }
        let ndc = glam::Vec2::new(clip.x, clip.y) / clip.w;
        let scale = rect[3] / 192.0;
        let (x, y) = if rect == SCREEN {
            let norm = (ndc + glam::Vec2::ONE) * 0.5;
            ((norm.x * 255.0).round(), 191.0 - (norm.y * 191.0).round())
        } else {
            // Canvas pixels back in the DS-pixel scale of the sheet.
            (
                (ndc.x + 1.0) * 0.5 * rect[2] / scale,
                (1.0 - ndc.y) * 0.5 * rect[3] / scale,
            )
        };
        Some((
            rect[0] + x * scale + BALLOON_OFFSET[0] * scale,
            rect[1] + y * scale + BALLOON_OFFSET[1] * scale,
        ))
    }

    /// The balloon over the partner for this pass, if it is up. `MsgWin_DrawGlyphs`
    /// (`0x02066d00`) calls `func_020675fc` inside `if (W+0x998 != 0)` (`arm9.c:83194`): with
    /// the window closed nothing is drawn.
    fn balloon_quad(&self, view: FieldView, rect: [f32; 4]) -> Option<Quad> {
        self.active.as_ref()?;
        let kind = self.balloon?.kind;
        let assets = self.assets.as_ref()?;
        let slot = match kind {
            BalloonKind::Exclamation => 0,
            BalloonKind::Question => 1,
        };
        let corner = self.balloon_corner(view, rect)?;
        assets.atlas.sprite_quad(slot, corner, rect[3] / 192.0)
    }

    /// The quads of the window, empty when no dialogue is open.
    fn window_quads(&self) -> Vec<Quad> {
        let (Some(assets), Some(dialogue)) = (&self.assets, &self.active) else {
            return Vec::new();
        };
        let atlas = &assets.atlas;
        let view = dialogue.view();
        let tags = view.tags;
        let mut window = tags.rect.unwrap_or(WINDOW);
        if tags.centered && tags.rect.is_none() {
            window[1] = (192 - window[3]) / 2;
        }
        let color = tags.color.map_or([1.0; 4], tint);
        let mut out = Vec::new();
        if !tags.frameless && !self.frameless {
            atlas.window(window, FILL, &mut out);
        }
        if let Some(plate) = view.name {
            let advance = plate.advance;
            let width = advance + 26 - (advance & 7);
            let rect = [
                window[0] + 6,
                window[1] - NAME_HEIGHT + 4,
                width,
                NAME_HEIGHT,
            ];
            atlas.window(rect, FILL, &mut out);
            let x = rect[0] + (width - advance) / 2;
            let y = rect[1] + (NAME_HEIGHT - atlas.font_height() as i32) / 2;
            atlas.text(&plate.glyphs, x, y, [1.0; 4], &mut out);
        }
        for (i, line) in view.lines.iter().enumerate() {
            let mut x = window[0] + TEXT_MARGIN[0];
            if tags.centered {
                x = window[0] + (window[2] - atlas.line_width(line)) / 2;
            }
            // Native 411 with 410 (`0206b6c8`): the window is moved to
            // `(192 − (lines − 1) × 20 − 8) / 2 − 16` and its text starts at the usual margin.
            // Without 410 the per-frame box geometry reset would overwrite it (`020439b0`).
            let y = if self.event_message && self.frameless && self.centered_caption {
                let lines = view.lines.len() as i32;
                (192 - (lines - 1) * LINE_HEIGHT - 8) / 2 - 16
                    + TEXT_MARGIN[1]
                    + i as i32 * LINE_HEIGHT
            } else {
                window[1] + TEXT_MARGIN[1] + i as i32 * LINE_HEIGHT
            };
            atlas.text(line, x, y, color, &mut out);
        }
        if view.arrow {
            let (w, h) = atlas.arrow_size();
            let bounce = (self.ticks / 20 % 2) as i32 * 2;
            // The arrow sprite and this corner position are still **[hypothesis]** choices of
            // `dq9-app` (`docs/re/dialogue.md`, § "Message window"): the original draws a
            // smaller triangle at the window's centre (128, 183) on the bottom screen.
            atlas.arrow(
                window[0] + window[2] - 8 - w as i32,
                window[1] + window[3] - 6 - h as i32 + bounce,
                &mut out,
            );
        }
        if let Some(prompt) = view.prompt {
            let labels = assets.labels(prompt.kind);
            let width = labels.iter().map(|l| l.width).max().unwrap_or(0) + 24;
            let height = 2 * LINE_HEIGHT + 12;
            let rect = [
                window[0] + window[2] - width - 6,
                window[1] - height - 2,
                width,
                height,
            ];
            atlas.window(rect, FILL, &mut out);
            for (i, label) in labels.iter().enumerate() {
                let y = rect[1] + 6 + i as i32 * LINE_HEIGHT;
                if usize::from(prompt.cursor) == i {
                    out.push(
                        atlas.solid([rect[0] + 4, y - 2, rect[2] - 8, LINE_HEIGHT - 2], SELECTED),
                    );
                }
                atlas.text(&label.glyphs, rect[0] + 12, y + 2, [1.0; 4], &mut out);
            }
        }
        // Native 409: the caption layer's alpha (`02066d00` writes the blend registers from
        // `W+0x9b0 >> 16`, 0 to 31); fade the text and its window with it.
        if self.event_message
            && let Some(alpha) = self.caption_alpha
        {
            for quad in &mut out {
                quad.color[3] *= alpha;
            }
        }
        // Natives 417/421: `0206b804` offsets the window rect (`02043b6c`), so the frame, the
        // name plate, the text and the arrow move together.
        let [dx, dy] = self.window_offset;
        if dx != 0 || dy != 0 {
            for quad in &mut out {
                quad.dst[0] += dx as f32;
                quad.dst[1] += dy as f32;
            }
        }
        out
    }

    /// Draws the window over `target` in `region`, the top screen (`screens.rs`). Nothing is
    /// drawn when no dialogue is open.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        region: Viewport,
    ) {
        let quads = self.quads();
        if quads.is_empty() {
            return;
        }
        let Some(assets) = &self.assets else {
            return;
        };
        if self.gpu.as_ref().is_none_or(|(f, _)| *f != format) {
            self.gpu = Some((format, Ui::new(device, queue, format, &assets.atlas)));
        }
        let Some((_, ui)) = &mut self.gpu else {
            return;
        };
        ui.draw(device, queue, encoder, target, region, &quads);
    }

    /// Draws the enhanced interface: the HUD ([`crate::hud`]) and the message window laid out
    /// in `canvas` (960×540 for the 16:9 layout, the window's aspect for the reactive one).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_hud(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        region: Viewport,
        canvas: crate::hud::Canvas,
        info: &crate::hud::HudInfo<'_>,
    ) {
        let quads = {
            let dialogue = self.window_and_fade_quads();
            let Some(assets) = &self.assets else {
                return;
            };
            let font = match assets.font() {
                Ok(font) => font,
                Err(e) => {
                    status(&format!("message font unavailable: {e:#}"));
                    return;
                }
            };
            let layout = if self.frameless || self.centered_caption {
                crate::hud::DialogueLayout::center(canvas)
            } else {
                crate::hud::DialogueLayout::bottom(canvas)
            };
            let mut out = Vec::new();
            // The balloon stays on the field, which the HUD's canvas covers: it is not
            // re-anchored with the window (`func_020675fc` projects the speaker, not the box).
            let field = [0.0, 0.0, canvas.width as f32, canvas.height as f32];
            if let Some(view) = self.view
                && let Some(balloon) = self.balloon_quad(view, field)
            {
                out.push(balloon);
            }
            out.extend(crate::hud::frame(
                &assets.atlas,
                &font,
                info,
                &dialogue,
                layout,
                canvas,
            ));
            out
        };
        if self.gpu.as_ref().is_none_or(|(f, _)| *f != format) {
            let Some(assets) = &self.assets else {
                return;
            };
            self.gpu = Some((format, Ui::new(device, queue, format, &assets.atlas)));
        }
        let Some((_, ui)) = &mut self.gpu else {
            return;
        };
        ui.draw_in(
            device,
            queue,
            encoder,
            target,
            region,
            (canvas.width as f32, canvas.height as f32),
            &quads,
        );
    }

    /// The interface's atlas and message font, for the windows that are not conversations
    /// (the battle's, `battle/ui.rs`). `None` when the ROM resources did not load.
    pub(crate) fn with_ui<R>(&self, f: impl FnOnce(&Atlas, &Font<'_>) -> R) -> Option<R> {
        let assets = self.assets.as_ref()?;
        let font = assets.font().ok()?;
        Some(f(&assets.atlas, &font))
    }

    /// The protagonist's name as the font draws it (`Hero` until a name is known).
    pub(crate) fn hero_name(&self) -> Vec<u8> {
        self.names.hero.clone()
    }

    /// Draws each set of quads over its screen of `target`, sharing the atlas (and the GPU
    /// state) of the message window.
    pub(crate) fn draw_screens(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        screens: &[(Viewport, Vec<Quad>)],
    ) {
        let Some(assets) = &self.assets else {
            return;
        };
        if self.gpu.as_ref().is_none_or(|(f, _)| *f != format) {
            self.gpu = Some((format, Ui::new(device, queue, format, &assets.atlas)));
        }
        let Some((_, ui)) = &mut self.gpu else {
            return;
        };
        for (region, quads) in screens {
            ui.draw(device, queue, encoder, target, *region, quads);
        }
    }
}

/// Where the hero stands to talk to an NPC with a given context (`Target`): what an autopilot
/// needs to have the conversation without walking there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partner {
    pub npc: u16,
    pub context: u16,
    pub position: glam::Vec3,
    pub yaw: f32,
}

impl Talk {
    /// The partner `Talk_NearestNpc` would pick at `position`/`yaw`, as [`Talk::begin`] does:
    /// what an autopilot needs to know before pressing the action key.
    pub fn nearest(&self, position: glam::Vec3, yaw: f32) -> Option<talk::Target> {
        talk::nearest(&self.npcs, position, yaw)
    }

    /// The conversations the map offers: for each NPC, a standing point in each of its talk
    /// zones and one free spot beside it, each checked with the game's partner search
    /// ([`talk::nearest`]).
    pub fn partners(&self) -> Vec<Partner> {
        use glam::Vec3;
        let mut out = Vec::new();
        for npc in &self.npcs {
            let at = Vec3::from_array(npc.position.map(|v| v as f32 / 4096.0));
            let zones = npc.zones.iter().map(|z| {
                let [x_max, z_max, x_min, z_min] = z.rect;
                let centre = Vec3::new(
                    ((x_max + x_min) / 2) as f32 / 4096.0,
                    at.y,
                    ((z_max + z_min) / 2) as f32 / 4096.0,
                );
                (centre, z.label as u16)
            });
            let free = [(0.0, 1.2), (0.0, -1.2), (1.2, 0.0), (-1.2, 0.0)]
                .map(|(dx, dz)| (at + Vec3::new(dx, 0.0, dz), 0u16));
            let mut free_found = false;
            for (position, context) in zones.chain(free) {
                if context == 0 && free_found {
                    continue;
                }
                let yaw =
                    dq9_core::field::yaw_of_angle((at.x - position.x).atan2(at.z - position.z));
                let target = talk::Target {
                    npc: npc.id,
                    context,
                };
                if talk::nearest(&self.npcs, position, yaw) == Some(target) {
                    free_found |= context == 0;
                    out.push(Partner {
                        npc: npc.id,
                        context,
                        position,
                        yaw,
                    });
                }
            }
        }
        out
    }

    /// What the conversation would do to the story: the events of the talk trigger and of the
    /// trigger that closes it (answering the first choice of a prompt), and the state after.
    /// Works on copies; nothing of the game changes.
    pub fn simulate(&self, partner: &Partner, field: &Field, state: &State) -> (Vec<Event>, State) {
        let (mut field, mut state) = (field.clone(), state.clone());
        state.zones = talk::zones_containing(&self.npcs, partner.position);
        let mut events = Vec::new();
        let context = match field.talk(partner.npc, partner.context, &mut state) {
            TalkTrigger::NoTrigger | TalkTrigger::Unsupported(_) => partner.context,
            TalkTrigger::Fired(fired) => {
                let mut context = None;
                for event in fired {
                    match event {
                        Event::Request(Request::Talk { npc, context: c }) if npc == partner.npc => {
                            context = Some(c);
                        }
                        other => events.push(other),
                    }
                }
                match context {
                    Some(context) if events.is_empty() => context,
                    _ => return (events, state),
                }
            }
        };
        events.extend(field.talk_ended(partner.npc, context, Some(0), &mut state));
        (events, state)
    }
}

/// The buttons of the message window. Every one of them leaves a wait on its press edge; A
/// and B also answer a prompt, and the d-pad moves its cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Confirm,
    Cancel,
    Up,
    Down,
}

#[cfg(test)]
mod wait_key_tests {
    use super::{Button, Input, Talk};

    /// Every key of the message window leaves a wait on its press edge (`MsgWin_PollWaitKey`
    /// `0x020440dc`); DS X, L and R do nothing else.
    #[test]
    fn every_window_key_sets_the_advance_edge() {
        for button in [Button::Confirm, Button::Cancel, Button::Up, Button::Down] {
            let mut talk = Talk::default();
            talk.press(button);
            assert!(talk.pending.advance, "{button:?} leaves a wait");
        }
        let mut talk = Talk::default();
        talk.advance();
        assert_eq!(
            talk.pending,
            Input {
                advance: true,
                ..Input::default()
            },
            "X, L and R press no prompt key"
        );
    }
}

#[cfg(test)]
mod turn_tests {
    use dq9_core::talk::{self, Script};
    use dq9_formats::font::{Font, FontId};

    use super::{Button, Dialogue, Names, Talk};

    /// Font of four 4-pixel glyphs `a b c d` (indices 0 to 3), advance 5, spaces 4: the same
    /// shape the dialogue's own tests lay out (`crates/dq9-core/src/talk/tests.rs`).
    fn font_files() -> (Vec<u8>, Vec<u8>) {
        let glyphs: [&[u8]; 4] = [b"a", b"b", b"c", b"d"];
        let table = 0x18;
        let strings = table + glyphs.len() * 8;
        let mut info = b"1.1\0".to_vec();
        for v in [glyphs.len(), 0, table, table, strings] {
            info.extend((v as u32).to_le_bytes());
        }
        let mut pool: Vec<u8> = Vec::new();
        for text in glyphs {
            info.extend(((strings + pool.len()) as u32).to_le_bytes());
            info.extend([4, text.len() as u8, 0, 0]);
            pool.extend(text);
        }
        info.extend(pool);
        let mut data = b"1.0\0".to_vec();
        for v in [8u16, 1] {
            data.extend(v.to_le_bytes());
        }
        data.extend(1u32.to_le_bytes());
        data.extend(16u32.to_le_bytes());
        data.push(0);
        (info, data)
    }

    pub(super) fn script(text: &[u8]) -> Script {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        talk::prepare(&font, text, &Names::default(), 228)
    }

    /// A window open on NPC 7, its line compiled from `text`: no map, no assets.
    fn window(text: &[u8]) -> Talk {
        Talk {
            conversation: Some((7, 0)),
            active: Some(Dialogue::new(script(text))),
            ..Talk::default()
        }
    }

    fn play(talk: &mut Talk, steps: usize) {
        for _ in 0..steps {
            talk.step();
        }
    }

    /// `<R_TURN>` mid-line: the application takes the report on the step the window gives it,
    /// keeps the window open (the rest of the page still types) and latches the conversation,
    /// so [`Talk::face_partner`] makes no new request for it.
    #[test]
    fn a_mid_message_release_is_taken_while_the_window_keeps_showing() {
        let mut talk = window(b"ab<R_TURN>cd<END>");
        play(&mut talk, 2);
        assert!(!talk.turn_back, "nothing before the code");
        play(&mut talk, 1);
        assert!(talk.turn_back, "the code's step reports the release");
        assert!(talk.is_active(), "the window stays open");
        play(&mut talk, 2);
        assert_eq!(
            talk.active.as_ref().unwrap().view().lines[0].len(),
            4,
            "the rest of the page typed on after the release"
        );
        assert!(talk.turn_back, "the latch holds for the conversation");
    }

    /// `<END_R_TURN>`: nothing while the text stays; the action key that closes the line
    /// reports it, then the window closes as an `<END_R_TURN>` end.
    #[test]
    fn an_end_r_turn_releases_at_the_key_and_a_plain_end_never_does() {
        let mut talk = window(b"ab<END_R_TURN>");
        play(&mut talk, 20);
        assert!(talk.is_active());
        assert!(!talk.turn_back, "nothing gives back while the text stays");
        talk.press(Button::Confirm);
        talk.step();
        assert!(talk.turn_back, "the key reports the release");
        play(&mut talk, dq9_core::talk::CLOSE_STEPS as usize);
        assert!(!talk.is_active(), "the window closed");

        let mut talk = window(b"ab<END>");
        play(&mut talk, 20);
        talk.press(Button::Confirm);
        talk.step();
        play(&mut talk, dq9_core::talk::CLOSE_STEPS as usize);
        assert!(!talk.is_active());
        assert!(!talk.turn_back, "a plain `<END>` keeps the heading");
    }
}

/// The balloons `<EXC>`/`<QES>` of the message window (`func_020675fc`, `docs/re/dialogue.md`
/// § Balloons): the 60-step timer, the projection of the speaker and the drawn sheet.
#[cfg(test)]
mod balloon_tests {
    use dq9_core::talk::{Balloon, BalloonKind, Dialogue, Talkable};
    use dq9_render::Camera;
    use dq9_render::camera::CameraMode;
    use glam::Vec3;

    use super::turn_tests::script;
    use super::{FieldView, SCREEN, Talk};

    /// The partner of [`Talkable::id`] 7, its feet at `position` (`fx32`).
    fn partner(position: [i32; 3]) -> Talkable {
        Talkable {
            id: 7,
            kind: 0,
            position,
            zones: Vec::new(),
            extra: None,
        }
    }

    /// A window open on NPC 7, its line compiled from `text`, the balloon the line's leading
    /// tag asks for taken like [`Talk::start`] does. No ROM assets (no atlas).
    fn window(text: &[u8]) -> Talk {
        let mut dialogue = Dialogue::new(script(text));
        let balloon = dialogue.take_balloon().expect("a leading balloon tag");
        Talk {
            npcs: vec![partner([0, 0, 0])],
            conversation: Some((7, 0)),
            balloon: Some(balloon),
            balloon_npc: Some(7),
            active: Some(dialogue),
            ..Talk::default()
        }
    }

    /// A camera looking straight down `-z`, its target at the world origin: `(-12, -62)` from
    /// the point it projects is [`super::BALLOON_OFFSET`].
    fn camera() -> Camera {
        Camera {
            mode: CameraMode::Fly,
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fovy: dq9_render::camera::DS_FOVY,
            near: 1.0,
            far: 400.0,
            roll: 0.0,
        }
    }

    fn play(talk: &mut Talk, steps: usize) {
        for _ in 0..steps {
            talk.step();
        }
    }

    /// `func_02065758`: the balloon drops one tick per fixed step and clears at zero; the
    /// page typing under it never raises it again, and the window is not held by it.
    #[test]
    fn a_balloon_lasts_sixty_steps_and_the_line_does_not_restart_it() {
        let mut talk = window(b"<EXC>ab<END>");
        assert_eq!(
            talk.balloon,
            Some(Balloon::new(BalloonKind::Exclamation)),
            "raised at the open"
        );
        play(&mut talk, 1);
        assert_eq!(talk.balloon.map(|b| b.ticks), Some(59));
        play(&mut talk, 58);
        assert_eq!(talk.balloon.map(|b| b.ticks), Some(1), "the last tick");
        assert!(talk.is_active(), "the line still shows (an `<END>` waits)");
        play(&mut talk, 1);
        assert_eq!(talk.balloon, None, "cleared at zero (+0x19b7)");
        assert_eq!(talk.balloon_npc, None);
        // The window still ends on its key with the balloon gone.
        talk.press(super::Button::Confirm);
        talk.step();
        play(&mut talk, dq9_core::talk::CLOSE_STEPS as usize);
        assert!(!talk.is_active(), "the balloon does not block the window");
    }

    /// The in-message form raises it on the step the code is met, 60 ticks again.
    #[test]
    fn the_interpreter_form_raises_it_at_its_step() {
        let mut talk = Talk {
            npcs: vec![partner([0, 0, 0])],
            conversation: Some((7, 0)),
            active: Some(Dialogue::new(script(b"<QES>ab<QES>cd<END>"))),
            ..Talk::default()
        };
        // The leading one is only taken by `start`; the page's own arrives with the code.
        play(&mut talk, 3);
        assert_eq!(
            talk.balloon,
            Some(Balloon::new(BalloonKind::Question)),
            "the code in the page"
        );
    }

    /// `MsgWin_Update` counts the timer inside `if (W+0x998 != 0)` (`0x020659a0`,
    /// `arm9.c:82465`): a closed window freezes it, and `MsgWin_DrawGlyphs` draws nothing over
    /// the field. The next message zeroes both fields (`0x0206a3d0`, `arm9.c:86527`).
    #[test]
    fn a_closed_window_freezes_the_balloon_and_the_next_message_clears_it() {
        let Some(rom) = testrom::load() else { return };
        let mut talk = window(b"<EXC>ab<END>");
        talk.set_rom(rom);
        talk.set_view(&camera(), false);
        play(&mut talk, 3);
        let view = talk.view.unwrap();
        assert!(
            talk.balloon_quad(view, SCREEN).is_some(),
            "drawn while open"
        );
        // The key on the `<END>` and the six steps of the close.
        talk.press(super::Button::Confirm);
        talk.step();
        play(&mut talk, dq9_core::talk::CLOSE_STEPS as usize);
        assert!(!talk.is_active(), "the window closed");
        let frozen = talk.balloon.expect("the field still holds it");
        assert_eq!(
            talk.balloon_quad(view, SCREEN),
            None,
            "nothing is drawn over a closed window"
        );
        play(&mut talk, 10);
        assert_eq!(
            talk.balloon,
            Some(frozen),
            "the countdown does not run without a window"
        );
        // A new event message compiles again and zeroes the fields.
        talk.show_event(b"caption").unwrap();
        assert_eq!(talk.balloon, None);
        assert_eq!(talk.balloon_npc, None);
    }

    /// A page break keeps the balloon: `Msg_Compile` runs once per line (`MsgWin_Show`
    /// `0x0204501c`), so `Msg_ConsumeLeadingTags` does not zero `+0x1959` between the pages of
    /// one message; the next message does (`start`).
    #[test]
    fn a_page_break_keeps_the_balloon() {
        let mut dialogue = Dialogue::new(script(b"<EXC>ab<PAGE>cd<END>"));
        let balloon = dialogue.take_balloon().expect("the leading tag");
        let mut talk = Talk {
            npcs: vec![partner([0, 0, 0])],
            conversation: Some((7, 0)),
            active: Some(dialogue),
            balloon: Some(balloon),
            balloon_npc: Some(7),
            ..Talk::default()
        };
        play(&mut talk, 3);
        assert!(talk.balloon.is_some(), "page 1 holds it");
        talk.press(super::Button::Confirm);
        talk.step();
        assert!(talk.balloon.is_some(), "the page break does not clear it");
        assert_eq!(talk.balloon_npc, Some(7));
    }

    /// The projection and the `(−12, −62)` offset on a synthetic camera: the sheet's corner is
    /// that far from the point the field camera projects the partner to.
    #[test]
    fn the_balloon_sits_at_the_offset_from_the_partners_projection() {
        let view = FieldView {
            camera: camera(),
            reactive: false,
        };
        // Feet on the camera's axis at 10 units: the point projects to the centre of the
        // original's `255 × 191` viewport, `(128, 95)`, then the `(−12, −62)` offset.
        let mut talk = window(b"<EXC>ab<END>");
        let corner = talk.balloon_corner(view, SCREEN).unwrap();
        assert_eq!(corner, (116.0, 33.0));

        // One unit to the right: ndc.x = 1 / (10 × tan 15° × 4/3), norm.x × 255 rounded, the
        // offset applied after.
        talk.npcs = vec![partner([4096, 0, 0])];
        let (x, y) = talk.balloon_corner(view, SCREEN).unwrap();
        let ndc = 1.0 / (10.0 * (std::f32::consts::PI / 12.0).tan() * 4.0 / 3.0);
        let expected = (((ndc + 1.0) * 0.5) * 255.0).round() - 12.0;
        assert_eq!((x, y), (expected, 33.0));

        // The wide canvas scales the sheet with the field's 192 lines and widens the projection
        // to the canvas's own aspect (16:9 here).
        let wide = [0.0, 0.0, 960.0, 540.0];
        let wide_ndc = 1.0 / (10.0 * (std::f32::consts::PI / 12.0).tan() * 960.0 / 540.0);
        let (x, y) = talk.balloon_corner(view, wide).unwrap();
        assert!(
            (x - (480.0 + wide_ndc * 480.0 - 12.0 * 540.0 / 192.0)).abs() < 0.01,
            "{x}"
        );
        assert!((y - (270.0 - 62.0 * 540.0 / 192.0)).abs() < 1e-3);

        // Behind the eye: nothing to draw.
        talk.npcs = vec![partner([0, 0, 20 * 4096])];
        assert_eq!(talk.balloon_corner(view, SCREEN), None);
        // No balloon up: no quad either.
        talk.npcs = vec![partner([0, 0, 0])];
        talk.balloon = None;
        assert_eq!(talk.balloon_quad(view, SCREEN), None);
    }

    /// The sheet drawn by the window pass: `ev_mark.spr` for `<EXC>`, `field_qu.spr` for
    /// `<QES>`, at the projected corner and its own size (1 texel = 1 DS pixel).
    #[test]
    fn the_drawn_sheet_is_the_one_of_its_tag() {
        let Some(rom) = testrom::load() else { return };
        let mut talk = window(b"<EXC>ab<END>");
        talk.set_rom(rom);
        talk.set_view(&camera(), false);
        let view = talk.view.unwrap();
        let exclamation = talk.balloon_quad(view, SCREEN).unwrap();
        assert_eq!(
            exclamation.dst,
            [
                116.0,
                33.0,
                exclamation.src[2] as f32,
                exclamation.src[3] as f32
            ]
        );
        // `ev_mark.spr` and `field_qu.spr` are different sheets.
        talk.balloon = Some(Balloon::new(BalloonKind::Question));
        let question = talk.balloon_quad(view, SCREEN).unwrap();
        assert_ne!(question.src, exclamation.src);
        assert!(exclamation.src[2] > 0 && exclamation.src[3] > 0);
        assert!(question.src[2] > 0 && question.src[3] > 0);
    }

    /// Renders one window pass into a 256×192 RGBA image over black: the frame
    /// `--screens port` composes over the field, without the 3D view (a synthetic camera
    /// stands in for it).
    #[cfg(not(target_arch = "wasm32"))]
    fn gpu() -> (wgpu::Device, wgpu::Queue) {
        pollster::block_on(async {
            let instance = crate::gpu::instance().await;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .expect("a GPU adapter for the balloon capture");
            crate::gpu::device(&adapter).await.expect("a GPU device")
        })
    }

    /// One pass of `talk` with `device` into a `size` RGBA image over black: the DS window
    /// pass, or (`hud`) the enhanced 960×540 one. The cached [`super::Ui`] belongs to the
    /// device, so every frame of one capture shares it.
    #[cfg(not(target_arch = "wasm32"))]
    fn render(
        talk: &mut Talk,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (width, height): (u32, u32),
        hud: bool,
    ) -> Vec<u8> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("balloon capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let row = (4 * width).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("balloon readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("balloon capture"),
        });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        let region = super::Viewport {
            x: 0,
            y: 0,
            width,
            height,
        };
        if hud {
            let party = dq9_core::party::Party::new_game();
            let info = crate::hud::HudInfo {
                title: "Angel Falls",
                code: "M01",
                time: "Day",
                party: Some(&party),
                glass: false,
            };
            talk.draw_hud(
                device,
                queue,
                wgpu::TextureFormat::Rgba8Unorm,
                &mut encoder,
                &view,
                region,
                crate::hud::Canvas::ENHANCED,
                &info,
            );
        } else {
            talk.draw(
                device,
                queue,
                wgpu::TextureFormat::Rgba8Unorm,
                &mut encoder,
                &view,
                region,
            );
        }
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        rx.recv().unwrap().unwrap();
        let data = slice.get_mapped_range().unwrap();
        let mut out = Vec::with_capacity((4 * width * height) as usize);
        for r in 0..height as usize {
            out.extend_from_slice(&data[r * row as usize..r * row as usize + 4 * width as usize]);
        }
        drop(data);
        buffer.unmap();
        out
    }

    /// The capture of `build/evidence/v3-balloons/`: the `!` and the `?` over the partner in
    /// the window pass, then the frame the balloon had left once its 60 steps have run (the
    /// page's own text is still there). Both are the same window without the sheet.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_capture_shows_the_balloon_for_sixty_steps_and_nothing_after() {
        let Some(rom) = testrom::load() else { return };
        let (device, queue) = gpu();
        let evidence = std::env::var_os("DQ9_EVIDENCE_DIR").map(std::path::PathBuf::from);
        let save = |name: &str, pixels: &[u8], (width, height): (u32, u32)| {
            let Some(dir) = &evidence else { return };
            std::fs::create_dir_all(dir).unwrap();
            let file = std::fs::File::create(dir.join(name)).unwrap();
            let mut encoder = png::Encoder::new(file, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(pixels)
                .unwrap();
        };
        // The bounding box of the pixels two frames `width` × `height` disagree on.
        let difference = |a: &[u8], b: &[u8], width: u32, height: u32| {
            let (mut count, mut bounds) = (0u32, [u32::MAX, u32::MAX, 0, 0]);
            for i in 0..(width * height) as usize {
                if a[i * 4..i * 4 + 4] == b[i * 4..i * 4 + 4] {
                    continue;
                }
                count += 1;
                let (x, y) = (i as u32 % width, i as u32 / width);
                bounds = [
                    bounds[0].min(x),
                    bounds[1].min(y),
                    bounds[2].max(x),
                    bounds[3].max(y),
                ];
            }
            (count, bounds)
        };
        // A window on the in-message form: `a` and `b` type, the code raises the balloon, the
        // `<END>` holds the page still, so every frame from here draws the same text.
        let mut talk = Talk {
            npcs: vec![partner([0, 0, 0])],
            conversation: Some((7, 0)),
            active: Some(Dialogue::new(script(b"ab<EXC><END>"))),
            ..Talk::default()
        };
        talk.set_rom(rom);
        talk.set_view(&camera(), false);
        play(&mut talk, 3);
        assert!(talk.balloon.is_some(), "the code raised it");
        let with = render(&mut talk, &device, &queue, (256, 192), false);
        save("balloon-exclamation.png", &with, (256, 192));
        let quad = talk.balloon_quad(talk.view.unwrap(), SCREEN).unwrap();
        // The same window without the sheet: the frame it must leave behind.
        let balloon = talk.balloon.take();
        let without = render(&mut talk, &device, &queue, (256, 192), false);
        assert!(without != with, "the sheet is drawn");
        let (count, bounds) = difference(&with, &without, 256, 192);
        assert!(count > 40, "only {count} pixels of the sheet");
        let [x, y, w, h] = quad.dst;
        assert!(
            bounds[0] as f32 >= x - 1.0
                && bounds[1] as f32 >= y - 1.0
                && (bounds[2] + 1) as f32 <= x + w
                && (bounds[3] + 1) as f32 <= y + h,
            "the changed pixels {bounds:?} sit in the sheet's quad {quad:?}"
        );
        // 59 more steps: still the same picture, then the 60th clears it (`+0x1959`).
        talk.balloon = balloon;
        play(&mut talk, 59);
        assert_eq!(talk.balloon.map(|b| b.ticks), Some(1));
        assert_eq!(
            render(&mut talk, &device, &queue, (256, 192), false),
            with,
            "still drawn at the last tick"
        );
        play(&mut talk, 1);
        assert_eq!(talk.balloon, None);
        let after = render(&mut talk, &device, &queue, (256, 192), false);
        save("balloon-after-sixty-steps.png", &after, (256, 192));
        assert_eq!(after, without, "the sheet is gone; the page is unchanged");
        assert!(talk.is_active(), "the window still waits for its key");

        // The `?` of `<QES>`, its own sheet.
        let mut question = Talk {
            npcs: vec![partner([0, 0, 0])],
            conversation: Some((7, 0)),
            active: Some(Dialogue::new(script(b"ab<QES><END>"))),
            ..Talk::default()
        };
        question.set_rom(rom);
        question.set_view(&camera(), false);
        play(&mut question, 3);
        let question_frame = render(&mut question, &device, &queue, (256, 192), false);
        save("balloon-question.png", &question_frame, (256, 192));
        assert!(
            difference(&question_frame, &with, 256, 192).0 > 0,
            "a different sheet"
        );

        // The enhanced layout draws the same sheet on the field's canvas, not re-anchored with
        // the message box the HUD moves to the bottom left.
        let hud = [0.0, 0.0, 960.0, 540.0];
        let quad = question.balloon_quad(question.view.unwrap(), hud).unwrap();
        let hud_with = render(&mut question, &device, &queue, (960, 540), true);
        save("balloon-enhanced.png", &hud_with, (960, 540));
        let held = question.balloon.take();
        let hud_without = render(&mut question, &device, &queue, (960, 540), true);
        question.balloon = held;
        let (count, bounds) = difference(&hud_with, &hud_without, 960, 540);
        assert!(
            count > 40,
            "the balloon in the enhanced HUD ({count} pixels)"
        );
        let [x, y, w, h] = quad.dst;
        assert!(
            bounds[0] as f32 >= x - 1.0
                && bounds[1] as f32 >= y - 1.0
                && (bounds[2] + 1) as f32 <= x + w
                && (bounds[3] + 1) as f32 <= y + h,
            "the enhanced sheet {bounds:?} in its quad {quad:?}"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use script::{parse_inputs, play};

/// Scripted inputs for `--screenshot` (`--input`): walking and pressing buttons without a window.
#[cfg(not(target_arch = "wasm32"))]
mod script {
    use anyhow::{Result, bail};
    use dq9_core::Input as Pad;
    use dq9_core::field::{self, Event, Leader};
    use glam::{Mat4, Vec3};

    use super::{Button, Talk};
    use crate::app::STEP;
    use crate::app::services::containers::{Containers, Taken};
    use crate::load::LoadedMap;
    use crate::walk::{self, Walk};
    use dq9_core::battle::Random;

    /// One part of an input script.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Step {
        Walk(Walk),
        Press(Button),
    }

    /// Parses `+x:2,-z:1,a,idle:1,down,a`: the walking parts of `--walk` (world axes or `idle`,
    /// a colon and seconds) and the buttons `a` (action), `b` (cancel), `up`, `down`. A button
    /// is pressed for one step, then the script goes on.
    pub fn parse_inputs(text: &str) -> Result<Vec<Step>> {
        let mut steps = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            let button = match part.to_ascii_lowercase().as_str() {
                "a" => Some(Button::Confirm),
                "b" => Some(Button::Cancel),
                "up" => Some(Button::Up),
                "down" => Some(Button::Down),
                _ => None,
            };
            match button {
                Some(button) => steps.push(Step::Press(button)),
                None => steps.extend(walk::parse_script(part)?.into_iter().map(Step::Walk)),
            }
        }
        Ok(steps)
    }

    /// Plays `steps` on a loaded map with the same fixed steps as the window: walking moves the
    /// hero (`Hero::step`, then `Field::step`), buttons go to the conversation. A map change
    /// ends the script with an error: the screenshot is of one map.
    pub fn play(
        steps: &[Step],
        loaded: &mut LoadedMap,
        rom: &[u8],
        state: &mut dq9_core::trigger::State,
        talk: &mut Talk,
    ) -> Result<()> {
        let _ = loaded.field.enter(state, false);
        talk.enter_map(loaded.npcs.clone(), &mut state.talk_counts);
        // The screenshot path has no field world, so the table draw uses a fixed generator
        // (seed 0) and says so; the window and the Web pass the world generator instead.
        let containers = match Containers::load(rom, &loaded.entry.code) {
            Ok(containers) => containers,
            Err(e) => {
                crate::app::status(&format!("containers: {e:#}"));
                Containers::default()
            }
        };
        let mut random = Random::new(0);
        let mut run = |loaded: &mut LoadedMap, direction: Vec3, talk: &mut Talk| -> Result<()> {
            let talking = talk.is_active();
            if !talking && talk.confirm_pending() {
                if !talk.has_partner(&loaded.hero)
                    && let Some((container, open)) = containers.open(
                        loaded.hero.position(),
                        loaded.hero.yaw(),
                        state,
                        &mut random,
                    )
                {
                    let taken = Taken::of(open);
                    println!(
                        "container {:?} #{} (id {}): {} (preview: the party is not updated)",
                        container.container,
                        container.number,
                        container.id,
                        taken.line()
                    );
                    if let Err(e) = talk.show_event(&taken.text()) {
                        crate::app::status(&format!("container #{}: {e:#}", container.number));
                    }
                } else {
                    let events = talk.begin(
                        rom,
                        &loaded.entry.code,
                        &loaded.hero,
                        &mut loaded.field,
                        state,
                    );
                    if events.iter().any(|e| matches!(e, Event::MapChange(_))) {
                        bail!("the script leaves the map");
                    }
                }
                talk.discard_input();
            }
            let direction = if talking || talk.is_active() {
                Vec3::ZERO
            } else {
                direction
            };
            loaded
                .hero
                .step(&loaded.collision, Pad::from_direction(direction), 0, 1);
            let leader = Leader {
                position: loaded.hero.position(),
                yaw: loaded.hero.yaw(),
                input: direction.normalize_or_zero(),
            };
            if field::Field::changes_map(&loaded.field.step(&leader, state)) {
                bail!("the script leaves the map");
            }
            talk.step();
            // The preview path has no fixed `AnimatedScene::step`: the partner's turn is
            // stepped here, between the script's frames (`AnimatedScene::step_turns`).
            talk.face_partner(&mut loaded.runtime, Some(&loaded.hero));
            loaded.runtime.step_turns();
            Ok(())
        };
        for step in steps {
            match *step {
                Step::Walk(Walk { direction, seconds }) => {
                    for _ in 0..(seconds / STEP).round() as u32 {
                        run(loaded, direction, talk)?;
                    }
                }
                Step::Press(button) => {
                    talk.press(button);
                    run(loaded, Vec3::ZERO, talk)?;
                }
            }
        }
        if let Some(instance) = loaded.runtime.scene.instances.get_mut(loaded.hero_instance) {
            instance.world = Mat4::from_translation(loaded.hero.position());
        }
        Ok(())
    }
}

#[cfg(test)]
mod saved_dialogue_tests {
    use super::{HeroIdentity, Names, archive_path};
    use dq9_core::{state::GameState, trigger::track_of_room};
    use dq9_formats::save::{FILE_SIZE, MAGIC, Save};
    use dq9_formats::{Font, FontId};

    /// Font with the glyphs "A" (index 0) and "B" (index 1), for the name and condition test.
    fn identity_font() -> (Vec<u8>, Vec<u8>) {
        // Four magic bytes, then five words (the last one unused by the parser), then the
        // glyph table at 0x18 and the glyph strings after it.
        let strings = 0x18u32 + 2 * 8;
        let mut info = b"1.1\0".to_vec();
        for v in [2u32, 0, 0x18, 0x18, strings] {
            info.extend(v.to_le_bytes());
        }
        info.extend(strings.to_le_bytes());
        info.extend([2, 0x41, 0, 0]); // "A": width 2, glyph string length 1 | vowel
        info.extend((strings + 1).to_le_bytes());
        info.extend([2, 0x41, 2, 0]); // "B": width 2 at x 2, length 1
        info.extend(b"AB");
        let mut data = b"1.0\0".to_vec();
        data.extend(8u16.to_le_bytes());
        data.extend(2u16.to_le_bytes());
        data.extend(2u32.to_le_bytes());
        data.extend(16u32.to_le_bytes());
        data.extend([0b1100_0000, 0]);
        (info, data)
    }

    #[test]
    fn saved_name_and_sex_feed_substitution_and_conditions() {
        let (info, data) = identity_font();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        let mut identity = HeroIdentity {
            name: [0; dq9_formats::save::NAME_LEN],
            male: true,
        };
        identity.name[0] = 1; // "B"
        let names = identity.names(&font);
        assert_eq!(names.hero.as_slice(), b"B".as_slice());
        let rendered = |names: &Names| -> Vec<u8> {
            let text = b"<HERO><IF_HERO_MALE>A<ELSE_HERO_NOT_MALE>B<ENDIF_HERO_MALE>";
            let (_, tokens) = dq9_core::talk::compile_message(&font, text, names);
            tokens
                .into_iter()
                .filter_map(|token| match token {
                    dq9_formats::message::Token::Text(bytes) => Some(bytes),
                    _ => None,
                })
                .flatten()
                .collect()
        };
        assert_eq!(rendered(&names).as_slice(), b"BA".as_slice());
        let female = HeroIdentity {
            male: false,
            ..identity
        };
        assert_eq!(rendered(&female.names(&font)).as_slice(), b"BB".as_slice());
    }

    #[test]
    fn a_save_without_a_name_keeps_the_default_identity() {
        let (info, data) = identity_font();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        let identity = HeroIdentity {
            name: [0; dq9_formats::save::NAME_LEN],
            male: false,
        };
        assert_eq!(identity.names(&font), Names::default());
    }

    #[test]
    fn saved_map_track_selects_the_dialogue_archive() {
        let mut bytes = vec![0; FILE_SIZE];
        bytes[..MAGIC.len()].copy_from_slice(MAGIC);
        bytes[21] = 1;
        let event = 11584;
        bytes[event..event + 3].copy_from_slice(&[19, 2, 1]);
        bytes[event + 3 * 0x1c..event + 3 * 0x1c + 3].copy_from_slice(&[11, 4, 2]);
        bytes[event + 3 * 0x1c + 3] = 2;
        bytes[event + 0x8c + 1] = 2;
        bytes[event + 0x2cc + 5] = 0x30;
        let mut save = Save::parse(&bytes).unwrap();
        save.slot_mut(0).unwrap().seal();
        let game = GameState::from_save(&save).unwrap();
        let story = dq9_formats::cast::Story::default();
        let mut state = crate::load::trigger_state(Some(&game), &story, [None; 3]);
        state.track = track_of_room(109);
        let track = &state.tracks[state.track];
        assert_eq!((track.chapter, track.step, track.substep), (19, 2, 1));
        assert_eq!(
            archive_path("C01M09", track.chapter).unwrap(),
            "data/scenario/C01Q0.gp2"
        );
        assert_eq!(
            archive_path("C01M01", track.chapter).unwrap(),
            "data/scenario/C01Q0.gp2"
        );
        assert!(state.flags.contains(&9));
        assert_eq!(state.quests.get(&11), Some(&3));
        state.track = track_of_room(200);
        let track = &state.tracks[state.track];
        assert_eq!((track.chapter, track.step, track.substep), (11, 4, 2));
        assert_eq!(
            archive_path("C02", track.chapter).unwrap(),
            "data/scenario/C02S0.gp2"
        );
        assert!(track.flags[0].contains(&1));
        let overridden = crate::load::trigger_state(Some(&game), &story, [Some(15), Some(7), None]);
        assert_eq!(overridden.tracks[3].step, 7);
        assert_eq!(
            archive_path("C02", overridden.tracks[3].chapter).unwrap(),
            "data/scenario/C02M0.gp2"
        );
        assert!(archive_path("C01", 0).is_err());
        assert!(archive_path("C01", 20).is_err());
    }

    #[test]
    fn transition_uses_destination_track_for_cast_and_dialogue() {
        use dq9_core::field::Field;
        use dq9_formats::cast::{Story, run_placements};
        use dq9_formats::dialogue::{self, Kind, Line, Session, TalkState};
        use dq9_formats::map::Regions;
        use dq9_formats::table::{Record, Table, Value};

        let mut state = dq9_core::trigger::State::default();
        (
            state.tracks[0].chapter,
            state.tracks[0].step,
            state.tracks[0].substep,
        ) = (1, 1, 1);
        (
            state.tracks[3].chapter,
            state.tracks[3].step,
            state.tracks[3].substep,
        ) = (11, 4, 2);
        let story = crate::load::story_from_state(&Story::default(), &state, Some(200));
        let placements = Table {
            records: vec![Record {
                kind: 5,
                values: vec![
                    Value::Int(11),
                    Value::Int(4),
                    Value::Int(1),
                    Value::Int(11),
                    Value::Int(4),
                    Value::Int(3),
                    Value::Int(2),
                    Value::Int(200),
                    Value::Int(7),
                    Value::Float(1.0),
                    Value::Float(0.0),
                    Value::Float(2.0),
                    Value::Float(0.0),
                ],
            }],
            string_count: 0,
        };
        let (cast, _) = run_placements(&placements, 200, &story);
        assert!(cast.contains_key(&7));
        let old_story = crate::load::story_from_state(&Story::default(), &state, Some(109));
        assert!(
            !run_placements(&placements, 200, &old_story)
                .0
                .contains_key(&7)
        );
        assert_eq!(state.track, 0);

        let mut field = Field::new(200, &Regions::default(), None).unwrap();
        field.enter(&mut state, true);
        let track = &state.tracks[state.track];
        assert_eq!(
            archive_path("C02M01", track.chapter).unwrap(),
            "data/scenario/C02S0.gp2"
        );
        let lines = [
            Line {
                kind: Some(Kind::Step),
                a: 1,
                b: 1,
                night: None,
                slot: 0,
                text: Some(b"old"),
            },
            Line {
                kind: Some(Kind::Step),
                a: 4,
                b: 4,
                night: None,
                slot: 0,
                text: Some(b"destination"),
            },
        ];
        let talk = TalkState {
            step: i32::from(track.step),
            substep: i32::from(track.substep),
            context: 0,
            talk_count: [0; 2],
            daytime: true,
            session: Session::default(),
            quest: &|_| 0,
        };
        assert_eq!(dialogue::select(&lines, &talk), Some(1));
    }
}
