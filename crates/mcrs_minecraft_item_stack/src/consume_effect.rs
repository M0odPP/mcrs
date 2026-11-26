use alloc::vec::Vec;
use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};

use crate::EffectInstance;
use crate::MobEffectSet;
use crate::SoundEventRef;
use crate::float_key;

#[derive(Clone, Debug)]
pub enum ConsumeEffect {
    ApplyEffects {
        effects: Vec<EffectInstance>,
        probability: f32,
    },
    RemoveEffects(MobEffectSet),
    ClearAllEffects,
    TeleportRandomly {
        diameter: f32,
        directional_particles: bool,
    },
    PlaySound(SoundEventRef),
}

impl ConsumeEffect {
    pub const DEFAULT_PROBABILITY: f32 = 1.0;
    pub const DEFAULT_TELEPORT_DIAMETER: f32 = 16.0;
    pub const DEFAULT_DIRECTIONAL_PARTICLES: bool = true;
}

impl PartialEq for ConsumeEffect {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::ApplyEffects {
                    effects: left_effects,
                    probability: left_probability,
                },
                Self::ApplyEffects {
                    effects: right_effects,
                    probability: right_probability,
                },
            ) => {
                left_effects == right_effects
                    && float_key::of_f32(*left_probability) == float_key::of_f32(*right_probability)
            }
            (Self::RemoveEffects(left), Self::RemoveEffects(right)) => left == right,
            (Self::ClearAllEffects, Self::ClearAllEffects) => true,
            (
                Self::TeleportRandomly {
                    diameter: left_diameter,
                    directional_particles: left_particles,
                },
                Self::TeleportRandomly {
                    diameter: right_diameter,
                    directional_particles: right_particles,
                },
            ) => {
                float_key::of_f32(*left_diameter) == float_key::of_f32(*right_diameter)
                    && left_particles == right_particles
            }
            (Self::PlaySound(left), Self::PlaySound(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for ConsumeEffect {}

impl Hash for ConsumeEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            Self::ApplyEffects {
                effects,
                probability,
            } => {
                effects.hash(state);
                float_key::of_f32(*probability).hash(state);
            }
            Self::RemoveEffects(effects) => effects.hash(state),
            Self::ClearAllEffects => {}
            Self::TeleportRandomly {
                diameter,
                directional_particles,
            } => {
                float_key::of_f32(*diameter).hash(state);
                directional_particles.hash(state);
            }
            Self::PlaySound(sound) => sound.hash(state),
        }
    }
}

const _: () = {
    assert!(
        size_of::<ConsumeEffect>() >= size_of::<SoundEventRef>(),
        "ConsumeEffect carries at least SoundEventRef"
    );
    assert!(
        align_of::<ConsumeEffect>() >= align_of::<MobEffectSet>(),
        "ConsumeEffect aligns at least to pointer alignment"
    );
};
