//! Moving one item between the shared bag and a member's held slots, and putting an item on.
//!
//! The bag's order and stacks are [`Party::bag`](dq9_core::party::Party): `Bag::remove` takes
//! copies from the first stack that holds the item and frees a slot at 0, `Bag::add` fills the
//! first stack of the item else the first empty slot (`func_0207c378`). A held slot is one item
//! (no quantity), the eight words at `7578 + 18 i` of the party slot (`Character::held_items`).
//! Neither move may lose a copy: every transfer is a bag operation that either succeeded or
//! changed nothing, which is what a full bag (152 slots) and an occupied slot refuse.
//!
//! Equipment: an item's thousands pick the slot it can be worn in (the wiki's *Character
//! Parts* naming, `docs/re/item.md`); the bands are those of `itemdt_<category>` (weapons
//! 19050–20918, shields 21000–21397, helmets 12170–12917, armour 13000–13794, gloves
//! 15000–15299, leggings 16093–16390, shoes 17091–17421, accessories 18000–18055). Which item
//! is wearable by which vocation is not modelled.

use dq9_core::party::{BAG_STACK, Character, HELD_ITEMS, Party};
use dq9_formats::save::EquipSlot;

/// The slot an item is worn in, from its id band; `None` for a tool, a seed or a face/hair.
pub fn equip_slot_of(item: u16) -> Option<EquipSlot> {
    Some(match item / 1000 {
        12 => EquipSlot::Head,
        13 => EquipSlot::Torso,
        15 => EquipSlot::Arms,
        16 => EquipSlot::Legs,
        17 => EquipSlot::Feet,
        19 | 20 => EquipSlot::Weapon,
        21 => EquipSlot::Shield,
        18 => EquipSlot::Accessory,
        _ => return None,
    })
}

/// Whether [`Bag::add`](dq9_core::party::Bag::add) would really store `item`: its `true` is
/// also returned when an existing stack is already at 99 and the copy is clamped away
/// (`func_0207c378`), which a transfer must never treat as success. A new stack needs an
/// empty slot.
fn bag_can_store(bag: &dq9_core::party::Bag, item: u16) -> bool {
    match bag.slots().iter().find(|stack| stack.item == Some(item)) {
        Some(stack) => stack.quantity < BAG_STACK,
        None => bag.slots().iter().any(|stack| stack.item.is_none()),
    }
}

/// The party's leader, whose held slots the field menu fills.
fn leader(party: &mut Party) -> Option<&mut Character> {
    party.members.first_mut()
}

/// Puts one copy of `item` from the bag into held slot `slot` of the leader. A slot that
/// already holds an item gives that item back to the bag first, so a full bag refuses the
/// swap instead of losing either item.
///
/// Refuses a slot outside the eight, an item the bag does not hold, an occupied slot whose
/// item the bag has no room for, and a bag removal that yields nothing: the bag is changed
/// only then, and by exactly one copy.
pub fn bag_to_held(party: &mut Party, item: u16, slot: usize) -> bool {
    if slot >= HELD_ITEMS {
        return false;
    }
    if party.bag.remove(item, 1) != 1 {
        return false;
    }
    let old = party
        .members
        .first()
        .and_then(|member| member.held_items[slot]);
    if let Some(old) = old
        && !bag_can_store(&party.bag, old)
    {
        // No real room for the item the slot held: put the chosen copy back.
        let _ = party.bag.add(item);
        return false;
    }
    if let Some(old) = old {
        let _ = party.bag.add(old);
    }
    let Some(member) = leader(party) else {
        // A party without a member cannot happen above (the bag removal succeeded), but the
        // copy must not vanish if it does: put it back.
        let _ = party.bag.add(item);
        return false;
    };
    member.held_items[slot] = Some(item);
    true
}

/// Takes held slot `slot` of the leader back into the bag. A full bag refuses, and the slot
/// keeps its item. The slot is cleared as it stands: the other slots keep their order (the
/// battle's compaction, `func_0208386c`, belongs to a use).
pub fn held_to_bag(party: &mut Party, slot: usize) -> bool {
    if slot >= HELD_ITEMS {
        return false;
    }
    let Some(item) = party
        .members
        .first()
        .and_then(|member| member.held_items[slot])
    else {
        return false;
    };
    if !bag_can_store(&party.bag, item) {
        return false;
    }
    let _ = party.bag.add(item);
    if let Some(member) = leader(party) {
        member.held_items[slot] = None;
    }
    true
}

