use crate::double_chest::ChestType;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DoubleChestSlot {
    pub half: ChestType,
    pub local_slot: u8,
}

#[must_use]
pub const fn double_chest_slot(slot_index: u8) -> Option<DoubleChestSlot> {
    let (half, local_slot) = match slot_index {
        0..=26 => (ChestType::Right, slot_index),
        27..=53 => (ChestType::Left, slot_index - 27),
        _ => return None,
    };
    Some(DoubleChestSlot { half, local_slot })
}
