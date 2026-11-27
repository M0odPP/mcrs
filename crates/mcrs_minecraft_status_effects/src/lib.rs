#![no_std]

extern crate alloc;

mod active_effects;
#[cfg(feature = "attributes")]
mod attribute_modifiers;
mod effect_id;
mod effect_tick;

pub use active_effects::{ActiveEffect, ActiveEffects, AddEffectStatus, EffectStack};
#[cfg(feature = "attributes")]
pub use attribute_modifiers::{
    EffectAttributeModifier, apply_effect_attribute_modifier, effect_attribute_modifier,
    effect_immunity_tag, remove_effect_attribute_modifier,
};
pub use effect_id::{
    EffectId, EffectImmunities, INFINITE_DURATION, SYNC_INTERVAL_TICKS, clamp_amplifier,
    is_shorter_duration,
};
pub use effect_tick::{
    EffectDamageKind, EffectTickEvent, EffectTickInput, EffectUpdate, HealingPolarity,
    effect_acts_on_tick, hunger_exhaustion_per_tick, instant_effect_with_potency,
};
