use std::mem::{align_of, size_of};

use bevy_ecs::component::Component;
use mcrs_minecraft_item_use::{ItemUseState, UseHand};
use mcrs_minecraft_physics::ItemUse;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[component(storage = "SparseSet")]
pub struct UsingItem(pub ItemUseState);

const _: () = assert!(
    size_of::<UsingItem>() == size_of::<ItemUseState>(),
    "UsingItem must wrap ItemUseState without extra fields"
);
const _: () = assert!(
    align_of::<UsingItem>() == align_of::<ItemUseState>(),
    "UsingItem must match ItemUseState alignment"
);

impl UsingItem {
    #[must_use]
    pub const fn item_use(self) -> ItemUse {
        ItemUse {
            using: true,
            speed_multiplier: self.0.speed_multiplier,
            permits_sprint: self.0.permits_sprint(),
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LivingFlags(pub u8);

const _: () = assert!(size_of::<LivingFlags>() == 1, "LivingFlags packs one byte");
const _: () = assert!(
    align_of::<LivingFlags>() == 1,
    "LivingFlags uses 1-byte alignment"
);

impl LivingFlags {
    #[must_use]
    pub const fn with_active_hand(self, active_hand: Option<UseHand>) -> Self {
        Self(mcrs_minecraft_item_use::living_flags_bits(
            self.0,
            active_hand,
        ))
    }
}
