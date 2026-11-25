use core::hash::Hash;
use core::hash::Hasher;

use crate::float_key;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct Weapon {
    pub item_damage_per_attack: i32,
    pub disable_blocking_for_seconds: f32,
}

impl Weapon {
    pub const DEFAULT_ITEM_DAMAGE_PER_ATTACK: i32 = 1;
    pub const DEFAULT_DISABLE_BLOCKING_FOR_SECONDS: f32 = 0.0;
    pub const DEFAULT: Self = Self {
        item_damage_per_attack: Self::DEFAULT_ITEM_DAMAGE_PER_ATTACK,
        disable_blocking_for_seconds: Self::DEFAULT_DISABLE_BLOCKING_FOR_SECONDS,
    };

    #[must_use]
    pub const fn new() -> Self {
        Self::DEFAULT
    }
}

impl Default for Weapon {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl PartialEq for Weapon {
    fn eq(&self, other: &Self) -> bool {
        self.item_damage_per_attack == other.item_damage_per_attack
            && float_key::of_f32(self.disable_blocking_for_seconds)
                == float_key::of_f32(other.disable_blocking_for_seconds)
    }
}

impl Eq for Weapon {}

impl Hash for Weapon {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.item_damage_per_attack.hash(state);
        float_key::of_f32(self.disable_blocking_for_seconds).hash(state);
    }
}
