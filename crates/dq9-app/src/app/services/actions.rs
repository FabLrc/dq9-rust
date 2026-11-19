//! What a command does: using an item or a spell, buying, selling, and the rows each screen lists.

use super::Pending;
use super::Services;
use super::ShopMode;
use super::Source;
use super::transfer::equip_slot_of;
use crate::app::status;
use crate::battle::data::NO_ACTION;
use dq9_core::battle::Random;
use dq9_core::battle::Reach;
use dq9_core::battle::item::effect_applied;
use dq9_core::battle::resolve_action;
use dq9_core::facility::shop;
use dq9_core::party::HELD_ITEMS;
use dq9_core::party::Party;
use dq9_formats::save::EquipSlot;

/// The party members an action of `reach` heals from the field, in party order. A heal never
/// raises the dead; `Actor` is the leader and `One`/`Attack` the chosen row.
pub(super) fn heal_targets(reach: Reach, chosen: usize, party: &Party) -> Vec<usize> {
    let living = |index: usize| party.members.get(index).is_some_and(|member| member.hp > 0);
    match reach {
        Reach::All | Reach::Group => (0..party.members.len())
            .filter(|&index| living(index))
            .collect(),
        Reach::Actor => (0..party.members.len())
            .find(|&index| living(index))
            .into_iter()
            .collect(),
        Reach::One | Reach::Attack => (living(chosen)).then_some(chosen).into_iter().collect(),
    }
}

/// Moves the target cursor to the next living member, leaving it in place when none is.
pub(super) fn step_living(party: &Party, cursor: usize, delta: isize) -> usize {
    let len = party.members.len();
    if len == 0 {
        return 0;
    }
    let mut at = cursor.min(len - 1);
    for _ in 0..len {
        at = (at as isize + delta).rem_euclid(len as isize) as usize;
        if party.members[at].hp > 0 {
            return at;
        }
    }
    cursor.min(len - 1)
}

/// Takes the used held item out of its slot, shifting the holes down as `func_0208386c` does.
pub(super) fn consume_held(party: &mut Party, slot: usize) {
    let Some(character) = party.members.first_mut() else {
        return;
    };
    if slot >= HELD_ITEMS {
        return;
    }
    let items = &mut character.held_items;
    items[slot] = None;
    for index in slot..HELD_ITEMS - 1 {
        if items[index].is_none() {
            items[index] = items[index + 1];
            items[index + 1] = None;
        }
    }
}

impl Services {
    /// The item at the cursor when the screen is a list of the bag and the held slots.
    pub(super) fn item_source(&self, party: &Party) -> Option<Source> {
        self.item_sources(party).get(self.cursor).copied()
    }

    /// The rows of the item screen in order: the bag's non-empty slots, then the leader's
    /// eight held slots.
    pub(super) fn item_sources(&self, party: &Party) -> Vec<Source> {
        let mut out = Vec::new();
        for stack in party.bag.slots() {
            if let Some(item) = stack.item {
                out.push(Source::Bag(item));
            }
        }
        if let Some(member) = party.members.first() {
            for (slot, item) in member.held_items.iter().enumerate() {
                if item.is_some() {
                    out.push(Source::Held(slot));
                }
            }
        }
        out
    }

    pub(super) fn bag_rows(&self, party: &Party) -> usize {
        party
            .bag
            .slots()
            .iter()
            .filter(|s| s.item.is_some())
            .count()
    }

    /// The action of the item's field record (`+0x14`), when the action tables know it.
    pub(super) fn item_action(&self, party: &Party, source: Source) -> Option<u16> {
        let data = self.data.as_ref()?;
        let item = match source {
            Source::Bag(item) => item,
            Source::Held(slot) => party
                .members
                .first()
                .and_then(|member| member.held_items.get(slot).copied().flatten())?,
        };
        let [field, _] = *data.item_actions.get(&item)?;
        if field == NO_ACTION {
            return None;
        }
        let tables = data.tables().ok()?;
        let skill = tables.actions.skill(field)?;
        (skill.kind == 2).then_some(field)
    }

    /// The reach of an action the field can resolve, or `None` when its tables cannot name it.
    pub(super) fn action_reach(&self, action: u16) -> Option<Reach> {
        let tables = self.data.as_ref()?.tables().ok()?;
        let skill = tables.actions.skill(action)?;
        skill.action.ok().map(|action| action.reach)
    }

