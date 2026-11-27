use core::mem::{align_of, size_of};

use mcrs_minecraft_entity_key::EntityUuid;

use crate::constants::{
    ORB_DEFAULT_COUNT, ORB_DEFAULT_HEALTH, ORB_LIFETIME, ORB_MERGE_GROUP_COUNT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbLifecycle {
    Keep,
    Despawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct ExperienceOrb {
    pub value: i32,
    pub count: i32,
    pub age: i16,
    pub health: i16,
    pub merge_group: u8,
    pub tick_count: u32,
    pub following_player: Option<EntityUuid>,
}

impl Default for ExperienceOrb {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl ExperienceOrb {
    #[must_use]
    pub const fn merge_group_for_id(network_id: i32) -> u8 {
        network_id.rem_euclid(ORB_MERGE_GROUP_COUNT) as u8
    }

    #[must_use]
    pub const fn new(value: i32, network_id: i32) -> Self {
        Self {
            value,
            count: ORB_DEFAULT_COUNT,
            age: 0,
            health: ORB_DEFAULT_HEALTH,
            merge_group: Self::merge_group_for_id(network_id),
            tick_count: 0,
            following_player: None,
        }
    }

    #[must_use]
    pub const fn is_alive(&self) -> bool {
        self.count > 0 && self.health > 0 && self.age < ORB_LIFETIME
    }

    #[must_use]
    pub const fn lifecycle(&self) -> OrbLifecycle {
        if self.is_alive() {
            OrbLifecycle::Keep
        } else {
            OrbLifecycle::Despawn
        }
    }

    pub fn take_one(&mut self) -> OrbLifecycle {
        self.count = self.count.saturating_sub(1);
        self.lifecycle()
    }
}

const _: () = {
    let valid = size_of::<OrbLifecycle>() == 1 && align_of::<OrbLifecycle>() == 1;
    assert!(valid, "OrbLifecycle layout");
    let valid = size_of::<ExperienceOrb>() == 36 && align_of::<ExperienceOrb>() == 4;
    assert!(valid, "ExperienceOrb layout");
};
