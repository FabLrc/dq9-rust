//! Several sequences at once: the music, a jingle, the sound effects. The DS driver gives each
//! sequence to a *player* of the archive (PLAYER records of INFO), which runs only so many
//! sequences together; when it is full, the sequence of lowest priority gives way, or the new
//! one is refused if every playing one outranks it. Hardware channels are not modelled: each
//! sequence has the 32 voices of its own [`Player`] (`docs/re/sdat.md`).

use std::collections::HashMap;
use std::sync::Arc;

use crate::player::Player;
use crate::song::Song;

/// Seconds a sequence takes to die out when another takes its place in a full player.
const STEAL_FADE: f32 = 0.02;

/// Names a sequence the mixer started. Stays valid (and false for [`Mixer::is_playing`])
/// after the sequence ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(u64);

#[derive(Debug)]
struct Slot {
    handle: Handle,
    player: Player,
    group: u8,
    priority: u8,
    /// Fading out or replaced: no longer counts against its player's limit.
    dying: bool,
}

/// What a stream callback pulls from: every playing sequence summed.
#[derive(Debug)]
pub struct Mixer {
    rate: u32,
    slots: Vec<Slot>,
    /// Sequences a player may run at once, by player index; missing: no limit.
    limits: HashMap<u8, u16>,
    next: u64,
    scratch: Vec<f32>,
}

impl Mixer {
    /// A mixer producing stereo frames at `rate` Hz.
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            slots: Vec::new(),
            limits: HashMap::new(),
            next: 0,
            scratch: Vec::new(),
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Declares how many sequences player `player` runs at once (its PLAYER record).
    pub fn set_player_limit(&mut self, player: u8, max_sequences: u16) {
        self.limits.insert(player, max_sequences);
    }

    /// Starts `song` on its player. `None` when the player is full of sequences that outrank
    /// it (equal priority gives way to the newcomer, the oldest first).
    pub fn play(&mut self, song: Arc<Song>) -> Option<Handle> {
        let (group, priority) = (song.player, song.priority);
        if let Some(&max) = self.limits.get(&group) {
            let live = |s: &&Slot| s.group == group && !s.dying && !s.player.finished();
            let playing = self.slots.iter().filter(live).count();
            if playing >= usize::from(max) {
                // Lowest priority, then oldest: slots are in start order.
                let victim = self
                    .slots
                    .iter_mut()
                    .filter(|s| s.group == group && !s.dying && !s.player.finished())
                    .min_by_key(|s| s.priority)?;
                if victim.priority > priority {
                    return None;
                }
                victim.dying = true;
                victim.player.fade_out(STEAL_FADE);
            }
        }
        let handle = Handle(self.next);
        self.next += 1;
        self.slots.push(Slot {
            handle,
            player: Player::new(song, self.rate),
            group,
            priority,
            dying: false,
        });
        Some(handle)
    }

