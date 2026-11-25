#![no_std]

extern crate alloc;

mod equipment;
mod slot;

pub use equipment::{EquipmentChangeOrigin, EquipmentSlots, diff_equipment};
pub use slot::EquipSlot;
