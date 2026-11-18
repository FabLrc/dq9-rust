//! `dq9rec` v1: the record of a human session and what replays it (`docs/app.md`,
//! § "Recording a human session").
//!
//! The game logic reads the outside world through a short list of inputs: the keys
//! [`KEYS`] (`App::key`), the mouse drag and wheel (the camera's yaw is the walk direction),
//! the focus loss (`held.clear()`) and the end of a jingle a script waits for (the sound
//! device decides when it ends, by wall time). Everything else a window does (layout, effects,
//! mute, resize) never reaches the logic and is not recorded. Each input is stamped with the
//! number of fixed 60 Hz steps completed when it was handled, so a replay is indexed by the
//! step and the pacing of the window's frames cannot matter.
//!
//! File: a JSON header line, then one line per input or digest:
//!
//! ```text
//! {"format":"dq9rec","version":1,"rom":"YDQP01",...}
//! +120 ArrowUp          key pressed at step 120
//! -163 ArrowUp          key released (a key held for 43 steps is two lines: a run)
//! !400                  focus lost: every held key is released
//! j512                  a jingle a script waits for ended
//! m730 3d4ccccd 0 0     mouse drag (x, y) and wheel, f32 bits in hex
//! =600 map=1100 ...     state digest (every 600 steps), the replay checks it
//! .3000                 end: steps completed
//! ```
//!
//! The file holds no ROM data: the ROM is identified by its header (game code, version, CRC)
//! and a save by a hash of its bytes.

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::fmt::Write as _;

use winit::keyboard::KeyCode;

/// `format` field of the header.
pub const FORMAT: &str = "dq9rec";
/// The only version this reader knows.
pub const VERSION: u32 = 1;
/// Fixed steps between two digests.
pub const DIGEST_EVERY: u64 = 600;

/// The keys the game logic reads, with their file names (winit's `KeyCode` variant names).
const KEYS: [(KeyCode, &str); 23] = [
    (KeyCode::ArrowUp, "ArrowUp"),
    (KeyCode::ArrowDown, "ArrowDown"),
    (KeyCode::ArrowLeft, "ArrowLeft"),
    (KeyCode::ArrowRight, "ArrowRight"),
    (KeyCode::KeyW, "KeyW"),
    (KeyCode::KeyA, "KeyA"),
    (KeyCode::KeyS, "KeyS"),
    (KeyCode::KeyD, "KeyD"),
    (KeyCode::Enter, "Enter"),
    (KeyCode::Space, "Space"),
    (KeyCode::KeyZ, "KeyZ"),
    (KeyCode::Backspace, "Backspace"),
    (KeyCode::KeyX, "KeyX"),
    (KeyCode::Escape, "Escape"),
    (KeyCode::KeyQ, "KeyQ"),
    (KeyCode::KeyE, "KeyE"),
    (KeyCode::KeyI, "KeyI"),
    (KeyCode::KeyH, "KeyH"),
    (KeyCode::KeyT, "KeyT"),
    (KeyCode::Tab, "Tab"),
    (KeyCode::KeyF, "KeyF"),
    (KeyCode::PageUp, "PageUp"),
    (KeyCode::PageDown, "PageDown"),
];

/// The file name of a key the logic reads; `None` for the keys it never sees (mute, layout,
/// shift, ...).
pub fn key_name(code: KeyCode) -> Option<&'static str> {
    KEYS.iter().find(|(key, _)| *key == code).map(|(_, n)| *n)
}

fn key_from_name(name: &str) -> Option<KeyCode> {
    KEYS.iter().find(|(_, n)| *n == name).map(|(key, _)| *key)
}

/// One input of the logic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Key {
        key: KeyCode,
        pressed: bool,
    },
    /// The window lost focus: the held keys are forgotten.
    FocusLost,
    /// A jingle a script waits for has ended (`Audio::take_jingle_ended`).
    JingleEnded,
    /// Mouse drag (yaw, pitch) and wheel, as the window added them to the camera input.
    Mouse {
        rotate: [f32; 2],
        zoom: f32,
    },
}

