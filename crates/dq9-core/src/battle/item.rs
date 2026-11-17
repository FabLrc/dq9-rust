//! Whether an item use changed anything, and so whether it is spent.
//!
//! The original consumes a held item at the end of `ResolveAction` (`ov_024::021eb5d0`, EU
//! `0x021ed724..0x021ed7e4`): it maps the actor's battle command entry (`battle + 0x3760`,
//! through `ov_000::02160130`/`ov_000::02160f08`) and calls the "use a held item" helper
//! (`func_020ddb38`, EU `0x020ddb48`), which clears the slot only when the item record's
//! `+0x08` bit 19 (used up) is set (`func_0208386c`, EU `0x0208387c`). That call is gated on
//! the action's applied counter (`context + 0x14`) being at least 1, and each kind handler
//! raises that counter only when it changed something:
//!
//! - kind 1 damage (`ov_024::021da9e0`): a blow of at least 1;
//! - kind 2 heal (`ov_024::021db3a4`): HP actually restored, or a rider result when the
//!   restored amount is 0 (riders are not modelled);
//! - kind 7 cure (`ov_024::021dbc64`): a status actually cured;
//! - kind 18 revive (`ov_024::021dd278`): a fallen target.
//!
//! So a herb on a full-HP ally, a revive on a living ally and an antidote with nothing to cure
//! all leave the item in hand. The one outcome this module can read is [`TargetOutcome`]: an HP
//! or MP total that changed. Kinds that move only a status or restore MP (4, 5, 6, 19, 26) are
//! not visible there, but the resolver refuses them; among the 47 used-up EU records, 16 carry
//! kind 2 (healing) and none carries kind 1 or 57, the only other kinds the resolver applies, so
//! the predicate covers every reachable item (`docs/re/battle.md`).

use super::resolve::TargetOutcome;

/// Whether the resolved targets changed a HP or MP total, the original's applied counter for
/// the kinds the resolver models. See the module documentation for the handler addresses and
/// the nil cases (full HP, nothing to cure, a living revive target) that leave the item
/// unspent.
pub fn effect_applied<'a>(targets: impl IntoIterator<Item = &'a TargetOutcome>) -> bool {
    targets
        .into_iter()
        .any(|target| target.hp_after != target.hp_before || target.mp_damage > 0)
}