    fn slot(&mut self, handle: Handle) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.handle == handle)
    }

    /// Fades the sequence out over `seconds` and drops it.
    pub fn stop(&mut self, handle: Handle, seconds: f32) {
        if let Some(slot) = self.slot(handle) {
            slot.dying = true;
            slot.player.fade_out(seconds);
        }
    }

    /// Stops everything at once.
    pub fn clear(&mut self) {
        self.slots.clear();
    }

    /// Whether the sequence still sounds (or is paused).
    pub fn is_playing(&self, handle: Handle) -> bool {
        self.slots
            .iter()
            .any(|s| s.handle == handle && !s.player.finished())
    }

    /// Whether any sequence is still playing.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.player.finished())
    }

    /// Holds the sequence where it is, silent, or lets it go on.
    pub fn set_paused(&mut self, handle: Handle, paused: bool) {
        if let Some(slot) = self.slot(handle) {
            slot.player.set_paused(paused);
        }
    }

    /// Host volume of the sequence, 0.0 to 1.0, at once.
    pub fn set_volume(&mut self, handle: Handle, volume: f32) {
        if let Some(slot) = self.slot(handle) {
            slot.player.set_volume(volume);
        }
    }

    /// Moves the host volume of the sequence to `volume` in a straight line over `seconds`.
    pub fn ramp_volume(&mut self, handle: Handle, volume: f32, seconds: f32) {
        if let Some(slot) = self.slot(handle) {
            slot.player.ramp_volume(volume, seconds);
        }
    }

    /// Mixes the next `out.len() / 2` frames of every sequence into `out` (interleaved
    /// stereo), overwriting it.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        self.scratch.resize(out.len(), 0.0);
        for slot in &mut self.slots {
            slot.player.render(&mut self.scratch);
            for (o, s) in out.iter_mut().zip(&self.scratch) {
                *o += *s;
            }
        }
        self.slots.retain(|s| !s.player.finished());
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A song of one endless note on a bank with no instrument: silent but alive.
    fn song(player: u8, priority: u8, stream: &[u8]) -> Arc<Song> {
        Arc::new(Song::synthetic(player, priority, stream))
    }

    /// A rest then a jump back to the start: a sequence that never ends.
    const FOREVER: [u8; 6] = [0x80, 12, 0x94, 0, 0, 0];

    #[test]
    fn a_full_player_gives_way_to_an_equal_or_higher_priority() {
        let mut mixer = Mixer::new(48_000);
        mixer.set_player_limit(3, 2);
        let a = mixer.play(song(3, 64, &FOREVER)).unwrap();
        let b = mixer.play(song(3, 64, &FOREVER)).unwrap();
        // Equal priority: the oldest gives way.
        let c = mixer.play(song(3, 64, &FOREVER)).unwrap();
        let mut out = vec![0.0; 2048];
        mixer.render(&mut out);
        assert!(!mixer.is_playing(a));
        assert!(mixer.is_playing(b) && mixer.is_playing(c));
    }

    #[test]
    fn a_lower_priority_newcomer_is_refused_by_a_full_player() {
        let mut mixer = Mixer::new(48_000);
        mixer.set_player_limit(1, 1);
        let a = mixer.play(song(1, 100, &FOREVER)).unwrap();
        assert!(mixer.play(song(1, 10, &FOREVER)).is_none());
        // Another player is not affected.
        assert!(mixer.play(song(2, 10, &FOREVER)).is_some());
        let mut out = vec![0.0; 256];
        mixer.render(&mut out);
        assert!(mixer.is_playing(a));
    }

    #[test]
    fn the_lowest_priority_is_the_one_replaced() {
        let mut mixer = Mixer::new(48_000);
        mixer.set_player_limit(1, 2);
        let high = mixer.play(song(1, 90, &FOREVER)).unwrap();
        let low = mixer.play(song(1, 20, &FOREVER)).unwrap();
        let new = mixer.play(song(1, 50, &FOREVER)).unwrap();
        let mut out = vec![0.0; 2048];
        mixer.render(&mut out);
        assert!(mixer.is_playing(high) && mixer.is_playing(new));
        assert!(!mixer.is_playing(low));
    }

    #[test]
    fn stopping_fades_and_removes_the_sequence() {
        let mut mixer = Mixer::new(48_000);
        let a = mixer.play(song(0, 64, &FOREVER)).unwrap();
        mixer.stop(a, 0.1);
        let mut out = vec![0.0; 2 * 48_000 / 5];
        mixer.render(&mut out);
        assert!(!mixer.is_playing(a));
        assert!(mixer.is_empty());
    }

    #[test]
    fn a_paused_sequence_stays_alive_and_silent() {
        let mut mixer = Mixer::new(48_000);
        let a = mixer.play(song(0, 64, &FOREVER)).unwrap();
        mixer.set_paused(a, true);
        let mut out = vec![1.0; 4096];
        mixer.render(&mut out);
        assert!(mixer.is_playing(a));
        assert!(out.iter().all(|s| *s == 0.0));
    }
}
