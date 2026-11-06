//! Conversations in the application: the action key starts a talk with the NPC in front of
//! the hero (`dq9_core::talk`), the line is chosen and prepared from the ROM, and the game's
//! message window is drawn over the map with the ROM's font (`dq9_render::ui`).
//!
//! Layout numbers come from `MessageShow` (`0x0204501c`): window rectangle `(2, 0x74, 0xfc,
//! 0x4a)`, three lines of 20 pixels, name plate `advance + 26 - (advance & 7)` wide and 22 high.
//! The margins of the text, the plate and prompt positions, the fill colour and the wait arrow
//! are **[hypothèses]** (`docs/re/dialogue.md`, § « Fenêtre »).

use anyhow::{Context as _, Result, bail};
use dq9_core::Hero;
use dq9_core::field::{Event, Field, TalkTrigger};
use dq9_core::talk::{self, Dialogue, Input, Names, PromptKind, TalkCounts, Talkable};
use dq9_core::trigger::{Request, State};
use dq9_formats::dialogue::{self, Session, TalkState};
use dq9_formats::spr::Sprite;
use dq9_formats::{Font, FontId, Gpc2, Table};
use dq9_render::Viewport;
use dq9_render::ui::{Atlas, Image, Quad, Ui};
use nds_rom::Rom;

use crate::app::status;

/// Language of the dialogue files (`NNN_<lg>.bin`) and of the prompt labels.
const LANGUAGE: &str = "en";
/// Chapter letters of the dialogue archives (`Scenario_ChapterLetter`, `ov_017::0218d2c4`).
const CHAPTER_LETTERS: &[u8] = b"ABCDEFGHIJSTKLMNOPQ";

