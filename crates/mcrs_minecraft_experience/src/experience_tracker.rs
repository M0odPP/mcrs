use core::mem::{align_of, size_of};

use crate::constants::ORB_PICKUP_COOLDOWN;
use crate::experience_state::ExperienceUpdate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
#[non_exhaustive]
pub struct ExperienceTracker {
    pub take_delay: u8,
    pub needs_sync: bool,
}

impl Default for ExperienceTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ExperienceTracker {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            take_delay: 0,
            needs_sync: true,
        }
    }

    pub fn tick_take_delay(&mut self) {
        self.take_delay = self.take_delay.saturating_sub(1);
    }

    #[must_use]
    pub const fn can_take_orb(&self) -> bool {
        self.take_delay == 0
    }

    pub fn record_orb_taken(&mut self) {
        self.take_delay = ORB_PICKUP_COOLDOWN;
    }

    pub fn apply_update(&mut self, update: ExperienceUpdate) {
        if update.needs_sync {
            self.needs_sync = true;
        }
    }
}

const _: () = {
    let valid = size_of::<ExperienceTracker>() == 2 && align_of::<ExperienceTracker>() == 1;
    assert!(valid, "ExperienceTracker layout");
};