/// What the start of the session was: the options that change the logic.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// Game code and maker code of the ROM header (`YDQP01`).
    pub rom: String,
    pub rom_version: u8,
    /// Header CRC-16 (`0x15E`) of the ROM.
    pub rom_crc: u16,
    pub new_game: bool,
    /// Seed of the battles' generators.
    pub seed: u64,
    pub map: Option<String>,
    /// FNV-1a 64 of the save file the session started from.
    pub save: Option<String>,
    pub time: Option<u32>,
    pub clock: Option<f32>,
    pub chapter: Option<u8>,
    pub step: Option<u8>,
    pub substep: Option<u8>,
    pub night: bool,
    pub stage: Option<(u32, u32)>,
    pub event: Option<u16>,
    /// Battle presentation: `show` (the window's), the only mode a human session has.
    pub battles: String,
}

/// A record: header, inputs and digests in step order.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub header: Header,
    pub inputs: Vec<(u64, Input)>,
    pub digests: Vec<(u64, Digest)>,
    /// Steps completed when the session ended; a file cut short ends at its last line.
    pub end: u64,
}

/// The state a replay compares: `name=value` pairs, without spaces.
pub type Digest = Vec<(String, String)>;

/// FNV-1a 64: a stable hash (the standard library's are not promised stable).
pub fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn json_str(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_opt<T: std::fmt::Display>(value: &Option<T>) -> String {
    value
        .as_ref()
        .map_or_else(|| "null".to_owned(), ToString::to_string)
}

impl Header {
    /// The ROM's identity as a header stores it: game code and maker code (`YDQP01`),
    /// version and header CRC-16. Never ROM data.
    pub fn rom_identity(rom: &[u8]) -> Result<(String, u8, u16), String> {
        let header = nds_rom::Header::parse(rom).map_err(|e| format!("the ROM header: {e}"))?;
        Ok((
            format!("{}{}", header.game_code, header.maker_code),
            header.version,
            header.header_crc,
        ))
    }

    /// The hash a header keeps of the save file a session started from.
    pub fn save_hash(save: &[u8]) -> String {
        format!("{:016x}", fnv64(save))
    }

    /// Whether this ROM and save are what the session started from.
    pub fn check_start(&self, rom: &[u8], save: Option<&[u8]>) -> Result<(), String> {
        let (code, version, crc) = Self::rom_identity(rom)?;
        if (code.as_str(), version, crc) != (self.rom.as_str(), self.rom_version, self.rom_crc) {
            return Err(format!(
                "the record was played on ROM {} v{} (CRC {:04x}); this one is {code} v{version} (CRC {crc:04x})",
                self.rom, self.rom_version, self.rom_crc
            ));
        }
        match (&self.save, save) {
            (Some(wanted), Some(bytes)) if *wanted == Self::save_hash(bytes) => Ok(()),
            (Some(wanted), _) => Err(format!(
                "the record started from a save (hash {wanted}): pass that file"
            )),
            (None, Some(_)) => Err("the record did not start from a save".to_owned()),
            (None, None) => Ok(()),
        }
    }

    fn to_json(&self) -> String {
        let stage = self
            .stage
            .map_or_else(|| "null".to_owned(), |(a, b)| format!("\"{a}.{b}\""));
        let save = self
            .save
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_str);
        let map = self
            .map
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_str);
        format!(
            "{{\"format\":\"{FORMAT}\",\"version\":{VERSION},\"rom\":{},\"rom_version\":{},\
             \"rom_crc\":\"{:04x}\",\"new_game\":{},\"seed\":{},\"map\":{map},\"save\":{save},\
             \"time\":{},\"clock\":{},\"chapter\":{},\"step\":{},\"substep\":{},\"night\":{},\
             \"stage\":{stage},\"event\":{},\"battles\":{},\"digest_every\":{DIGEST_EVERY}}}",
            json_str(&self.rom),
            self.rom_version,
            self.rom_crc,
            self.new_game,
            self.seed,
            json_opt(&self.time),
            json_opt(&self.clock),
            json_opt(&self.chapter),
            json_opt(&self.step),
            json_opt(&self.substep),
            self.night,
            json_opt(&self.event),
            json_str(&self.battles),
        )
    }

    fn from_json(line: &str) -> Result<Self, String> {
        let fields = parse_object(line)?;
        let get = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
                .ok_or_else(|| format!("header: no `{name}`"))
        };
        let text = |name: &str| match get(name)? {
            Json::Str(text) => Ok(text.clone()),
            _ => Err(format!("header: `{name}` is not a string")),
        };
        let flag = |name: &str| match get(name)? {
            Json::Bool(flag) => Ok(*flag),
            _ => Err(format!("header: `{name}` is not a boolean")),
        };
        fn number<T: std::str::FromStr>(name: &str, value: &Json) -> Result<Option<T>, String> {
            match value {
                Json::Null => Ok(None),
                Json::Num(text) => text
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("header: `{name}` is not a valid number: {text}")),
                _ => Err(format!("header: `{name}` is not a number")),
            }
        }
        let opt_text = |name: &str| match get(name)? {
            Json::Null => Ok(None),
            Json::Str(text) => Ok(Some(text.clone())),
            _ => Err(format!("header: `{name}` is not a string")),
        };
        if text("format")? != FORMAT {
            return Err("not a dq9rec file".to_owned());
        }
        let version: Option<u32> = number("version", get("version")?)?;
        if version != Some(VERSION) {
            return Err(format!(
                "dq9rec version {version:?} is not supported (this build reads version {VERSION})"
            ));
        }
        let digest_every: Option<u64> = number("digest_every", get("digest_every")?)?;
        if digest_every != Some(DIGEST_EVERY) {
            return Err(format!(
                "digest_every {digest_every:?} is not {DIGEST_EVERY}"
            ));
        }
        let rom_crc = u16::from_str_radix(&text("rom_crc")?, 16)
            .map_err(|_| "header: `rom_crc` is not hexadecimal".to_owned())?;
        let stage = match opt_text("stage")? {
            None => None,
            Some(text) => {
                let (major, minor) = text.split_once('.').ok_or("header: bad `stage`")?;
                Some((
                    major.parse().map_err(|_| "header: bad `stage`")?,
                    minor.parse().map_err(|_| "header: bad `stage`")?,
                ))
            }
        };
        Ok(Self {
            rom: text("rom")?,
            rom_version: number("rom_version", get("rom_version")?)?
                .ok_or("header: no `rom_version`")?,
            rom_crc,
            new_game: flag("new_game")?,
            seed: number("seed", get("seed")?)?.ok_or("header: no `seed`")?,
            map: opt_text("map")?,
            save: opt_text("save")?,
            time: number("time", get("time")?)?,
            clock: number("clock", get("clock")?)?,
            chapter: number("chapter", get("chapter")?)?,
            step: number("step", get("step")?)?,
            substep: number("substep", get("substep")?)?,
            night: flag("night")?,
            stage,
            event: number("event", get("event")?)?,
            battles: text("battles")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
}

/// Parses one flat JSON object (string, number, boolean and null values).
fn parse_object(line: &str) -> Result<Vec<(String, Json)>, String> {
    let mut chars = line.trim().chars().peekable();
    let mut fields = Vec::new();
    let bad = || "header: not a flat JSON object".to_owned();
    if chars.next() != Some('{') {
        return Err(bad());
    }
    fn string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
        if chars.next()? != '"' {
            return None;
        }
        let mut out = String::new();
        loop {
            match chars.next()? {
                '"' => return Some(out),
                '\\' => out.push(chars.next()?),
                c => out.push(c),
            }
        }
    }
    loop {
        match chars.peek() {
            Some('}') => {
                chars.next();
                return Ok(fields);
            }
            Some(',') => {
                chars.next();
            }
            _ => {}
        }
        let key = string(&mut chars).ok_or_else(bad)?;
        if chars.next() != Some(':') {
            return Err(bad());
        }
        let value = match chars.peek() {
            Some('"') => Json::Str(string(&mut chars).ok_or_else(bad)?),
            _ => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c == ',' || c == '}' {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                match word.as_str() {
                    "null" => Json::Null,
                    "true" => Json::Bool(true),
                    "false" => Json::Bool(false),
                    _ => Json::Num(word),
                }
            }
        };
        fields.push((key, value));
    }
}