/// The message window on the 256×192 screen (`func_02042b98(win, 2, 0x74, 0xfc, 0x4a)`).
const WINDOW: [i32; 4] = [2, 0x74, 0xfc, 0x4a];
/// Where the text starts, from the window's corner **[hypothèse]** (the game's text origin is
/// `(2, 0x7e)` in its own text layer).
const TEXT_MARGIN: [i32; 2] = [12, 10];
/// Pixels between the lines (`+0x1864 = 0x14`).
const LINE_HEIGHT: i32 = 20;
/// Width the text wraps at **[hypothèse]**.
const TEXT_WIDTH: i32 = 228;
const NAME_HEIGHT: i32 = 22;
/// Window fill **[hypothèse]**.
const FILL: [f32; 4] = [0.02, 0.03, 0.22, 0.9];
const SELECTED: [f32; 4] = [0.25, 0.35, 0.8, 0.95];

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
        // The big arrow of `arrow2.spr` as the wait arrow **[hypothèse]**.
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
        Ok(Self {
            atlas: Atlas::build(&font, &border, &arrow),
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

/// A closed conversation, as trigger event 1 sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended {
    pub npc: u16,
    pub context: u16,
    /// Choice (0 first, 1 second) of the last prompt answered.
    pub answer: Option<u8>,
}

/// Conversations of the application: the NPCs of the map, the counters, the open dialogue.
#[derive(Debug, Default)]
pub struct Talk {
    assets: Option<Assets>,
    gpu: Option<(wgpu::TextureFormat, Ui)>,
    npcs: Vec<Talkable>,
    counts: TalkCounts,
    names: Names,
    active: Option<Dialogue>,
    /// Buttons pressed since the last step.
    pending: Input,
    ticks: u32,
    /// An event message is open (see [`Talk::show_event`]).
    event_message: bool,
    /// The player closed it.
    closed: bool,
    /// Native 410 hid the frame of the current event message.
    frameless: bool,
    /// The NPC and context of the open conversation (not an event message).
    conversation: Option<(u16, u16)>,
    /// A conversation the player closed, until [`Talk::take_ended`].
    ended: Option<Ended>,
    fade: Option<[f32; 4]>,
    sounds: Vec<talk::Sound>,
}

impl Talk {
    /// Loads the font and the window pieces of the ROM.
    pub fn set_rom(&mut self, rom: &[u8]) {
        match Assets::load(rom) {
            Ok(assets) => self.assets = Some(assets),
            Err(e) => status(&format!("message window unavailable: {e:#}")),
        }
    }

    /// A map is loaded: its NPCs, and the map counters cleared.
    pub fn enter_map(&mut self, npcs: Vec<Talkable>) {
        self.npcs = npcs;
        self.counts.enter_map();
        self.active = None;
    }

    /// Action 0x7c: the NPC leaves the map (flag `0x8000` of its object): nobody to talk to.
    pub fn remove_npc(&mut self, npc: u16) {
        self.npcs.retain(|n| n.id != npc);
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Whether the action key was pressed since the last step.
    pub fn confirm_pending(&self) -> bool {
        self.pending.confirm
    }

    /// Records a button press for the next step.
    pub fn press(&mut self, button: Button) {
        match button {
            Button::Confirm => self.pending.confirm = true,
            Button::Cancel => self.pending.cancel = true,
            Button::Up => self.pending.up = true,
            Button::Down => self.pending.down = true,
        }
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
        let input = std::mem::take(&mut self.pending);
        let Some(dialogue) = &mut self.active else {
            return;
        };
        dialogue.step(input);
        self.sounds.extend(dialogue.take_sounds());
        if let Some(outcome) = dialogue.outcome() {
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
                    self.ended = Some(Ended {
                        npc,
                        context,
                        answer: outcome.answer,
                    });
                }
            }
            self.active = None;
        }
    }

    /// The conversation the player closed since the last call: the application fires the
    /// trigger event 1 with it (`Field::talk_ended`).
    pub fn take_ended(&mut self) -> Option<Ended> {
        self.ended.take()
    }

    /// Native 400: opens the window on an event's text (see [`Dialogue::for_event`]).
    pub fn show_event(&mut self, text: &[u8]) -> Result<()> {
        let Some(assets) = &self.assets else {
            bail!("no message window (the ROM resources failed to load)");
        };
        let font = assets.font()?;
        let script = talk::prepare(&font, text, &self.names, TEXT_WIDTH);
        self.active = Some(Dialogue::for_event(script));
        self.event_message = true;
        self.frameless = false;
        self.closed = false;
        Ok(())
    }

    /// Native 410: the window of the current event message has no frame.
    pub fn hide_event_frame(&mut self) {
        self.frameless = true;
    }

    /// Native 401: closes the event message without reporting it as read.
    pub fn close_event(&mut self) {
        if self.event_message {
            self.active = None;
            self.event_message = false;
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
        state: &State,
        npc: u16,
        context: u16,
    ) -> Result<()> {
        let Some(assets) = &self.assets else {
            bail!("no message window (the ROM resources failed to load)");
        };
        let track = &state.tracks[state.track];
        let letter = *CHAPTER_LETTERS
            .get(usize::from(track.chapter).wrapping_sub(1))
            .with_context(|| format!("chapter {} has no dialogue archive", track.chapter))?;
        let area = map_code.get(..3).context("map code")?;
        let archive = format!("data/scenario/{area}{}0.gp2", char::from(letter));
        let member = format!("{npc:03}_{LANGUAGE}.bin");
        let rom = Rom::parse(rom).context("parsing the ROM")?;
        let archive = Gpc2::parse(rom.file_by_path(&archive)?).context("parsing the archive")?;
        let Some(entry) = archive.find(&member) else {
            bail!("NPC {npc} has no dialogue file {member}");
        };
        let data = archive.member(entry)?;
        let table = Table::parse(&data).context("not a record table")?;
        let lines = dialogue::parse(&table)?;
        let quests = |q: i32| state.quests.get(&q).copied().unwrap_or(0);
        let talk_state = TalkState {
            step: i32::from(track.step),
            substep: i32::from(track.substep),
            context: i32::from(context),
            talk_count: self.counts.get(npc),
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
            "talk: {area}{}0/{member} record {line}, context {context}",
            char::from(letter)
        ));
        self.counts.record(npc);
        self.conversation = Some((npc, context));
        self.active = Some(Dialogue::new(script));
        Ok(())
    }

    /// The quads of the window and of the fade over it on the 256×192 screen.
    pub fn quads(&self) -> Vec<Quad> {
        let mut out = self.window_quads();
        if let (Some(assets), Some(color)) = (&self.assets, self.fade) {
            out.push(assets.atlas.solid([0, 0, 256, 192], color));
        }
        out
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
            let y = window[1] + TEXT_MARGIN[1] + i as i32 * LINE_HEIGHT;
            atlas.text(line, x, y, color, &mut out);
        }
        if view.arrow {
            let (w, h) = atlas.arrow_size();
            let bounce = (self.ticks / 20 % 2) as i32 * 2;
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
}

/// Where the hero stands to talk to an NPC with a given context (`Target`): what an autopilot
/// needs to have the conversation without walking there.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partner {
    pub npc: u16,
    pub context: u16,
    pub position: glam::Vec3,
    pub yaw: f32,
}

#[cfg(not(target_arch = "wasm32"))]
impl Talk {
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

/// The buttons of the message window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Confirm,
    Cancel,
    Up,
    Down,
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
    use crate::load::LoadedMap;
    use crate::walk::{self, Walk};

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
        story: &dq9_formats::cast::Story,
        talk: &mut Talk,
    ) -> Result<()> {
        let mut state = field::initial_state(story);
        let _ = loaded.field.enter(&mut state, false);
        talk.enter_map(loaded.npcs.clone());
        let mut run = |loaded: &mut LoadedMap, direction: Vec3, talk: &mut Talk| -> Result<()> {
            let talking = talk.is_active();
            if !talking && talk.confirm_pending() {
                let events = talk.begin(
                    rom,
                    &loaded.entry.code,
                    &loaded.hero,
                    &mut loaded.field,
                    &mut state,
                );
                talk.discard_input();
                if events.iter().any(|e| matches!(e, Event::MapChange(_))) {
                    bail!("the script leaves the map");
                }
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
            if field::Field::changes_map(&loaded.field.step(&leader, &mut state)) {
                bail!("the script leaves the map");
            }
            talk.step();
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
