//! The field menu and the facility screens, containers, and the hero's look when equipment changes
//! (`app/services.rs` holds the screens themselves).

use super::App;
use super::services;
use super::services::containers::Taken;
use super::status;
use crate::talk::Button;
use crate::talk::ServiceTag;
use dq9_core::field;

impl App {
    /// The party members' names as the font draws them, for the menu's rows.
    fn service_names(&self) -> Vec<Vec<u8>> {
        let hero = self.talk.hero_name();
        self.party
            .members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                if index == 0 {
                    return hero.clone();
                }
                self.talk
                    .with_ui(|_, font| {
                        let mut name = Vec::new();
                        for &glyph in member.name() {
                            match font.glyphs.get(usize::from(glyph)) {
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

    /// `I` on the field: opens the menu, unless a conversation, an event or a battle holds
    /// the player. Returns whether the menu opened.
    pub(super) fn open_field_menu(&mut self) -> bool {
        let Some(rom) = self.rom.as_deref() else {
            return false;
        };
        let names = self.service_names();
        self.services.set_names(names);
        self.talk.discard_input();
        match self.services.open_menu(rom, &mut self.party) {
            Ok(()) => {
                status("field menu");
                true
            }
            Err(e) => {
                status(&format!("field menu unavailable: {e:#}"));
                false
            }
        }
    }

    /// The facility a talk line named.
    pub(super) fn open_service(&mut self, tag: ServiceTag) -> bool {
        let Some(rom) = self.rom.as_deref() else {
            return false;
        };
        let names = self.service_names();
        self.services.set_names(names);
        self.talk.discard_input();
        match self.services.open(tag, rom, &mut self.party, &self.day) {
            Ok(()) => {
                status(&format!("service: {tag:?}"));
                true
            }
            Err(e) => {
                status(&format!("service {tag:?} unavailable: {e:#}"));
                false
            }
        }
    }

    /// The action key on a container: opens the one the hero faces, gives its content and
    /// shows a placeholder line. Returns whether a container was there: the key is spent even
    /// when it refuses (`AlreadyOpen`, a lock). The draw comes from the **world** generator
    /// (`GetBTRandom`, `0x020741e0`; the core rule labels which generator is open
    /// **[hypothesis]**).
    pub(super) fn examine_container(&mut self) -> bool {
        let Some(hero) = &self.hero else {
            return false;
        };
        let (position, yaw) = (hero.position(), hero.yaw());
        let Some((container, open)) = self.containers.open(
            position,
            yaw,
            &mut self.state,
            &mut self.combat.world.random,
        ) else {
            return false;
        };
        let taken = Taken::of(open);
        let stored = taken.give(&mut self.party, &mut self.state);
        status(&format!(
            "container {:?} #{} (id {}): {}",
            container.container,
            container.number,
            container.id,
            taken.line()
        ));
        if let (false, Taken::Item(id)) = (stored, taken) {
            status(&format!(
                "container #{}: item {id} did not fit the bag (the flag stays set)",
                container.number
            ));
        }
        if let Err(e) = self.talk.show_event(&taken.text()) {
            status(&format!("container #{}: {e:#}", container.number));
        }
        true
    }

    pub(super) fn service_button(&mut self, button: Button) {
        let effect = self.services.press(
            button,
            &mut self.party,
            &mut self.combat.world.random,
            &mut self.day,
        );
        if effect == services::Effect::Save {
            let result = self.confess();
            self.services.confession_written(result);
        }
        if effect == services::Effect::Suspend {
            self.quick_save();
        }
        self.after_service(effect);
    }

    /// The `--save` container's file and the `--save-out` destination. The Confession writes
    /// into `--save-out` when it is given and into the container itself otherwise (`--save`),
    /// the way the DS writes the cartridge's own normal slot.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_save_files(
        &mut self,
        save: Option<std::path::PathBuf>,
        out: Option<std::path::PathBuf>,
    ) {
        self.save_path = save;
        self.save_out = out;
    }

    /// The `--suspend-out` destination of the field menu's Quick Save (`--save` itself when
    /// it is absent).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_suspend_out(&mut self, out: Option<std::path::PathBuf>) {
        self.suspend_out = out;
    }

    /// The Confession's write (`ov_003::02159d0c`: message 100, then `Save_WriteStep` at
    /// `0x020aad2c`): the live state into the container's current normal slot, sealed,
    /// mirrored when it is slot 0, through the CLI's temporary file and rename. The input image
    /// is the base of the write; the file named by `--save-out`, or the `--save` container
    /// itself when no output was named, is the one replaced — never any other file.
    ///
    /// The error is the reason the window shows: the port has no container without `--save`,
    /// and no destination when neither option names a file.
    #[cfg(not(target_arch = "wasm32"))]
    fn confess(&mut self) -> Result<(), String> {
        let (path, container) = match (&self.save_out, &self.save_path) {
            (Some(out), _) => (out.clone(), false),
            (None, Some(save)) => (save.clone(), true),
            (None, None) => {
                return Err(
                    "The adventure log cannot be recorded: no save file is open. Start the \
                     session with --save <file> (and --save-out <file> to leave it untouched)."
                        .to_owned(),
                );
            }
        };
        let state = self.write_save_out(&path).map_err(|e| {
            format!("The adventure log was not recorded: {e}. The adventure goes on without it.")
        })?;
        status(&format!(
            "confession: {} written{} (map {}, story {}.{}.{}, {} member(s), {} gold)",
            path.display(),
            if container {
                ", the --save container itself"
            } else {
                ", --save untouched"
            },
            state.map,
            state.scenario.chapter,
            state.scenario.step,
            state.scenario.substep,
            state.party.members.len(),
            state.party.gold,
        ));
        Ok(())
    }

    /// The field menu's Misc. → Quick Save (`ov_002::02169efc`): the live state into the
    /// container's **suspend** slot (slot 1), sealed, with the leader's current position and
    /// heading in the `Warp` block, through the CLI's temporary file and rename. The
    /// destination is `--suspend-out` when it is given and the `--save` container itself
    /// otherwise, as the DS writes its own cartridge. The normal slot is never touched.
    #[cfg(not(target_arch = "wasm32"))]
    fn quick_save(&mut self) {
        let (path, container) = match (&self.suspend_out, &self.save_path) {
            (Some(out), _) => (out.clone(), false),
            (None, Some(save)) => (save.clone(), true),
            (None, None) => {
                self.services.quick_save_refused(
                    "The quick save log cannot be recorded: no save file is open. Start the \
                     session with --save <file> (and --suspend-out <file> to leave it untouched).",
                );
                status("quick save: no save file is open");
                return;
            }
        };
        // Sub-state 1 shows 4701 ("Recording quick save log. Do not turn the power OFF…")
        // while the slot is written; the write is instantaneous here, so the line is logged.
        let writing = self.services.misc_message(services::misc_line::WRITING);
        if !writing.is_empty() {
            status(&format!(
                "quick save: {}",
                String::from_utf8_lossy(&writing).replace('\n', " ")
            ));
        }
        match self.write_suspend_out(&path) {
            Ok(state) => {
                status(&format!(
                    "quick save: {} written{} (map {}, story {}.{}.{}, {} member(s), {} gold)",
                    path.display(),
                    if container {
                        ", the --save container itself"
                    } else {
                        ", --save untouched"
                    },
                    state.map,
                    state.scenario.chapter,
                    state.scenario.step,
                    state.scenario.substep,
                    state.party.members.len(),
                    state.party.gold,
                ));
                self.services.quick_save_done();
                // The interactive app returns to the title once 4702 is dismissed
                // (`after_service`); a headless run keeps the menu.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.title_pending = self.interactive;
                }
            }
            Err(e) => {
                status(&format!("quick save: not written: {e:#}"));
                self.services.quick_save_refused(&format!(
                    "The quick save log was not recorded: {e}. The adventure goes on without it."
                ));
            }
        }
    }

    /// The Quick Save without a container: this build has no save file at all (`App`'s save
    /// fields are the native options).
    #[cfg(target_arch = "wasm32")]
    fn quick_save(&mut self) {
        self.services.quick_save_refused(
            "The quick save log cannot be recorded here: this build has no save file.",
        );
    }

    /// The Confession without a container: the page has no save file at all (`App`'s save
    /// fields are the native options).
    #[cfg(target_arch = "wasm32")]
    fn confess(&mut self) -> Result<(), String> {
        Err("The adventure log cannot be recorded here: this build has no save file.".to_owned())
    }

    /// Ends the session: the original's 110 ("Bless you, my child. Please turn the power OFF.")
    /// is its last line — state 9 shows it and stays there, its words telling the player to
    /// switch the console off. The DS returns to its title screen when the console is switched
    /// on again; the interactive port shows the title at once, at `ov_004`'s own slot check
    /// (`docs/app/title.md`). A headless run only records the flag.
    pub(super) fn end_session(&mut self) {
        self.session_over = true;
        status("session over: the Confession said its last line (110)");
        #[cfg(not(target_arch = "wasm32"))]
        if self.interactive {
            // The original stays on 110; the port cannot power off and shows the title,
            // which is what the next switch-on does.
            self.show_title();
            status("the title screen is back, as after switching the console off");
        }
    }

    pub(super) fn service_side(&mut self, right: bool) {
        self.services.side(right);
    }

    pub(super) fn service_hold(&mut self) {
        self.services.hold(&self.party);
    }

    pub(super) fn service_take(&mut self) {
        self.services.take(&mut self.party);
    }

    /// What follows a command: the facility's conversation ends when its screen closes, the
    /// lighting follows an inn's clock and the field's scene is rebuilt once equipment
    /// changed and the menu is gone.
    pub(super) fn after_service(&mut self, effect: services::Effect) {
        if self.services.take_end_session() {
            self.end_session();
        }
        // The Quick Save's 4702 closes back to the title, as `ov_002::02169efc` returns the
        // player there. Headless runs keep their steps.
        #[cfg(not(target_arch = "wasm32"))]
        if self.title_pending && !self.services.is_open() {
            self.title_pending = false;
            self.show_title();
            status("quick save: 4702 dismissed, the title screen is back");
            return;
        }
        if !self.services.is_open() {
            self.talk.finish_service();
        }
        if effect == services::Effect::Clock {
            self.sync_clock_from_day();
        }
        if !self.services.is_open() && self.services.look_dirty() {
            self.services.clear_look_dirty();
            self.refresh_hero_look();
        }
    }

    /// Rebuilds only the hero the renderer draws for the leader's current equipment, through
    /// [`AnimatedScene::replace_hero`]: the same model and instance slots are reused in place
    /// and obsolete hero textures are released. The field, its cast, talk counters, doors,
    /// camera, events and generators are untouched: this must not reload the map, whose entry
    /// would replay its triggers (`sync_day_phase` warns against it).
    pub(super) fn refresh_hero_look(&mut self) {
        let mut look = self.look.clone();
        if let Some(member) = self.party.leader() {
            look.armour = member.equipment.torso;
            look.legwear = member.equipment.legs;
            look.gloves = member.equipment.arms;
            look.footwear = member.equipment.feet;
            look.headgear = member.equipment.head;
        }
        // The rig draws no weapon, shield or accessory: when the visible fields did not move
        // (same clothing, a weapon-only swap, equip then undo), leave the current rig alone.
        if look == self.look {
            return;
        }
        self.look = look;
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let Some(hero) = self.hero else {
            return;
        };
        let image = match nds_rom::Rom::parse(rom) {
            Ok(image) => image,
            Err(e) => {
                status(&format!("hero look: {e:#}"));
                return;
            }
        };
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        // The same scene model and instance are reused in place; a malformed new hero
        // leaves the previous rig and its storage untouched.
        match runtime.replace_hero(&self.look, |path| dq9_formats::map::rom_file(&image, path)) {
            Ok(()) => {
                if let Some(instance) = self.hero_instance {
                    let world =
                        runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()));
                    if let Some(shown) = runtime.scene.instances.get_mut(instance) {
                        shown.world = world;
                    }
                }
                self.upload();
            }
            Err(e) => status(&format!("hero look: {e:#}")),
        }
    }
}

#[cfg(all(test, windows))]
mod tests;
