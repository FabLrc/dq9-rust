use std::collections::BTreeMap;

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
