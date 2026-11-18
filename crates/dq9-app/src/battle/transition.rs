//! The swirl that opens a fight (`docs/re/battle-transition.md`): its clock, the camera it
//! drives and the fade it starts. Pure logic, no GPU and no ROM: the application feeds it one
//! fixed 60 Hz step at a time and reads the roll, the field of view and the cover back.
//!
//! port: 0x0204701c
//!
//! The application's transition (`App::step_battle`) feeds it one fixed 60 Hz step at a time
//! and reads the roll, the field of view and the cover back (`docs/re/battle-transition.md`,
//! "What the application does with it").
//!
//! The original runs the swirl from a task that is updated about every second frame (the game
//! loop's `numTicks`: 3 on the first update, 2 after) and `SwirlEffect_Update` (`0x0204701c`)
//! does, per update: roll −8°, half field of view −0.4333°, add the ticks to its counter, ask
//! for a 20-tick black fade of both screens once the counter passes 15, and finish when it passes
//! 35. The model's own animation advances half a frame per tick (motion 0, speed 0.5).

/// Ticks of the first update (`GetTickCount` capped at 3 after the long frame the contact's
/// loading leaves) and of every later one.
const FIRST_UPDATE_TICKS: u32 = 3;
const UPDATE_TICKS: u32 = 2;
/// Roll added per update, degrees (`[2] -= 8.0`; wraps past ±180 as the original's accumulator).
const ROLL_STEP: f32 = 8.0;
/// Half field of view at the start and what each update takes off, degrees
/// (`0x02046f88` = 15.0, `0x02047164` = 0.4333).
const FOV_START: f32 = 15.0;
const FOV_STEP: f32 = 0.4333;
/// The counter value past which the fade starts, and past which the swirl is done.
const FADE_AFTER: u32 = 15;
const DONE_AFTER: u32 = 35;
/// Ticks of the fade of both screens to black (`SetMainBrightness(-16, 20)`).
pub const FADE_TICKS: u32 = 20;
/// The model's first animation frame and its speed per tick (`ev999991500.bcfg`, motion 0).
const ANIM_START: f32 = 1.0;
const ANIM_SPEED: f32 = 0.5;

/// The state of the swirl after some steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swirl {
    /// Steps taken (60 Hz).
    ticks: u32,
    /// The original's counter (`record[0]`): the sum of the ticks of the updates so far.
    counter: u32,
    roll: f32,
    fov: f32,
    /// The animation clock in model frames.
    frame: f32,
    /// Step at which the fade began, once asked for.
    fade_from: Option<u32>,
}

impl Default for Swirl {
    fn default() -> Self {
        Self::new()
    }
}

impl Swirl {
    pub const fn new() -> Self {
        Self {
            ticks: 0,
            counter: 0,
            roll: 0.0,
            fov: FOV_START,
            frame: ANIM_START,
            fade_from: None,
        }
    }

    /// One fixed step. The task updates on every second step starting with the first, as the
    /// original's frames do; returns whether the swirl has just ended.
    pub fn step(&mut self) -> bool {
        let update = self.ticks.is_multiple_of(UPDATE_TICKS) && !self.done();
        self.ticks += 1;
        if !update {
            return false;
        }
        let delta = if self.counter == 0 {
            FIRST_UPDATE_TICKS
        } else {
            UPDATE_TICKS
        };
        self.roll -= ROLL_STEP;
        if self.roll <= -180.0 {
            self.roll = 180.0;
        }
        self.fov -= FOV_STEP;
        self.frame += delta as f32 * ANIM_SPEED;
        self.counter += delta;
        // `SetMainBrightness` is asked for in this update and shows from the one after: the
        // capture's first darker frame is the 18th step of the swirl.
        if self.counter > FADE_AFTER && self.fade_from.is_none() {
            self.fade_from = Some(self.ticks + 2);
        }
        self.done()
    }

    /// Whether the swirl has ended: the counter is past 35.
    pub const fn done(&self) -> bool {
        self.counter > DONE_AFTER
    }

    /// The field camera's roll in degrees, as `Camera+0x7c` holds it (negative: the view turns
    /// clockwise).
    pub const fn roll_degrees(&self) -> f32 {
        self.roll
    }

