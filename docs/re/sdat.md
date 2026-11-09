# SDAT: music, jingles and sound effects (SSEQ sequencer, SSAR archives, banks, waves)

Status: the container, the SSEQ/SBNK/SWAR of `bgm.sdat`, the sequence archives (SSAR) of the
effects in `se_norm.sdat`/`se_btl.sdat`, the players and groups, the choice of the sequence
played by a map and the scripts' sound natives are read and verified on the European ROM
(`YDQP`). The `dq9-audio` sequencer is a software port of the DS sound driver, written from
the published documentation; **it has not been compared by ear nor against melonDS** (see
"Not established").

Code: `crates/dq9-formats/src/sdat.rs` (container and files), `bgm.rs` (which sequence for
which map), `crates/dq9-audio` (sequencer, multi-sequence mixer, WAV),
`crates/dq9-app/src/audio.rs` (sound manager), `cargo dq9re sseq|sdat|se` (offline rendering
and inspection).

## Sources

In the order of `docs/external-resources.md`:

- the [SDAT](https://github.com/DQIX/wiki/blob/main/SDAT.md) page of the DQIX wiki: container,
  SYMB, INFO, FAT, SSEQ, SBNK, SWAR, banks and waves of `bgm.sdat` (no license: read, not
  copied);
- GBATEK "DS Sound" (formats and ADPCM); Gota7's Nitro Studio 2 specification (SSEQ command
  set);
