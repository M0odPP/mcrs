use alloc::vec::Vec;
use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};

use crate::ConsumeEffect;
use crate::SoundEventRef;
use crate::UseAnimation;
use crate::float_key;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct Consumable {
    pub consume_seconds: f32,
    pub animation: UseAnimation,
    pub sound: Option<SoundEventRef>,
    pub has_consume_particles: bool,
    pub on_consume_effects: Vec<ConsumeEffect>,
}

impl Consumable {
    pub const DEFAULT_CONSUME_SECONDS: f32 = 1.6;
    pub const DEFAULT_ANIMATION: UseAnimation = UseAnimation::Eat;
    pub const DEFAULT_HAS_CONSUME_PARTICLES: bool = true;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            consume_seconds: Self::DEFAULT_CONSUME_SECONDS,
            animation: Self::DEFAULT_ANIMATION,
            sound: None,
            has_consume_particles: Self::DEFAULT_HAS_CONSUME_PARTICLES,
            on_consume_effects: Vec::new(),
        }
    }
}

impl Default for Consumable {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Consumable {
    fn eq(&self, other: &Self) -> bool {
        float_key::of_f32(self.consume_seconds) == float_key::of_f32(other.consume_seconds)
            && self.animation == other.animation
            && self.sound == other.sound
            && self.has_consume_particles == other.has_consume_particles
            && self.on_consume_effects == other.on_consume_effects
    }
}

impl Eq for Consumable {}

impl Hash for Consumable {
    fn hash<H: Hasher>(&self, state: &mut H) {
        float_key::of_f32(self.consume_seconds).hash(state);
        self.animation.hash(state);
        self.sound.hash(state);
        self.has_consume_particles.hash(state);
        self.on_consume_effects.hash(state);
    }
}

const _: () = {
    assert!(
        size_of::<Consumable>()
            >= size_of::<Option<SoundEventRef>>() + size_of::<Vec<ConsumeEffect>>(),
        "Consumable stores sound, effects list, and scalars"
    );
    assert!(
        align_of::<Consumable>() >= align_of::<Vec<ConsumeEffect>>(),
        "Consumable aligns at least to vector alignment"
    );
};
