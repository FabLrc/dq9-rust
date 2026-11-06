//! What a running `.stb` script asks the application to do: one [`Event`] per native call
//! whose effect is visible outside the interpreter. Arguments are decoded as the handlers
//! read them (`docs/re/stb-natives.md`, `ov_001::0215ae1c…`); units are the game's.

/// A display engine of the DS, as the fade natives address them (`docs/re/stb-natives.md`,
/// § Temps et unités). Which one is the top screen is **[unknown]**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Both,
    Main,
    Sub,
}

/// What a fade does to the brightness lock of its engines (`+0x24`, `+0x25` of the
/// brightness controller: a locked engine ignores plain fades).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lock {
    Keep,
    Lock,
    Unlock,
}

/// The text of a message native (400): an entry of the event's text table, or a literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    /// Message number, as the script's text table (`evNNNNN_<lg>.bin`) numbers them.
    Id(i32),
    Literal(String),
}

/// One command of an actor's queues (natives 203 to 232). The game runs the commands of an
/// actor on eight channels; an actor stays busy until all of them are done (native 204).
#[derive(Debug, Clone, PartialEq)]
pub enum ActorCommand {
    /// 203: forgets the queues and the binding.
    Reset,
    /// 205: the actor drives binding slot `slot` (see [`Event::BindSlot`]).
    Bind {
        slot: i32,
    },
    /// 206.
    SetPosition([f32; 3]),
    /// 207: linear move; `duration` in 30 Hz updates (`docs/re/stb-natives.md`).
    MoveTo {
        position: [f32; 3],
        duration: i32,
        arrival_animation: Option<String>,
    },
    /// 232: like 207 on the ground, the height following the collision.
    MoveOnGround {
        x: f32,
        z: f32,
        duration: i32,
        arrival_animation: Option<String>,
    },
    /// 208: radians, the middle component turns around Y.
    SetRotation([f32; 3]),
    /// 209: negative `duration` adds the angles each update.
    RotateTo {
        rotation: [f32; 3],
        duration: i32,
        direction: i32,
    },
    /// 221: turns towards another actor.
    TurnTowards {
        target: i32,
        duration: i32,
        direction: Option<i32>,
    },
    /// 210: starts at once.
    Animate {
        name: String,
        flags: i32,
        speed: f32,
    },
    /// 224: starts when the current animation ends.
    QueueAnimation {
        name: String,
        flags: i32,
        speed: f32,
    },
    /// 222.
    StopAnimation,
    /// 213: the animation channel waits for the current animation to end.
    WaitAnimation,
    /// 218: the animation channel waits `count` updates.
    DelayAnimation(i32),
    /// 211: `duration` ≤ 0 assigns.
    SetScale {
        scale: [f32; 3],
        duration: i32,
    },
    /// 219.
    SetAlpha(i32),
    /// 220.
    FadeAlpha {
        alpha: i32,
        duration: i32,
    },
    /// 226.
    SetRadius(f32),
    /// 227.
    FadeRadius {
        target: f32,
        duration: i32,
    },
    /// 214 to 217: a spline through up to 16 points.
    PathReset,
    PathSpeed(i32),
    PathPoint([f32; 3]),
    PathRun,
}