- [SSEQPlayer](https://github.com/kode54/SSEQPlayer) (kode54) and [FSS](https://github.com/fincs/FSS)
  (fincs), both WTFPL: conversion tables (decibels, attack, decay) and command semantics,
  adapted with attribution in `tables.rs` and `player.rs`;
- Nintendo's sound library linked into the game is platform code: replaced, not translated.

## What we verified on the ROM

`crates/dq9-formats/tests/real_sdat.rs` (ignored without ROM):

| check | result |
|---|---|
| `data/sound/bgm.sdat` parses; sequences with a file | 64 |
| sequence → bank → wave archives chain | 64 / 64 resolve |
| PCM notes of the banks (all IMA ADPCM) | 2,079, all on an existing wave that decodes |
| `mapbgm.bin` | 67 entries (the header record announces 67) |
| music of every map in `maplist9.bin`, 4 times of day, flags off/on | always a sequence that has a file, or none |

`crates/dq9-audio/tests/real_rom.rs`: each of the 64 sequences produces, over its first 10
seconds, finite samples, below full scale, and is not silent.

## The sequencer

### Timing

The driver runs **192 times per second**; each pass adds the tempo (BPM) to a counter and
plays a *tick* each time it reaches 240. A tick is 1/48 of a quarter note: at 120 BPM, 96
ticks per second. Note lengths, waits and glides are counted in ticks; envelopes and
modulations update on each of the 192 passes. Measured by test
(`tempo_sets_the_note_length`): a quarter note lasts 0.500 s at 120 BPM and 0.250 s at 240.

### Commands (command byte, arguments)

All multi-byte arguments are little-endian, except the variable length (7 bits per byte,
most significant first, bit 7 = continuation). Jump offsets count from the start of the
stream (`0x18` of the file, i.e. `0x1C` on all sequences of the ROM).

| command | role |
|---|---|
| `00..7F n v d` | note of key `n`, velocity `v`, length `d` (variable length); if "note wait" is active the track waits `d` ticks. **`d = 0`: the note is held** until the end of the sequence [hypothesis, see below] |
| `80 d` | rest |
| `81 p` | program (bank instrument) |
| `93 t o₂₄` | opens track `t` at offset `o` |
| `94 o₂₄` / `95 o₂₄` / `FD` | jump / call / return (8 levels) |
| `D4 n` / `FC` | loop start (`n` passes, 0 = infinite) / loop end |
| `FE m₁₆` | mask of used tracks (ignored: tracks open via `93`) |
| `FF` | end of track |
| `A0 min₁₆ max₁₆`, `A1 v`, `A2` | prefixes: last argument drawn at random, read from a variable; `A2` executes the next command only if the condition is true |
| `B0..B6 v x₁₆` | variable: =, +, −, ×, ÷, shift, random; `B8..BD`: comparisons (==, ≥, >, ≤, <, ≠) that set the `A2` condition |
| `C0` / `C1` / `D5` | pan (0..127, 64 = center) / track volume / expression |
| `C2` | sequence master volume |
| `C3` / `C4` / `C5` | transpose / pitch bend (signed) / bend range (semitones) |
| `C7` / `C8` | note wait / tie (tied notes on the same voice) |
| `C9` / `CE` / `CF` | portamento key / portamento on / portamento time |
| `CA..CD`, `E0` | modulation: depth, speed, type (0 pitch, 1 volume, 2 pan), range, delay |
| `D0..D3` | attack, decay, sustain, release overriding the instrument's |
| `E1` | tempo (BPM, 16 bits) |
| `E3` | pitch sweep |
| `C6`, `D6` | priority, variable print: read, no effect |

An unknown command stops the track; a track that would execute 100,000 commands in a single
tick without waiting is stopped (guard against loops without waits). A random stream neither
panics nor hangs (`garbage_streams_end_without_panic`).

### Voices, envelope, volume

- One voice per note, 32 at most; beyond that the quietest one in release is reused,
  otherwise the oldest. The instrument comes from the bank: single note, drum set (one note
  per key) or key-range split (`Instrument::note`).
- **Envelope** (units of 1/128 of 1/10 dB, from −723 × 128 to 0): attack `a ← a × rate / 256`
  with `rate = 255 − attack` (table for attack ≥ 109), decay `a −= decay` down to the sustain
  level, release `a −= release` down to −723 × 128 where the voice dies. Decay and release:
  `127 → 0xFFFF`, `126 → 0x3C00`, `< 50 → 2v + 1`, otherwise `0x1E00 / (126 − v)`.
- **Volume**: sum in 1/10 dB of the `decibel_square` of velocity, track volume, expression,
  sequence volume (INFO record) and master volume, plus envelope and modulation; linear gain
  is `10^(dB/200)`. `decibel_square(v)` = `400·log10(v/127)` with floor −722 (the SDK table:
  −722, −721, −651, −601, −562… recovered by test). Linear **pan**, center at −6 dB on each
  side.
- **Pitch**: in 1/64 semitone around the wave's base key; pitch bend, sweep and modulation
  add to it. Measured by test: a 32-sample wave at 32,728.5 Hz sounds at 1,022.8 Hz on its
  base key and exactly one octave higher 12 keys above (`notes_sound_at_their_pitch`).
- **Waves**: IMA-ADPCM decoded once by `Song::load` (all waves of the bank), read with linear
  interpolation, looped at the loop point. **PSG**: square wave with 8 steps per cycle, duty
  cycle 1/8 to 7/8, key 60 frequency = `16756991 / (8 × 0x1F46)`; **noise**: 15-bit shift
  register clocked like the PSG.
- **Mixing**: voices are summed in stereo `f32` at the host rate; each voice's gains are
  ramped linearly between two driver updates (no click). The output gets a headroom gain of
  0.6 (−4.4 dB) then a soft limiter from 0.8.

### Offline rendering

```powershell
cargo dq9re sseq                                   # lists the sequences of bgm.sdat
cargo dq9re sseq 5 --wav build/audio/bg005.wav     # intro + one loop + 2 s fade
cargo dq9re sseq --map 1100 --time 0 --wav build/audio/af-night.wav
```

Measurements (release, 32,728 Hz, one loop pass then 2 s fade):

| sequence | tempo | tracks | command bytes | loop reached at | WAV length | mix peak* | written peak | RMS | full-scale samples |
|---|---|---|---|---|---|---|---|---|---|
| `BG_005` (Angel Falls, day) | 90 BPM | 13 | 20,848 | 80.35 s | 82.35 s | 1.519 | 0.901 | 0.133 | 0 |
| `BG_006` (Angel Falls, night) | 70 BPM | 13 | 7,656 | 58.32 s | 60.32 s | 0.820 | 0.492 | 0.095 | 0 |

\* Before headroom gain, at the DS mixer scale (1.0 = hardware clipping). Over all sequences
tried, from 0.35 (`ME_005`) to 2.04 (`BG_007`): the DS clips on the loudest ones, which the
headroom gain and the limiter avoid here. The exact equivalence of our gains with the
hardware mix (channel volume dividers) is **[hypothesis]**.

## Which sequence plays on a map

Found in the ARM9 code (Ghidra, European addresses):

1. `0x0209cd60` (`Bgm_SequenceForMap`) receives the map id. Special case of ids 5900 to
   5999: if byte `+0x2788` of the zone object is 0 and its halfword `+0x2786` is 1900, the
   id becomes 1900: **not modeled** **[unknown]** (what these fields are is not read).
2. It reads the map record (`0x02099960`, see `maplist.md`): `+0xD` bits 0-6, the **music
   slot** (slot 6), goes through `0x0209cb00` (`Bgm_SlotToSequence`).
3. `0x0209cb00`: slot ≥ 47 → −1; slot → `g_BgmSlotTable` (`0x020e8ee8`, 47 signed bytes,
   verified on the ROM: identity except 0, 9, 25, 39 and 42 which are −1); **at night** (time
   of day 0 in `LightingManager+0x98`) 3 becomes 4 and 5 becomes 6; slot 14 (field regions)
   becomes 16 under a global condition (`0x0202ae28`/`0x0202b7e8`, unidentified object)
   **[unknown]**: `dq9-formats` keeps 14.
4. Finally `0x0209cc4c` (`Bgm_FindOverride`) walks `data/bin/mapbgm.bin`: an entry is a
   32-bit word (map id bits 0-15, sequence bits 16-21, story flag bits 22-30, bit 31 "night
   only"); the first entry for the map whose story flag is set replaces the sequence
   (0 = silence). Fully reimplemented: `bgm::Overrides::find`.
5. The loader `0x0209c3c4` (`Bgm_Select`) receives this id as is: nothing if it is already the
   current sequence, otherwise it stops the old one and loads the new one. The id is the index
   in the sequence list of the `bgm.sdat` INFO, as the wiki deduced from `maplist9.bin` (here
   confirmed by the code, no longer only by the values). Native 736 (`ov_001::02163aec`)
   chains these two calls for the current location.

Angel Falls: map 1100 (`M01`, slot 5) plays `BG_005` by day and `BG_006` by night; its church
(1106, slot 10) `BG_010`; the surrounding region (slot 14) `BG_014`. The application takes the
time from `--time`/`--clock` (night by default: `BG_006`).

What is not read: the loader call from the field (`ov_017`, three calls to `0x0209c3c4` at
`0x0219efe4`, `0x021b19c8`, `0x021bcd34`) has not been decompiled: the application chooses
the sequence on each map load, which is the expected effect of 736, without having proven
that the game calls it at the same moment.

## Archives, players and groups (`se_norm.sdat`, `se_btl.sdat`)

`dq9re sdat <bgm|se_norm|se_btl> [list [index]]` lists the eight INFO record lists with their
names (`Sdat::record`, `Sdat::name`). Read from the ROM (slots / filled):

| list | `bgm.sdat` | `se_norm.sdat` | `se_btl.sdat` |
|---|---|---|---|
| sequences (SSEQ) | 82 / 64 | 0 | 0 |
| sequence archives (SSAR) | 0 | 462 / 279 | 936 / 481 |
| banks | 82 / 60 | 462 / 279 | 936 / 481 |
| wave archives | 81 / 59 | 108 / 3 | 105 / 5 |
| players | 8 / 7 | 16 / 6 (numbers 10 to 15) | 17 / 5 (10 to 13 and 16) |
| groups | 82 / 64 | 462 / 279 | 936 / 481 |

- **Sequence record** (12 bytes): `+0` file, `+4` bank, `+6` volume, `+7` channel priority,
  `+8` player priority, `+9` **player number**. The DQIX wiki shifts them by one ("polyphonic
  pressure" at `+8`, "player priority" at `+9`); on the ROM `+9` is 2 for `BG_001` and 0 for
  `BG_002` to `BG_005`, and `+8` is 0: only the SDK layout (`SNDSeqParam`: `cpr`, `ppr`,
  `ply`) gives existing player numbers (10 to 15 for all effects). Fixed in `SequenceInfo`
  (`channel_priority`, `player_priority`, `player`).
- **Player** (8 bytes): `+0` number of simultaneous sequences (u16), `+2` hardware channel
  mask (u16), `+4` heap (u32). `se_norm.sdat`: `PLAYER_NM` 4 sequences channels 0-3,
  `PLAYER_FT` 1 (0-3), `PLAYER_SR` 8 (channels 4-15), `PLAYER_NN` 1, `PLAYER_EV` 4,
  `PLAYER_PO` 1; `bgm.sdat`: `PLAYER_BG12` 1 sequence (channels 4-15), `BG16` 2 (0-15),
  `BG14` 2 (2-15), `BGR` 1, `ME12` 1, `ME16` 2, `ME14` 2. The player numbers of `bgm.sdat`
  (0 to 7) and `se_norm.sdat` (10 to 15) do not overlap: they form a single numbering space.
- **Group**: a count then 8-byte entries `{type u8, flags u8, 0, 0, index u32}`; types 0
  sequence, 1 bank, 2 wave archive, 3 sequence archive, flags bit 0 the file, bit 1 the bank,
  bit 2 the waves. Group `n` of `se_norm.sdat` loads bank `n` and sequence archive `n`
  (verified on `GRP_SY_001` = 100 and `GRP_SP_ev05121` = 278): native 726 called with a
  single argument therefore plays the archive with the same number.
- **Sequence archive (SSAR)**: `0x18` stream start, `0x1C` number of entries, then 12-byte
  entries `{offset u32, bank u16, volume u8, channel priority u8, player priority u8, player
  u8, 0, 0}`; an offset of `0xFFFFFFFF` marks an empty slot. Track and jump offsets count from
  the stream start (same reference as the entries). `Ssar::parse`. The field's base archive,
  `se_norm.sdat` no. 100, has 205 slots of which 54 are filled (bank 100 for all, channel
  priority 96, player priority 64 except effect 152 at 54, players 10 to 15); its effects 200
  to 204 are the door ones (see below).
- **Length-0 notes**: effects are written `81 p  n 7f 00  ff` (program, key, velocity,
  length 0, end of track). With "held until the end of the sequence then released" these
  effects died immediately (muted rendering of effects 12, 10, 18, 200…). The sequencer
  therefore lets held notes whose wave does not loop play to the end; a looping wave is
  released at the end of the tracks **[hypothesis]**. On the ROM the 54 effects of archive
  100 sound and end on their own in under 20 s (test `field_effects_sound_and_end`), the
  measured ones last 0.05 to 0.65 s.

## The game's sound manager (natives 712 to 738)

The manager is the singleton `0x02108760`. Fields read in `Se_*` and `Bgm_*`
(`re/symbols.txt`, "Sound" section): `+0xb4` base archive (100 in the field, 101 in battle),
`+0xbc` archive loaded by 730, `+0xc4` archive loaded by 726, `+0xc8` lock bits (bit 2:
native 735), `+0xc9` jingle state, `+0xce` jingle sequence.

| native | role | reproduced by |
|---|---|---|
| 726 / 730 `(group[, archive])` | loads the group then selects the archive (by default the group number) at `+0xc4` / `+0xbc` (`Se_LoadGroup` `0x0205eb64`, `Se_LoadSecondaryGroup` `0x0205eae0`) | `Sound::LoadEffects`: the archive is decoded (bank and waves) and cached |
| 727 / 731 | releases the DS heap | no effect: decoded archives stay cached |
| 728 / 732 `(index[, &handle])` | plays effect `index` of archive `+0xc4` / `+0xbc` (`Se_PlayLoaded` `0x0205eba0`, `Se_PlaySecondary` `0x0205eb34`) on one of the scripts' 16 handles (or on the manager's own without an address) | `Sound::PlayEffect`, `handle` −1 without an address |
| 712 `(index)` | plays on the **base** archive (`Se_PlayBase` `0x0205eab0`); with an address, same as 728 (`ov_001::02163564`) | `Sound::PlayBaseEffect` / `PlayEffect` |
| 729 / 723 `(handle, duration)`, 733 `(handle)` | stops the effect, with or without fade | `Sound::StopEffect` |
| 713 `(sequence)` | `Bgm_Select` then stop | `Sound::SelectBgm` |
| 714 | restarts the selected sequence at volume 127 (`Bgm_Restart` `0x0209c5f8`) | `Sound::RestartBgm` |
| 715 `(volume, duration)` | volume of the music and of players 5 to 7 (`Bgm_SetVolume` `0x0209c2f0`), duration in frames **[hypothesis]** | `Sound::BgmVolume`, linear fade |
| 721 `(duration)` | stop with fade (`Bgm_Stop` `0x0209c688`) | `Sound::StopBgm` |
| 720 `(sequence)`, 725 `(&busy)` | **jingle**: see below; 725 is true until the jingle has finished | `Sound::Jingle`, `Signal::JingleEnded` |
| 736 | `Bgm_SequenceForMap` of the location then `Bgm_Select` | `Sound::ZoneBgm` |
| 737 `(sequence)` | stops the music and plays the sequence alone, without resume (`Bgm_PlaySequence` `0x0209c6e8`); observed ids 67 (`ME_018`) and 55 (`ME_006`) | `Sound::PlaySequence` |
| 738 | resets everything, volume 127 (`Bgm_Reset` `0x0209c21c`) | `Sound::ResetBgm` |
| 735 `(enabled)` | lock: select, restart and stop have no effect | `Sound::BgmEnabled` |
| 734 `(mode)` | audio memory mode (BGM heap `0x57000` / `0xa2000`, opens or closes the base archive) | no equivalent (DS memory) |
| 554 `(voice)` | `W+0x99c`: text blip voice (0, 1, 2) | `Sound::TextVoice` |

**Jingle** (720, `Bgm_UpdateJingle` `0x0209c850`, states 1 to 3 of `+0xc9`): on request the
music goes down to 0 in 20 frames; after 800 ms it is paused (not stopped) and the jingle
sequence starts; when it has finished, 500 ms later the music resumes with a 30-frame fade
and `+0xc9` goes back to 0 (that is when 725 becomes false). `Audio::step` replays this
sequence (48, 30 and 30 frames of 1/60 s). Muted or without a device, the request is
finished immediately (otherwise the script would wait forever).

Sounds of the prologue scripts (`dq9-app --stage 1.1 --headless-steps 30000 --press-every 20
--sound-wav build/audio/open.wav`, sound log): `ME_018` (id 67, native 737) during
`ev21507`; effects 0 to 20 of the archives loaded by each cutscene's groups (278 for
`ev01130`, effect 12); the music `BG_005` restarted. The WAV is 123 s long, peak 0.948. Order
and durations are not compared against melonDS.

### Field sounds (base archive, `se_norm.sdat` no. 100)

Found by decompiling the callers of `Se_PlayBase` (`0x0205eab0`, about a hundred sites):

| index | when | where |
|---|---|---|
| 10, 12, 11 | blip of a typed character, depending on the voice (0, 1, 2); cut when `+0x19b2` is 0 (native 413, "mute text" according to the wiki) | `MsgWin_PlayTextBlip` `0x02045608`, called by `MsgWin_Update` when counter `+0x912` exceeds 3 during typing: one blip every 4 steps **[hypothesis: one character per step]** |
| 6, 28 | `<EXC>` (`0xFF2E`) and `<QES>` (`0xFF2F`); the game then sets `+0x1959` to 60 (bubble, probable wait): **wait not reproduced** | `MsgWin_Update` `0x020659a0` |
| 5 | opening of a choice (`<YESNO>`, `<UKEYAME>`…) | `MsgWin_Update` |
| 1 | answer to a choice: first and second answers for `0xFF04`/`0xFF05`/`0xFF08`; none for `<YESNO_NOTSE>` (`0xFF06`); first only for `<YESNO_NOTSE_IIE>` (`0xFF07`); B (cancel, −2) is silent | `MsgWin_AnswerYesNo` `0x020678cc` (`0xFF04`..`0xFF07`) and `MsgWin_AnswerAcceptDecline` `0x020679e4` (`0xFF08`), called through table `0x020f08b8` of `MsgWin_UpdatePrompt` `0x020676f0` |
| 101 | `<ST=…>` (`0xFF1C`) with a valid choice **[hypothesis]** | `MsgWin_Update`, not wired |
| 14 | code `0xFF4B` (unnamed) | `MsgWin_Update`, not wired |
| 200 to 204 | region door: `g_DoorSeTable[flags & 0xF]` (`0x020e6e84`) | `Region_Fire` `0x02018310` **[hypothesis: the door role]**; not wired (the application's door transition does not go through this function) |
| 18, 19 (`0x12`, `0x13`) | opening and closing of an animated door (wiki, `func_02015554`) | not wired: the door task does not exist yet |
| 50 (`0x32`) | puff of an NPC that disappears | `0x020486a0`, not wired |
| 96 (`0x60`) | state change of a field object | `0x02034054`, not wired |

**Message choice**: `MsgWin_UpdatePrompt` (`0x020676f0`) returns 0 (A on the first answer),
1 (A on the second) or −2 (B), then calls the handler of the choice type through the static
table `0x020f08b8` (two entries: `0x020678cc` for `<YESNO>`, `<NOYES>`, `<YESNO_NOTSE>` and
`<YESNO_NOTSE_IIE>`; `0x020679e4` for `<UKEYAME>`). The first reads the prompt code from the
stream (`0xFF04` to `0xFF07`) and plays effect 1 of the base archive: `0xFF06` plays for no
answer; `0xFF07` plays for the first and stays silent for the second; `0xFF04` and `0xFF05`
play for both. The second (`<UKEYAME>`) always plays. Both skip the sound block for B (−2),
which answers "no" without an effect. Slot 1 of archive 100 is indeed filled (stream offset
`0x144`, bank 100, volume 61). Up/down movement (`0x0205cedc` → `0x0205c580` → `0x0205bb94`,
or `0x02045750` when `+0x19bc` is set) plays no effect: no caller of `Se_PlayBase` on that
path.

**Footsteps**: `Object3D::AdvanceAnimations` (`0x02034ea0`) plays the effect whose index is
in a linked list of the animation (`animation + 0x20`), on archive `object + 0x7a`, when the
frame passes the note's frame: these are the footstep and gesture sounds, carried by the
animation data. Neither the format of this list nor each character's archive is read
**[unknown]**: footsteps are not reproduced.

These effects are rendered by `dq9re se <index> [--archive 100]`; measurements (32,728 Hz,
peak before limiter): 5 (0.645 s, 0.327), 6 (0.609 s, 0.452), 10 (0.056 s, 0.183), 12
(0.073 s, 0.299), 20 (0.151 s, 0.380), 28 (0.340 s, 0.325), 152 (0.071 s, 0.007).

## Output in `dq9-app`

- `audio.rs`: on map load, `Audio::set_map` chooses the sequence (time of the day phase,
  story flags from the trigger state), loads the song (cache of 6) and hands it to the mixer;
  a sequence already playing is not restarted; a change fades the old one out in 0.4 s. The
  engine (`Engine`) is behind a `Mutex` and produces the mix (`dq9_audio::Mixer`) in the
  device callback, at the device rate; if that differs from 48 kHz, the mixer is rebuilt and
  the music restarted on open.
- `Mixer`: several `Player`s at once (music, jingle, effects), each on the player of its
  record. A full player (`PLAYER`: simultaneous sequences) yields its place to the
  lowest-priority sequence, the oldest on a tie; if all those playing have a strictly higher
  priority, the new one is refused **[hypothesis: the SDK rule has not been re-read]**.
  Hardware channels (masks) are not modeled.
- Events: `EventPlayer` returns `Outcome::Sound`; `Audio::script_sound` executes it. Message
  text (`dq9_core::talk::Sound`) goes through `Talk::take_sounds` then `Audio::talk_sound`.
  Each fixed step calls `Audio::step` (jingle waits).
- `--sound-wav f.wav` with `--headless-steps N`: sound is mixed offline at 32,728 Hz (no
  device) and written as WAV; the log lists each script effect.
- Native: `cpal` (stream opened on first need; `--mute` opens no device; `M` mutes and
  unmutes). Web: `AudioContext` + `ScriptProcessorNode` created and resumed on the first
  gesture (keyboard, mouse); `?mute=1`.

## Not established

- **Listening and comparison**: no comparison by ear nor against a melonDS capture; the
  measurements above are objective properties (pitch, duration, levels).
- **Length-0 note**: held until the end of the sequence for a looping wave, until the end of
  the wave otherwise. `BG_100` and `BG_101` only make sense that way (note `30 7f 00` followed
  by `C4` sweeping the pitch); the SDK semantics for a length of 0 have not been re-read in
  its code **[hypothesis]**.
- Hardware channels (16; PSG on 8-13, noise on 14-15 not reproduced: 32 arbitrary voices per
  sequence), channel priority (`C6`, `+7` of the records), exact replacement rule for a full
  player, player setting (volumes 0..4, table `{0, 0.378, 0.669, 0.850, 1.0}` at
  `0x020e8ec4`: we play at maximum), unit of fade durations (taken as 1/60 s frames), STRM
  streams, battle effects (`se_btl.sdat`, base archive 101).
- Field sounds not wired: doors, `<EXC>`/`<QES>` bubbles (60-frame wait), footsteps and
  gestures (animation sound lists). Choice confirmation is wired (effect 1); cursor movement
  is silent based on the absence of a `Se_PlayBase` caller on its path, without listening to
  confirm it.
- Portamento and sweep: durations taken from the note length when the duration is 0; the
  SDK's exact formula in driver steps is not verified **[hypothesis]**.
- The 25 maps with slot 80 give `BG_100`; slot 25 (no file, 2 maps) gives silence, as in the
  table.
