use core::hash::Hash;
use core::hash::Hasher;
use core::mem::align_of;
use core::mem::size_of;

use crate::float_key;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct UseEffects {
    pub can_sprint: bool,
    pub interact_vibrations: bool,
    pub speed_multiplier: f32,
}

const _: () = {
    assert!(size_of::<UseEffects>() == 8, "UseEffects is 8 bytes");
    assert!(
        align_of::<UseEffects>() == 4,
        "UseEffects aligns to 4 bytes"
    );
};

impl UseEffects {
    pub const DEFAULT_CAN_SPRINT: bool = false;
    pub const DEFAULT_INTERACT_VIBRATIONS: bool = true;
    pub const DEFAULT_SPEED_MULTIPLIER: f32 = 0.2;
    pub const DEFAULT: Self = Self {
        can_sprint: Self::DEFAULT_CAN_SPRINT,
        interact_vibrations: Self::DEFAULT_INTERACT_VIBRATIONS,
        speed_multiplier: Self::DEFAULT_SPEED_MULTIPLIER,
    };

    #[must_use]
    pub const fn new() -> Self {
        Self::DEFAULT
    }
}

impl Default for UseEffects {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl PartialEq for UseEffects {
    fn eq(&self, other: &Self) -> bool {
        self.can_sprint == other.can_sprint
            && self.interact_vibrations == other.interact_vibrations
            && float_key::of_f32(self.speed_multiplier) == float_key::of_f32(other.speed_multiplier)
    }
}

impl Eq for UseEffects {}

impl Hash for UseEffects {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.can_sprint.hash(state);
        self.interact_vibrations.hash(state);
        float_key::of_f32(self.speed_multiplier).hash(state);
    }
}
