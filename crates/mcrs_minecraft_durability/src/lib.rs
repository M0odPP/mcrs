#![no_std]

use core::mem::align_of;
use core::mem::size_of;

mod armour_bypass;
mod hurt_wear_policy;
mod slot_break_event;
mod wear;
mod wear_kind;

pub use armour_bypass::ArmourBypass;
pub use hurt_wear_policy::HurtWearPolicy;
pub use slot_break_event::SlotBreakEvent;
pub use wear::armour_hit_wear;
pub use wear::attack_wear;
pub use wear::block_break_wear;
pub use wear::roll_unbreaking;
pub use wear::roll_unbreaking_salted;
pub use wear_kind::WearKind;

const _: () = assert!(size_of::<WearKind>() == 1, "WearKind size");
const _: () = assert!(align_of::<WearKind>() == 1, "WearKind align");
const _: () = assert!(size_of::<ArmourBypass>() == 1, "ArmourBypass size");
const _: () = assert!(align_of::<ArmourBypass>() == 1, "ArmourBypass align");
const _: () = assert!(size_of::<HurtWearPolicy>() == 1, "HurtWearPolicy size");
const _: () = assert!(align_of::<HurtWearPolicy>() == 1, "HurtWearPolicy align");
const _: () = assert!(size_of::<SlotBreakEvent>() == 1, "SlotBreakEvent size");
const _: () = assert!(align_of::<SlotBreakEvent>() == 1, "SlotBreakEvent align");