    pub(super) fn apply(
        &mut self,
        pending: Pending,
        target: usize,
        party: &mut Party,
        rng: &mut Random,
    ) {
        let Some(data) = &self.data else {
            return;
        };
        let actor = party
            .members
            .first()
            .map(|member| member.fighter(&data.items, &data.params));
        let Some(actor) = actor else {
            return;
        };
        if actor.hp == 0 {
            return;
        }
        match pending {
            Pending::Spell { action, cost } => {
                let Ok(tables) = data.tables() else {
                    return;
                };
                let Some(skill) = tables.actions.skill(action) else {
                    return;
                };
                let Ok(action_data) = skill.action else {
                    return;
                };
                if actor.mp < i32::from(cost) {
                    return;
                }
                let indices = heal_targets(action_data.reach, target, party);
                if indices.is_empty() {
                    return;
                }
                let fighters: Vec<_> = indices
                    .iter()
                    .map(|&index| party.members[index].fighter(&data.items, &data.params))
                    .collect();
                let Ok(outcome) = resolve_action(rng, &actor, &action_data, &fighters, 0) else {
                    return;
                };
                let before: Vec<u16> = indices
                    .iter()
                    .map(|&index| party.members[index].hp)
                    .collect();
                for (&index, hit) in indices.iter().zip(&outcome.targets) {
                    party.members[index].set_hp(hit.hp_after.max(0) as u16);
                }
                if let Some(caster) = party.members.first_mut() {
                    caster.set_mp(caster.mp.saturating_sub(u16::from(cost)));
                }
                status(&format!(
                    "field spell {action} on members {:?}: HP {:?} -> {:?}, MP {}",
                    indices,
                    before,
                    indices
                        .iter()
                        .map(|&index| party.members[index].hp)
                        .collect::<Vec<_>>(),
                    party.members.first().map_or(0, |member| member.mp)
                ));
            }
            Pending::Item(source) => {
                let Some(action) = self.item_action(party, source) else {
                    return;
                };
                let item = match source {
                    Source::Bag(item) => item,
                    Source::Held(slot) => match party
                        .members
                        .first()
                        .and_then(|member| member.held_items[slot])
                    {
                        Some(item) => item,
                        None => return,
                    },
                };
                let Ok(tables) = data.tables() else {
                    return;
                };
                let Some(skill) = tables.actions.skill(action) else {
                    return;
                };
                let Ok(action_data) = skill.action else {
                    return;
                };
                let indices = heal_targets(action_data.reach, target, party);
                if indices.is_empty() {
                    return;
                }
                let fighters: Vec<_> = indices
                    .iter()
                    .map(|&index| party.members[index].fighter(&data.items, &data.params))
                    .collect();
                let Ok(outcome) = resolve_action(rng, &actor, &action_data, &fighters, 0) else {
                    return;
                };
                let before: Vec<u16> = indices
                    .iter()
                    .map(|&index| party.members[index].hp)
                    .collect();
                for (&index, hit) in indices.iter().zip(&outcome.targets) {
                    party.members[index].set_hp(hit.hp_after.max(0) as u16);
                }
                // The field menu spends the copy only when the action changed something:
                // `func_ov002_021536ec`'s handlers report a change and only that result reaches
                // `func_ov002_0215a7b4`, which takes the bag copy or clears the held slot. A
                // herb on a member at full HP is kept, as in battle (`func_020ddb48`).
                let applied = effect_applied(&outcome.targets);
                match source {
                    Source::Bag(item) => {
                        if applied {
                            party.bag.remove(item, 1);
                        }
                        status(&format!(
                            "field item {item} on members {:?}: HP {:?} -> {:?}, bag {} left, {}",
                            indices,
                            before,
                            indices
                                .iter()
                                .map(|&index| party.members[index].hp)
                                .collect::<Vec<_>>(),
                            party.bag.count(item),
                            if applied { "spent" } else { "kept" }
                        ));
                    }
                    Source::Held(slot) => {
                        let used_up = data.item_meta.get(&item).is_some_and(|meta| meta.used_up);
                        if applied && used_up {
                            consume_held(party, slot);
                        }
                        status(&format!(
                            "field held item {item} on members {:?}: HP {:?} -> {:?}, slot {slot} {}",
                            indices,
                            before,
                            indices
                                .iter()
                                .map(|&index| party.members[index].hp)
                                .collect::<Vec<_>>(),
                            if applied && used_up { "spent" } else { "kept" }
                        ));
                    }
                }
            }
        }
    }