fn input_line(step: u64, input: &Input) -> String {
    match input {
        Input::Key { key, pressed } => format!(
            "{}{step} {}",
            if *pressed { '+' } else { '-' },
            key_name(*key).unwrap_or("?")
        ),
        Input::FocusLost => format!("!{step}"),
        Input::JingleEnded => format!("j{step}"),
        Input::Mouse { rotate, zoom } => format!(
            "m{step} {:08x} {:08x} {:08x}",
            rotate[0].to_bits(),
            rotate[1].to_bits(),
            zoom.to_bits()
        ),
    }
}

fn parse_bits(word: &str) -> Result<f32, String> {
    u32::from_str_radix(word, 16)
        .map(f32::from_bits)
        .map_err(|_| format!("bad float bits `{word}`"))
}

impl Record {
    /// The file's text.
    pub fn to_text(&self) -> String {
        let mut out = self.header.to_json();
        out.push('\n');
        // Inputs and digests are two sorted lists; merge them by step (inputs first: a replay
        // applies the inputs of a step before it checks that step's digest).
        let (mut i, mut d) = (0, 0);
        while i < self.inputs.len() || d < self.digests.len() {
            let input_first = match (self.inputs.get(i), self.digests.get(d)) {
                (Some((a, _)), Some((b, _))) => a <= b,
                (Some(_), None) => true,
                _ => false,
            };
            if input_first {
                let (step, input) = &self.inputs[i];
                out.push_str(&input_line(*step, input));
                i += 1;
            } else {
                let (step, digest) = &self.digests[d];
                let _ = write!(out, "={step}");
                for (name, value) in digest {
                    let _ = write!(out, " {name}={value}");
                }
                d += 1;
            }
            out.push('\n');
        }
        let _ = writeln!(out, ".{}", self.end);
        out
    }