/// Puts `item` (or nothing) in `slot` of the leader's equipment.
///
/// The item leaves the bag and the one it replaces comes back, in that order; when the bag
/// cannot take the old item the new one goes back and nothing changes. `item` must be worn in
/// `slot` ([`equip_slot_of`]) and held in the bag.
pub fn equip(party: &mut Party, slot: EquipSlot, item: Option<u16>) -> bool {
    if let Some(item) = item
        && equip_slot_of(item) != Some(slot)
    {
        return false;
    }
    let Some(member) = party.members.first_mut() else {
        return false;
    };
    if member.equipment.get(slot) == item {
        return true;
    }
    let old = member.equipment.get(slot);
    if let Some(item) = item
        && party.bag.remove(item, 1) != 1
    {
        return false;
    }
    if let Some(old) = old
        && !bag_can_store(&party.bag, old)
    {
        if let Some(item) = item {
            let _ = party.bag.add(item);
        }
        return false;
    }
    if let Some(old) = old {
        let _ = party.bag.add(old);
    }
    if let Some(member) = party.members.first_mut() {
        member.equipment.set(slot, item);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_core::party::BAG_CAPACITY;

    fn new_party() -> Party {
        let mut party = Party::new_game();
        party.members[0].held_items = [None; HELD_ITEMS];
        party
    }

    const HERB: u16 = 22000;
    const HERB2: u16 = 22004;

    #[test]
    fn a_copy_moves_from_the_bag_into_the_chosen_slot() {
        let mut party = new_party();
        assert!(party.bag.add_copies(HERB, 2));
        assert!(party.bag.add_copies(HERB2, 1));
        assert!(bag_to_held(&mut party, HERB, 3));
        assert_eq!(party.bag.count(HERB), 1);
        assert_eq!(party.bag.count(HERB2), 1);
        assert_eq!(party.members[0].held_items[3], Some(HERB));
        // The other slots are untouched, duplicates keep their order.
        assert_eq!(party.members[0].held_items[0], None);
    }

    #[test]
    fn an_occupied_slot_swaps_its_item_and_a_missing_item_changes_nothing() {
        let mut party = new_party();
        assert!(party.bag.add_copies(HERB, 1));
        party.members[0].held_items[2] = Some(HERB2);
        assert!(bag_to_held(&mut party, HERB, 2));
        assert_eq!(party.bag.count(HERB), 0);
        assert_eq!(party.bag.count(HERB2), 1);
        assert_eq!(party.members[0].held_items[2], Some(HERB));
        assert!(!bag_to_held(&mut party, HERB, 8));
        assert!(!bag_to_held(&mut party, 20004, 0));
    }

    #[test]
    fn holding_back_keeps_the_slot_order_and_a_full_bag_refuses() {
        let mut party = new_party();
        party.members[0].held_items[1] = Some(HERB);
        party.members[0].held_items[4] = Some(HERB2);
        assert!(held_to_bag(&mut party, 1));
        assert_eq!(party.bag.count(HERB), 1);
        assert_eq!(party.members[0].held_items[1], None);
        assert_eq!(party.members[0].held_items[4], Some(HERB2));

        let mut full = new_party();
        full.members[0].held_items[0] = Some(HERB);
        full.bag = dq9_core::party::Bag::empty(BAG_CAPACITY);
        for item in 22001..22001 + BAG_CAPACITY as u16 {
            assert!(full.bag.add(item));
        }
        assert!(!held_to_bag(&mut full, 0));
        assert_eq!(full.members[0].held_items[0], Some(HERB));
    }

    #[test]
    fn equipping_moves_the_old_piece_back_to_the_bag() {
        let mut party = new_party();
        party.members[0].equipment = Default::default();
        assert!(party.bag.add(20004)); // copper sword
        assert!(party.bag.add(20005)); // soldier's sword
        assert!(equip(&mut party, EquipSlot::Weapon, Some(20004)));
        assert_eq!(party.members[0].equipment.weapon, Some(20004));
        assert_eq!(party.bag.count(20004), 0);
        assert!(equip(&mut party, EquipSlot::Weapon, Some(20005)));
        assert_eq!(party.members[0].equipment.weapon, Some(20005));
        assert_eq!(party.bag.count(20005), 0);
        assert_eq!(party.bag.count(20004), 1, "the replaced sword comes back");
        assert!(equip(&mut party, EquipSlot::Weapon, None));
        assert_eq!(party.members[0].equipment.weapon, None);
        assert_eq!(party.bag.count(20005), 1);
    }

    #[test]
    fn an_item_out_of_its_band_or_not_held_is_refused() {
        let mut party = new_party();
        party.members[0].equipment = Default::default();
        assert!(party.bag.add(22000));
        assert!(!equip(&mut party, EquipSlot::Weapon, Some(22000)));
        assert!(!equip(&mut party, EquipSlot::Weapon, Some(20004)));
        assert_eq!(party.members[0].equipment.weapon, None);
        assert_eq!(party.bag.count(22000), 1);
        assert_eq!(equip_slot_of(20004), Some(EquipSlot::Weapon));
        assert_eq!(equip_slot_of(13007), Some(EquipSlot::Torso));
        assert_eq!(equip_slot_of(22000), None);
    }
}

/// A 99 stack plus a copy elsewhere: the total copies must never drop.
#[cfg(test)]
mod stack_tests {
    use super::*;
    use dq9_core::party::{BAG_CAPACITY, Bag};

    const HERB: u16 = 22000;
    const SWORD: u16 = 20004;

    fn party() -> Party {
        let mut party = Party::new_game();
        party.members[0].held_items = [None; HELD_ITEMS];
        party
    }

    #[test]
    fn a_full_stack_refuses_holding_back_and_keeps_the_held_copy() {
        let mut party = party();
        assert!(party.bag.add_copies(HERB, 99));
        party.members[0].held_items[0] = Some(HERB);
        assert!(!held_to_bag(&mut party, 0));
        assert_eq!(party.bag.count(HERB), 99);
        assert_eq!(party.members[0].held_items[0], Some(HERB));
    }

    #[test]
    fn a_full_stack_refuses_the_swap_and_keeps_both_items() {
        let mut party = party();
        assert!(party.bag.add_copies(HERB, 99));
        assert!(party.bag.add(SWORD));
        party.members[0].held_items[0] = Some(HERB);
        assert!(!bag_to_held(&mut party, SWORD, 0));
        assert_eq!(party.bag.count(HERB), 99);
        assert_eq!(party.bag.count(SWORD), 1);
        assert_eq!(party.members[0].held_items[0], Some(HERB));
    }

    #[test]
    fn a_full_stack_refuses_equipping_and_keeps_the_worn_piece() {
        let mut party = party();
        party.members[0].equipment.weapon = Some(SWORD);
        assert!(party.bag.add_copies(SWORD, 99));
        assert!(!equip(
            &mut party,
            dq9_formats::save::EquipSlot::Weapon,
            None
        ));
        assert_eq!(party.bag.count(SWORD), 99);
        assert_eq!(
            party.members[0].equipment.weapon,
            Some(SWORD),
            "the worn sword stays worn"
        );
    }

    #[test]
    fn removing_the_only_copy_frees_the_slot_for_the_old_piece() {
        let mut party = party();
        party.bag = Bag::empty(BAG_CAPACITY);
        assert!(party.bag.add(SWORD));
        for item in 22001..22001 + (BAG_CAPACITY as u16 - 1) {
            assert!(party.bag.add(item), "item {item}");
        }
        party.members[0].held_items[0] = Some(HERB);
        assert!(bag_to_held(&mut party, SWORD, 0));
        assert_eq!(party.bag.count(SWORD), 0);
        assert_eq!(party.bag.count(HERB), 1);
        assert_eq!(party.members[0].held_items[0], Some(SWORD));
        assert_eq!(
            party
                .bag
                .slots()
                .iter()
                .filter(|s| s.item.is_none())
                .count(),
            0
        );
    }
}