/// The camera natives (300 to 327, 531, 532, 572, 580). Durations are the scripts' own
/// counts; their unit is **[unknown]** for most (`docs/re/stb-natives.md`).
#[derive(Debug, Clone, PartialEq)]
pub enum Camera {
    /// 300: empties the queues.
    Reset,
    /// 302, 303.
    SetPosition([f32; 3]),
    SetTarget([f32; 3]),
    /// 304: both at constant speed; `snap` forces the destination.
    MoveBoth {
        position: [f32; 3],
        target: [f32; 3],
        duration: i32,
        snap: bool,
    },
    /// 305, 306.
    MoveTarget {
        target: [f32; 3],
        duration: i32,
    },
    MovePosition {
        position: [f32; 3],
        duration: i32,
    },
    /// 310: eye = target + Ry(azimuth) · (0, height, √(distance² − height²)).
    SetOrbit {
        azimuth: f32,
        height: f32,
        distance: f32,
    },
    /// 311: `direction` −1 takes the short way round, 0 decreases, else increases.
    MoveOrbit {
        azimuth: f32,
        height: f32,
        distance: f32,
        duration: i32,
        direction: i32,
        snap: bool,
    },
    /// 321.
    MoveTargetKeepOrbit {
        target: [f32; 3],
        duration: i32,
    },
    /// 322.
    MoveTargetAndOrbit {
        target: [f32; 3],
        azimuth: f32,
        height: f32,
        distance: f32,
        duration: i32,
        direction: i32,
    },
    /// 317 (eye and target together), 326 (target only); negative `duration` never ends.
    Shake {
        amplitude: [f32; 3],
        duration: i32,
        target_only: bool,
    },
    /// 324, 325: the target follows an actor plus an offset.
    FollowActor {
        actor: i32,
        offset: [f32; 3],
    },
    StopFollow,
    /// 327: degrees.
    Roll(f32),
    /// 531: reinstalls the camera saved by 572.
    RestoreSaved,
    /// 532: degrees.
    SetFov(f32),
    /// 580.
    MoveFov {
        degrees: f32,
        duration: i32,
    },
    /// 572: a camera driven by two bones of the character bound to `slot`.
    BoneCamera {
        slot: i32,
        eye: String,
        target: String,
    },
}

/// The sound natives (713 to 738).
#[derive(Debug, Clone, PartialEq)]
pub enum Sound {
    /// 713: selects a sequence and stops it; 714 restarts it.
    SelectBgm(i32),
    RestartBgm,
    /// 715: 0 to 127.
    BgmVolume {
        volume: i32,
        duration: i32,
    },
    /// 721.
    StopBgm {
        duration: i32,
    },
    /// 737: a sequence on the handle of the jingle.
    PlaySequence(i32),
    /// 738.
    ResetBgm,
    /// 726, 730: loads a sound-effect group (`secondary` for 730): its bank and waves, and
    /// the sequence archive `archive` (the group's own number unless 726 gives another) the
    /// next 728/732 play from.
    LoadEffects {
        group: i32,
        archive: i32,
        secondary: bool,
    },
    /// 712 with one argument: plays effect `index` of the base archive (the field's
    /// `se_norm.sdat` archive 100, a battle's `se_btl.sdat` 101) without a handle.
    PlayBaseEffect {
        index: i32,
    },
    /// 720: asks for the jingle `sequence`: the music pauses, the jingle plays out, the
    /// music resumes. The application ends the wait of native 725 with `Signal::JingleEnded`.
    Jingle(i32),
    /// 736: the music of the current place, as on a map change.
    ZoneBgm,
    /// 735: `false` locks the music (selecting, restarting and stopping do nothing).
    BgmEnabled(bool),
    /// 554: the voice of the message window's text blip (0, 1 or 2).
    TextVoice(i32),
    /// 727, 731: restores the heap from before the matching load.
    RestoreEffects {
        secondary: bool,
    },
    /// 728, 732: plays effect `index` of the loaded group; `handle` (0 to 15) is what the
    /// script keeps to stop it, -1 when it passes no address to receive it.
    PlayEffect {
        index: i32,
        handle: i32,
        secondary: bool,
    },
    /// 729, 723: stops the effect of `handle`.
    StopEffect {
        handle: i32,
        duration: i32,
    },
}