    /// Reads a file's text; any malformed line is an error naming it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let header = Header::from_json(lines.next().ok_or("empty record")?)?;
        let mut record = Self {
            header,
            inputs: Vec::new(),
            digests: Vec::new(),
            end: 0,
        };
        let mut last = 0u64;
        for (index, line) in lines.enumerate() {
            let number = index + 2;
            let fail = |why: &str| format!("line {number}: {why}: `{line}`");
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            let tag_len = line.chars().next().map_or(0, char::len_utf8);
            let (tag, rest) = line.split_at(tag_len);
            let (step_text, args) = rest.split_once(' ').unwrap_or((rest, ""));
            let step: u64 = step_text.parse().map_err(|_| fail("bad step"))?;
            if step < last {
                return Err(fail("steps go backwards"));
            }
            last = step;
            match tag {
                "+" | "-" => {
                    let key = key_from_name(args).ok_or_else(|| fail("unknown key"))?;
                    let pressed = tag == "+";
                    record.inputs.push((step, Input::Key { key, pressed }));
                }
                "!" => record.inputs.push((step, Input::FocusLost)),
                "j" => record.inputs.push((step, Input::JingleEnded)),
                "m" => {
                    let words: Vec<&str> = args.split(' ').collect();
                    let [x, y, zoom] = words[..] else {
                        return Err(fail("a mouse line has three values"));
                    };
                    let bits = |w| parse_bits(w).map_err(|e| fail(&e));
                    record.inputs.push((
                        step,
                        Input::Mouse {
                            rotate: [bits(x)?, bits(y)?],
                            zoom: bits(zoom)?,
                        },
                    ));
                }
                "=" => {
                    let mut digest = Digest::new();
                    for pair in args.split(' ').filter(|p| !p.is_empty()) {
                        let (name, value) =
                            pair.split_once('=').ok_or_else(|| fail("bad field"))?;
                        digest.push((name.to_owned(), value.to_owned()));
                    }
                    record.digests.push((step, digest));
                }
                "." => record.end = step,
                _ => return Err(fail("unknown line")),
            }
        }
        record.end = record
            .end
            .max(record.inputs.last().map_or(0, |(s, _)| *s))
            .max(record.digests.last().map_or(0, |(s, _)| *s));
        Ok(record)
    }
}

