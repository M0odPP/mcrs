use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};

use crate::float_key;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct AttackRange {
    pub min_reach: f32,
    pub max_reach: f32,
    pub min_creative_reach: f32,
    pub max_creative_reach: f32,
    pub hitbox_margin: f32,
    pub mob_factor: f32,
}

const _: () = assert!(
    size_of::<AttackRange>() == 24 && align_of::<AttackRange>() == 4,
    "AttackRange is 24 bytes with 4-byte alignment"
);

impl AttackRange {
    pub const DEFAULT: Self = Self {
        min_reach: 0.0,
        max_reach: 3.0,
        min_creative_reach: 0.0,
        max_creative_reach: 5.0,
        hitbox_margin: 0.3,
        mob_factor: 1.0,
    };

    #[must_use]
    pub fn is_valid_reach(value: f32) -> bool {
        value.total_cmp(&0.0).is_ge() && value.total_cmp(&64.0).is_le()
    }

    #[must_use]
    pub fn is_valid_mob_factor(value: f32) -> bool {
        value.total_cmp(&0.0).is_ge() && value.total_cmp(&2.0).is_le()
    }

    const fn key(self) -> [u32; 6] {
        [
            float_key::of_f32(self.min_reach),
            float_key::of_f32(self.max_reach),
            float_key::of_f32(self.min_creative_reach),
            float_key::of_f32(self.max_creative_reach),
            float_key::of_f32(self.hitbox_margin),
            float_key::of_f32(self.mob_factor),
        ]
    }
}

impl Default for AttackRange {
    fn default() -> Self {
        Self::DEFAULT
    }
}
impl PartialEq for AttackRange {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl Eq for AttackRange {}
impl Hash for AttackRange {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}
