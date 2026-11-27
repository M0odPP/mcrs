use alloc::vec::Vec;
use core::mem::{align_of, size_of};

use crate::ConsumeEffect;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct DeathProtection {
    pub death_effects: Vec<ConsumeEffect>,
}

const _: () = assert!(
    size_of::<DeathProtection>() == size_of::<Vec<ConsumeEffect>>()
        && align_of::<DeathProtection>() == align_of::<Vec<ConsumeEffect>>(),
    "DeathProtection matches Vec<ConsumeEffect> layout"
);

impl DeathProtection {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            death_effects: Vec::new(),
        }
    }
}