/// A session being recorded.
#[derive(Debug)]
pub struct Recorder {
    record: Record,
    #[cfg(not(target_arch = "wasm32"))]
    path: Option<std::path::PathBuf>,
}

impl Recorder {
    pub fn new(header: Header) -> Self {
        Self {
            record: Record {
                header,
                inputs: Vec::new(),
                digests: Vec::new(),
                end: 0,
            },
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
        }
    }

    /// A recorder that writes its file at every digest and at the end.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn to_file(header: Header, path: std::path::PathBuf) -> Self {
        let mut recorder = Self::new(header);
        recorder.path = Some(path);
        recorder
    }

    pub fn input(&mut self, step: u64, input: Input) {
        self.record.inputs.push((step, input));
    }

    pub fn digest(&mut self, step: u64, digest: Digest) {
        if self
            .record
            .digests
            .last()
            .is_some_and(|(at, _)| *at == step)
        {
            return;
        }
        self.record.digests.push((step, digest));
        #[cfg(not(target_arch = "wasm32"))]
        self.write();
    }

    /// The record so far, ended at `steps`.
    pub fn finish(&mut self, steps: u64) -> &Record {
        self.record.end = steps;
        #[cfg(not(target_arch = "wasm32"))]
        self.write();
        &self.record
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn write(&self) {
        let Some(path) = &self.path else { return };
        if let Err(e) = std::fs::write(path, self.record.to_text()) {
            crate::app::status(&format!("record: cannot write {}: {e}", path.display()));
        }
    }
}

/// A record being replayed: the inputs due at each step, the digests to check.
#[derive(Debug)]
pub struct Replay {
    record: Record,
    next_input: usize,
    next_digest: usize,
    checked: usize,
    failure: Option<String>,
    reported: bool,
}

impl Replay {
    pub fn new(record: Record) -> Self {
        Self {
            record,
            next_input: 0,
            next_digest: 0,
            checked: 0,
            failure: None,
            reported: false,
        }
    }

    /// `true` the first time it is asked: the window says how the replay ended once.
    pub fn first_report(&mut self) -> bool {
        !std::mem::replace(&mut self.reported, true)
    }

    /// Steps the session lasted.
    pub fn end(&self) -> u64 {
        self.record.end
    }

    /// The next input due at `step` (stamped at or before it), in file order.
    pub fn next_input(&mut self, step: u64) -> Option<Input> {
        let &(at, input) = self.record.inputs.get(self.next_input)?;
        if at > step {
            return None;
        }
        self.next_input += 1;
        Some(input)
    }

    /// Whether a digest is due at `step`.
    pub fn digest_due(&self, step: u64) -> bool {
        self.record
            .digests
            .get(self.next_digest)
            .is_some_and(|(at, _)| *at <= step)
    }

    /// Compares the replay's state at `step` with the recorded digest: the first mismatch
    /// is kept, with the step and the field.
    pub fn check(&mut self, step: u64, actual: &Digest) {
        let Some((at, expected)) = self.record.digests.get(self.next_digest) else {
            return;
        };
        let at = *at;
        self.next_digest += 1;
        self.checked += 1;
        if self.failure.is_some() {
            return;
        }
        for (name, value) in expected {
            let seen = actual.iter().find(|(n, _)| n == name).map(|(_, v)| v);
            if seen != Some(value) {
                self.failure = Some(format!(
                    "digest mismatch at step {step} (recorded at {at}): `{name}` recorded {value}, replayed {}",
                    seen.map_or("(absent)", String::as_str)
                ));
                return;
            }
        }
    }

