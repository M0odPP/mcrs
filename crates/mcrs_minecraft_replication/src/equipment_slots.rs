use mcrs_minecraft_equipment::{EquipSlot, EquipmentSlots};
use mcrs_minecraft_protocol_game::entity::EquipmentSlot;
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_registries::Wire;

#[must_use]
pub const fn protocol_slot(slot: EquipSlot) -> EquipmentSlot {
    match slot {
        EquipSlot::MainHand => EquipmentSlot::MainHand,
        EquipSlot::OffHand => EquipmentSlot::OffHand,
        EquipSlot::Feet => EquipmentSlot::Feet,
        EquipSlot::Legs => EquipmentSlot::Legs,
        EquipSlot::Chest => EquipmentSlot::Chest,
        EquipSlot::Head => EquipmentSlot::Head,
    }
}

#[must_use]
pub const fn equip_slot(slot: EquipmentSlot) -> Option<EquipSlot> {
    match slot {
        EquipmentSlot::MainHand => Some(EquipSlot::MainHand),
        EquipmentSlot::OffHand => Some(EquipSlot::OffHand),
        EquipmentSlot::Feet => Some(EquipSlot::Feet),
        EquipmentSlot::Legs => Some(EquipSlot::Legs),
        EquipmentSlot::Chest => Some(EquipSlot::Chest),
        EquipmentSlot::Head => Some(EquipSlot::Head),
        EquipmentSlot::Body | EquipmentSlot::Saddle => None,
    }
}

#[must_use]
pub(crate) fn normalize_stack(stack: &ProtoStack<Wire>) -> Option<ProtoStack<Wire>> {
    (!stack.is_empty()).then(|| stack.clone())
}

pub(crate) fn apply_wire_slots(
    target: &mut EquipmentSlots<ProtoStack<Wire>>,
    slots: &[(EquipmentSlot, ProtoStack<Wire>)],
) {
    for (slot, stack) in slots {
        if let Some(equip_slot) = equip_slot(*slot) {
            target.set(equip_slot, normalize_stack(stack));
        }
    }
}

#[must_use]
pub(crate) fn from_wire_slots(
    slots: &[(EquipmentSlot, ProtoStack<Wire>)],
) -> EquipmentSlots<ProtoStack<Wire>> {
    let mut equipment = EquipmentSlots::new();
    apply_wire_slots(&mut equipment, slots);
    equipment
}
