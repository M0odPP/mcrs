use mcrs_minecraft_durability::SlotBreakEvent;
use mcrs_minecraft_durability::WearKind;
use mcrs_minecraft_inventory_player::ArmourSlot;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DurabilitySlot {
    MainHand,
    OffHand,
    Armour(ArmourSlot),
}

impl DurabilitySlot {
    pub(crate) const fn wear_kind(self) -> WearKind {
        match self {
            Self::MainHand | Self::OffHand => WearKind::Tool,
            Self::Armour(_) => WearKind::Armour,
        }
    }

    pub(crate) const fn break_event(self) -> SlotBreakEvent {
        match self {
            Self::MainHand => SlotBreakEvent::MainHand,
            Self::OffHand => SlotBreakEvent::OffHand,
            Self::Armour(ArmourSlot::Feet) => SlotBreakEvent::Feet,
            Self::Armour(ArmourSlot::Legs) => SlotBreakEvent::Legs,
            Self::Armour(ArmourSlot::Chest) => SlotBreakEvent::Chest,
            Self::Armour(ArmourSlot::Head) => SlotBreakEvent::Head,
        }
    }

    pub(crate) fn salt(self, event_index: u32) -> u64 {
        let slot_tag = u64::from(self.break_event().status().cast_unsigned());
        (slot_tag << 32) | (u64::from(event_index) << 16)
    }
}
