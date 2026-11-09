//! A monster's HP at the start of a battle (`InitMonsterBattleStatus`, `func_02089630`, EU
//! `0x02089640`).
//!
//! Only the HP draw is modelled here, not the rest of the status the function builds, so the
//! function is not listed as ported.

use super::random::Random;

/// The HP a monster comes to a battle with: `(int)(0.5f + (float)HP × NextRandomFloatBetween(0.8f,
/// 1.0f))`, one draw **from the world's generator** `world` (never the battle's); the result is
/// both its HP and its maximum. The table's HP is a ceiling: a slime's 8 comes to 6, 7 or 8.
///
/// A set battle (the setup holds a number other than −1 at `+0x0C`, wiki `Battle-Resolution`,
/// INFERRED to be a scripted battle) skips the draw and keeps the table's HP.
pub fn init_monster_hp(world: &mut Random, table_hp: u16, set_battle: bool) -> i32 {
    if set_battle {
        return i32::from(table_hp);
    }
    (0.5f32 + f32::from(table_hp) * world.next_float_between(0.8, 1.0)) as i32
}