    /// The first digest mismatch, if any.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Digests compared so far.
    pub fn checked(&self) -> usize {
        self.checked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            rom: "YDQP01".into(),
            rom_version: 0,
            rom_crc: 0xab12,
            new_game: true,
            seed: 7,
            map: None,
            save: Some("0123456789abcdef".into()),
            time: Some(2),
            clock: Some(12.5),
            chapter: None,
            step: Some(3),
            substep: None,
            night: false,
            stage: Some((1, 4)),
            event: Some(2750),
            battles: "show".into(),
        }
    }

    fn record() -> Record {
        Record {
            header: header(),
            inputs: vec![
                (
                    3,
                    Input::Key {
                        key: KeyCode::ArrowUp,
                        pressed: true,
                    },
                ),
                (
                    9,
                    Input::Key {
                        key: KeyCode::ArrowUp,
                        pressed: false,
                    },
                ),
                (9, Input::FocusLost),
                (12, Input::JingleEnded),
                (
                    600,
                    Input::Mouse {
                        rotate: [0.0123, -0.5],
                        zoom: 1.0,
                    },
                ),
            ],
            digests: vec![
                (0, vec![("map".into(), "1100".into())]),
                (
                    600,
                    vec![("map".into(), "1100".into()), ("gold".into(), "0".into())],
                ),
            ],
            end: 700,
        }
    }

    #[test]
    fn a_record_survives_its_text() {
        let record = record();
        assert_eq!(Record::parse(&record.to_text()).unwrap(), record);
    }

    #[test]
    fn inputs_come_before_the_digest_of_their_step() {
        let text = record().to_text();
        let order: Vec<&str> = text.lines().skip(1).map(|l| &l[..1]).collect();
        assert_eq!(order, ["=", "+", "-", "!", "j", "m", "=", "."]);
    }

    #[test]
    fn a_file_cut_short_ends_at_its_last_line() {
        let text = record().to_text();
        let cut: String = text.lines().take(5).map(|l| format!("{l}\n")).collect();
        assert_eq!(Record::parse(&cut).unwrap().end, 9);
    }

    #[test]
    fn other_versions_and_bad_lines_are_refused() {
        let text = record().to_text().replace("\"version\":1", "\"version\":2");
        assert!(Record::parse(&text).unwrap_err().contains("version"));
        for bad in ["+x ArrowUp", "+3 KeyM", "?3", "m3 0 0", "=3 map"] {
            let text = format!("{}\n{bad}\n", record().header.to_json());
            assert!(Record::parse(&text).is_err(), "{bad}");
        }
        let backwards = format!("{}\n+5 KeyW\n-4 KeyW\n", record().header.to_json());
        assert!(Record::parse(&backwards).unwrap_err().contains("backwards"));
    }

    #[test]
    fn the_replay_names_the_first_mismatching_field() {
        let mut replay = Replay::new(record());
        replay.check(0, &vec![("map".into(), "1100".into())]);
        assert_eq!(replay.failure(), None);
        let actual = vec![("map".into(), "1100".into()), ("gold".into(), "5".into())];
        replay.check(600, &actual);
        let why = replay.failure().unwrap();
        assert!(
            why.contains("step 600") && why.contains("`gold` recorded 0, replayed 5"),
            "{why}"
        );
    }

    #[test]
    fn only_the_logic_keys_have_a_name() {
        assert_eq!(key_name(KeyCode::KeyQ), Some("KeyQ"));
        assert_eq!(key_name(KeyCode::KeyM), None);
        assert_eq!(key_name(KeyCode::KeyR), None);
    }
}