    /// The field camera's half field of view in degrees.
    pub const fn half_fov_degrees(&self) -> f32 {
        self.fov
    }

    /// The model's animation frame (the swirl's `.nsbca`/`.nsbma` clock, 1-based).
    pub const fn model_frame(&self) -> f32 {
        self.frame
    }

    /// Alpha of the black cover, 0 to 1: the fade of both screens from its start over
    /// [`FADE_TICKS`], linear as the master brightness is, applied by the game's 30 Hz updates
    /// (two frames of the capture always share a level).
    pub fn cover(&self) -> f32 {
        self.fade_from.map_or(0.0, |from| {
            ((self.ticks & !1).saturating_sub(from) as f32 / FADE_TICKS as f32).min(1.0)
        })
    }
}

/// Steps from the first step of the swirl to the end of its fade: when the application builds
/// the battle scene and the black ends. Measured 38 on the original (`docs/re/battle-transition.md`).
pub fn entry_ticks() -> u32 {
    let mut swirl = Swirl::new();
    let mut steps = 0;
    while swirl.cover() < 1.0 || !swirl.done() {
        swirl.step();
        steps += 1;
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(steps: u32) -> Swirl {
        let mut swirl = Swirl::new();
        for _ in 0..steps {
            swirl.step();
        }
        swirl
    }

    /// The original's updates, read from the DS core's RAM every frame (`w-contact1`): roll
    /// 352°, 344°, … and half field of view 14.567, 14.133, … at the 1st, 2nd, … update, the
    /// counter 3, 5, 7, …; the 18th (counter 37) ends it.
    #[test]
    fn updates_match_the_measured_camera() {
        let mut swirl = Swirl::new();
        for update in 1..=18u32 {
            let mut done = false;
            for _ in 0..2 {
                done |= swirl.step();
            }
            let roll = -8.0 * update as f32;
            assert_eq!(swirl.roll_degrees(), roll, "update {update}");
            let fov = 15.0 - 0.4333 * update as f32;
            assert!(
                (swirl.half_fov_degrees() - fov).abs() < 1e-4,
                "update {update}"
            );
            assert_eq!(done, update == 18, "update {update}");
        }
        assert!(swirl.done());
        assert_eq!(swirl.counter, 37);
        // No update after the end: the camera is given back as it was.
        swirl.step();
        swirl.step();
        assert_eq!(swirl.roll_degrees(), -144.0);
    }

    /// The camera changes only on every second step.
    #[test]
    fn the_camera_moves_at_thirty_hertz() {
        let mut swirl = Swirl::new();
        swirl.step();
        let first = swirl.roll_degrees();
        swirl.step();
        assert_eq!(swirl.roll_degrees(), first);
        swirl.step();
        assert_ne!(swirl.roll_degrees(), first);
    }

    /// Both screens stay lit until the counter passes 15 (the 8th update), then go black over
    /// 20 ticks: the capture's frames 35 to 55, the 18th to the 38th from the first update.
    #[test]
    fn the_fade_starts_after_the_eighth_update() {
        assert_eq!(run(16).cover(), 0.0);
        assert_eq!(run(17).cover(), 0.0);
        assert!(run(18).cover() > 0.0);
        assert_eq!(run(37).cover(), 0.95);
        assert_eq!(run(38).cover(), 1.0);
    }

    /// The entry takes 38 steps: the capture is black from its 38th frame on (the old
    /// constants said 35).
    #[test]
    fn the_entry_lasts_thirty_eight_steps() {
        assert_eq!(entry_ticks(), 38);
    }

    /// The model's animation runs half a frame per tick from frame 1 (counter 3 on the first
    /// update): the last update draws frame 1 + 37/2.
    #[test]
    fn the_model_animation_runs_at_half_speed() {
        assert_eq!(run(36).model_frame(), 19.5);
    }

    /// The roll wraps the way the accumulator does when it passes −180 (never reached in
    /// the 18 updates of a swirl, but the original code does it).
    #[test]
    fn the_roll_accumulator_wraps() {
        let mut swirl = Swirl::new();
        swirl.roll = -176.0;
        swirl.counter = 1;
        swirl.step();
        assert_eq!(swirl.roll_degrees(), 180.0);
    }
}