    pub(super) fn trade(&mut self, mode: ShopMode, item: u16, party: &mut Party) {
        let (Some(shops), Some(prices)) = (&self.shops, &self.prices) else {
            return;
        };
        let Some(shop) = shops.shop(self.shop_number) else {
            return;
        };
        match mode {
            ShopMode::Buy => match shop::buy(party, shop, prices, item, 1) {
                Ok(total) => {
                    status(&format!(
                        "shop {item}: bought for {total}, gold {}, bag {}",
                        party.gold,
                        party.bag.count(item)
                    ));
                    self.show_text(&format!("Bought for {total} gold."));
                }
                Err(error) => {
                    status(&format!(
                        "shop {item}: buy refused ({error:?}), gold {}, bag {}",
                        party.gold,
                        party.bag.count(item)
                    ));
                    self.show_text(&format!("{error:?}"));
                }
            },
            ShopMode::Sell => match shop::sell(party, prices, item, 1) {
                Ok(total) => {
                    status(&format!(
                        "shop {item}: sold for {total}, gold {}",
                        party.gold
                    ));
                    self.show_text(&format!("Sold for {total} gold."));
                }
                Err(error) => {
                    status(&format!(
                        "shop {item}: sell refused ({error:?}), gold {}, bag {}",
                        party.gold,
                        party.bag.count(item)
                    ));
                    self.show_text(&format!("{error:?}"));
                }
            },
        }
    }

    /// The shop's rows for the mode in use: `(label, item)`. A buy row is one stock item, a
    /// sell row one item the bag holds (its first slot, in bag order).
    pub(super) fn shop_rows(&self, party: &Party) -> Vec<(Vec<u8>, Option<u16>)> {
        let Some(shops) = &self.shops else {
            return Vec::new();
        };
        let Some(shop) = shops.shop(self.shop_number) else {
            return Vec::new();
        };
        let prices = self.prices.as_ref();
        let mut out = Vec::new();
        match self.shop_mode {
            ShopMode::Buy => {
                for item in shop.items() {
                    let price = prices
                        .and_then(|prices| shop::buy_price(shop, prices, item))
                        .unwrap_or(0);
                    let mut label = self.item_name(item);
                    label.extend_from_slice(format!("  {price}").as_bytes());
                    out.push((label, Some(item)));
                }
            }
            ShopMode::Sell => {
                let mut seen = Vec::new();
                for stack in party.bag.slots() {
                    let Some(item) = stack.item else { continue };
                    if seen.contains(&item) {
                        continue;
                    }
                    seen.push(item);
                    let price = prices
                        .and_then(|prices| shop::sell_price(prices, item))
                        .unwrap_or(0);
                    let mut label = self.item_name(item);
                    label.extend_from_slice(format!("  x{}  {price}", stack.quantity).as_bytes());
                    out.push((label, Some(item)));
                }
            }
        }
        out
    }

    /// The equipment choices for `slot`: the bag's items of that band, then `None` (take the
    /// piece off).
    pub(super) fn equip_choices(&self, party: &Party, slot: EquipSlot) -> Vec<Option<u16>> {
        let mut out = Vec::new();
        for stack in party.bag.slots() {
            let Some(item) = stack.item else { continue };
            if equip_slot_of(item) == Some(slot) && !out.contains(&Some(item)) {
                out.push(Some(item));
            }
        }
        out.push(None);
        out
    }

    /// The leader's learned healing spells: actions up to the member's level in its vocation
    /// whose kind is 2 (heal) and whose targets are its own side. `(action, MP cost, usable)`.
    pub(super) fn spells(&self, party: &Party) -> Vec<(u16, u8, bool)> {
        let (Some(data), Some(member)) = (&self.data, party.members.first()) else {
            return Vec::new();
        };
        let tables = match data.tables() {
            Ok(tables) => tables,
            Err(_) => return Vec::new(),
        };
        let actor = member.fighter(&data.items, &data.params);
        let target = actor.clone();
        data.spells_known(member.vocation, member.level())
            .into_iter()
            .filter_map(|action| {
                let skill = tables.actions.skill(action)?;
                if skill.kind != 2 || skill.aims != 2 {
                    return None;
                }
                let usable = member.hp > 0
                    && skill.action.as_ref().is_ok_and(|action| {
                        i32::from(action.mp_cost) <= actor.mp
                            && dq9_core::battle::validate(
                                &actor,
                                action,
                                std::slice::from_ref(&target),
                            )
                            .is_ok()
                    });
                Some((
                    action,
                    data.lines.get(&action).map_or(0, |lines| lines.mp_cost),
                    usable,
                ))
            })
            .collect()
    }
}
