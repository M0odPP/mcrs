use alloc::vec::Vec;
use core::mem::{align_of, size_of};

use crate::EquipSlot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct EquipmentSlots<S>([Option<S>; EquipSlot::COUNT]);

const _: () = assert!(
    size_of::<EquipmentSlots<u8>>() == 12,
    "EquipmentSlots<u8> must keep its compact 12-byte layout"
);
const _: () = assert!(
    align_of::<EquipmentSlots<u8>>() == 1,
    "EquipmentSlots<u8> must have one-byte alignment"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipmentChangeOrigin {
    SlotUpdate,
    HandSwap,
}

impl<S> Default for EquipmentSlots<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> EquipmentSlots<S> {
    #[must_use]
    pub const fn new() -> Self {
        Self([None, None, None, None, None, None])
    }

    fn slot_ref(&self, slot: EquipSlot) -> &Option<S> {
        let [main, off, feet, legs, chest, head] = &self.0;
        match slot {
            EquipSlot::MainHand => main,
            EquipSlot::OffHand => off,
            EquipSlot::Feet => feet,
            EquipSlot::Legs => legs,
            EquipSlot::Chest => chest,
            EquipSlot::Head => head,
        }
    }

    fn slot_mut(&mut self, slot: EquipSlot) -> &mut Option<S> {
        let [main, off, feet, legs, chest, head] = &mut self.0;
        match slot {
            EquipSlot::MainHand => main,
            EquipSlot::OffHand => off,
            EquipSlot::Feet => feet,
            EquipSlot::Legs => legs,
            EquipSlot::Chest => chest,
            EquipSlot::Head => head,
        }
    }

    #[must_use]
    pub fn get(&self, slot: EquipSlot) -> Option<&S> {
        self.slot_ref(slot).as_ref()
    }

    pub fn set(&mut self, slot: EquipSlot, value: Option<S>) -> Option<S> {
        core::mem::replace(self.slot_mut(slot), value)
    }

    pub fn swap_hands(&mut self) {
        self.0
            .swap(EquipSlot::MainHand.index(), EquipSlot::OffHand.index());
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    pub fn non_empty(&self) -> impl Iterator<Item = (EquipSlot, &S)> {
        EquipSlot::ALL
            .into_iter()
            .filter_map(move |slot| self.slot_ref(slot).as_ref().map(|s| (slot, s)))
    }
}

pub fn diff_equipment<S: Clone + PartialEq>(
    synced: &mut EquipmentSlots<S>,
    live: &EquipmentSlots<S>,
    origin: EquipmentChangeOrigin,
    out: &mut Vec<(EquipSlot, Option<S>)>,
) {
    out.clear();
    if origin == EquipmentChangeOrigin::HandSwap {
        synced.swap_hands();
    }
    for slot in EquipSlot::ALL {
        let next = live.slot_ref(slot);
        let prev = synced.slot_mut(slot);
        if prev != next {
            *prev = next.clone();
            out.push((slot, prev.clone()));
        }
    }
}