/// Something a script asks of the application.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The script starts running entry `entry` (see [`super::Entries`]).
    Phase {
        entry: i32,
    },
    /// Natives 100 to 122: fade the brightness of `screen` to `level` (−16 black, 0 normal,
    /// +16 white) in `ticks` ticks of 1/60 s.
    Fade {
        screen: Screen,
        level: i32,
        ticks: i32,
        lock: Lock,
    },
    /// 109, 110: the display swap bit of `POWCNT1`.
    SwapScreens(bool),
    /// Native 400. The script then polls 405 until the application reports the message
    /// closed (`Signal::MessageClosed`).
    Message {
        text: Text,
        flags: i32,
    },
    /// 401.
    CloseMessage,
    /// 410: no frame around the text.
    HideMessageFrame,
    /// 200: loads a model (`.chr`) as character `character` (negative ids are the event's).
    LoadModel {
        path: String,
        character: i32,
        selector: i32,
    },
    /// 212.
    ReleaseModel {
        character: i32,
    },
    /// 228.
    CloneModel {
        source: i32,
        destination: i32,
    },
    /// 229: animations/resources of an archive added to an existing character.
    LoadModelArchive {
        path: String,
        character: i32,
        tag: i32,
    },
    /// 230.
    RemoveAnimationPackage {
        character: i32,
        tag: i32,
    },
    /// 233: a monster model of `enemy.gp2`.
    LoadMonster {
        path: String,
        character: i32,
    },
    /// 201: shows or hides a character by id.
    CharacterVisible {
        character: i32,
        visible: bool,
    },
    /// 202: binding slot `slot` designates `character`; `kind` is the slot type (0, 1, 4, 5
    /// characters; 2, 6 map objects).
    BindSlot {
        slot: i32,
        character: i32,
        kind: i32,
    },
    /// 570: shows or hides the object of a binding slot.
    SlotVisible {
        slot: i32,
        visible: bool,
    },
    /// 235, 236: attaches the object of `child` to a bone of the object of `parent`.
    AttachBone {
        child: i32,
        parent: i32,
        bone: Option<String>,
    },
    DetachBone {
        slot: i32,
    },
    /// Natives 203 to 232 on actor `actor` (0 to 23).
    Actor {
        actor: i32,
        command: ActorCommand,
    },
    Camera(Camera),
    Sound(Sound),
    /// 566: a cast member declared for the scene build. `character` is the id the game
    /// reserves for it (modes 4, 6, 7, 8: −1, −2, …; mode 5 copies `reference`).
    DeclareCast {
        mode: i32,
        path: Option<String>,
        reference: Option<i32>,
        attach: i32,
        option: Option<i32>,
        character: Option<i32>,
    },
    /// 567: a character archive loaded for the cast members whose `attach` equals `n`.
    DeclareCharacterArchive {
        path: String,
        attach: i32,
    },
    /// 568: scene flags accumulated (bits 0x1, 0x2, 0x8 suspend subsystems, 0x4 hides the
    /// objects 0 to 3 outside the cast, 0x20 …; `docs/re/stb-natives.md`).
    AddSceneFlags(i32),
    /// 506, 845: files to load ahead; 507 reports when they are in.
    Preload(Vec<String>),
    /// 508.
    ReleasePreloaded,
    /// 521: a 2D sprite (`data/ani/<name>`), whose handle the script keeps.
    CreateSprite {
        name: String,
        handle: i32,
    },
    /// 522.
    DestroySprite {
        handle: i32,
    },
    /// 548 (`Some(mode)`), 549 (`None`): lighting colour table index override.
    LightingOverride(Option<i32>),
    /// 578: brightness factor of the lighting.
    LightIntensity {
        intensity: f32,
        duration: i32,
    },
    /// 579: stops (`true`) or runs the day clock.
    DayTimerStopped(bool),
    /// 588: forces the scene's time-of-day profile (0 to 3) without touching the save.
    SceneTimeOfDay(i32),
    /// 589: the scene's profile follows the saved time of day.
    SyncSceneTimeOfDay,
    /// 808: writes the saved time of day (`Save_SetTimeOfDay`).
    SetTimeOfDay(i32),
    /// 804: 3D fog.
    Fog(bool),
    /// 8, 9: the marker that chains the next map load to the event.
    Chaining(bool),
    /// A native whose effect is not known or not modelled: reported once per id.
    Unimplemented {
        id: i32,
    },
    /// The event `event` starts: its script is loaded.
    Started {
        event: u16,
    },
    /// The event's script could not be loaded; it is skipped.
    LoadFailed {
        event: u16,
        reason: String,
    },
    /// The event ended (or its script faulted): the game fires trigger event 11.
    Ended {
        event: u16,
    },
    /// The script stopped on an interpreter error.
    Fault(super::vm::Fault),
}

impl Event {
    /// The completion an application that executes every request at once reports for this
    /// event (the fake application of `dq9re script` and the tests).
    pub fn instant_signal(&self) -> Option<super::Signal> {
        use super::Signal;
        Some(match self {
            Self::Message { .. } => Signal::MessageClosed,
            Self::Actor { actor, .. } => Signal::ActorIdle(*actor),
            Self::Camera(_) => Signal::CameraIdle,
            Self::Preload(_) => Signal::Preloaded,
            Self::Sound(Sound::Jingle(_)) => Signal::JingleEnded,
            _ => return None,
        })
    }
}
