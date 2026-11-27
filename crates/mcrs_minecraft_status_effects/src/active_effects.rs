use alloc::boxed::Box;
use alloc::vec::{Drain, Vec};

use crate::effect_id::{
    EffectId, EffectImmunities, INFINITE_DURATION, PTR_SIZE, has_layout, is_shorter_duration,
};
use crate::effect_tick::{EffectTickEvent, EffectTickInput, tick_effects};

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ActiveEffect {
    pub effect: EffectId,
    pub stack: EffectStack,
}

impl ActiveEffect {
    #[must_use]
    pub const fn new(effect: EffectId, duration: i32, amplifier: u8) -> Self {
        Self {
            effect,
            stack: EffectStack::new(duration, amplifier),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct EffectStack {
    pub amplifier: u8,
    pub duration: i32,
    pub ambient: bool,
    pub show_particles: bool,
    pub show_icon: bool,
    pub hidden_effect: Option<Box<EffectStack>>,
}

impl EffectStack {
    #[must_use]
    pub const fn new(duration: i32, amplifier: u8) -> Self {
        Self {
            amplifier,
            duration,
            ambient: false,
            show_particles: true,
            show_icon: true,
            hidden_effect: None,
        }
    }

    #[must_use]
    pub const fn is_infinite(&self) -> bool {
        self.duration == INFINITE_DURATION
    }

    #[must_use]
    pub const fn is_expired(&self) -> bool {
        !self.is_infinite() && self.duration <= 0
    }

    #[must_use]
    pub const fn packet_flags(&self) -> u8 {
        (self.ambient as u8) | ((self.show_particles as u8) << 1) | ((self.show_icon as u8) << 2)
    }

    pub(crate) fn merge(&mut self, incoming: &Self) -> bool {
        let mut outer_changed = None;
        let mut cursor = Some(self);
        while let Some(existing) = cursor {
            let mut changed = false;
            let mut next_hidden = None;
            if incoming.amplifier > existing.amplifier {
                if is_shorter_duration(incoming.duration, existing.duration) {
                    let previous_hidden = existing.hidden_effect.take();
                    let mut previous = existing.clone();
                    previous.hidden_effect = previous_hidden;
                    existing.hidden_effect = Some(Box::new(previous));
                }
                existing.amplifier = incoming.amplifier;
                existing.duration = incoming.duration;
                changed = true;
            } else if is_shorter_duration(existing.duration, incoming.duration) {
                if incoming.amplifier == existing.amplifier {
                    existing.duration = incoming.duration;
                    changed = true;
                } else if existing.hidden_effect.is_none() {
                    existing.hidden_effect = Some(Box::new(incoming.clone()));
                } else {
                    next_hidden = existing.hidden_effect.as_deref_mut();
                }
            }
            if (!incoming.ambient && existing.ambient) || changed {
                existing.ambient = incoming.ambient;
                changed = true;
            }
            if incoming.show_particles != existing.show_particles
                || incoming.show_icon != existing.show_icon
            {
                existing.show_particles = incoming.show_particles;
                existing.show_icon = incoming.show_icon;
                changed = true;
            }
            outer_changed = outer_changed.or(Some(changed));
            cursor = next_hidden;
        }
        matches!(outer_changed, Some(true))
    }

    pub(crate) fn decrement_durations(&mut self) {
        let mut cursor = Some(self);
        while let Some(layer) = cursor {
            if layer.duration > 0 {
                layer.duration -= 1;
            }
            cursor = layer.hidden_effect.as_deref_mut();
        }
    }

    pub(crate) fn promote_hidden(&mut self) -> bool {
        if self.duration != 0 {
            return false;
        }
        let Some(promoted) = self.hidden_effect.take() else {
            return false;
        };
        *self = *promoted;
        true
    }
}

impl Drop for EffectStack {
    fn drop(&mut self) {
        let mut next = self.hidden_effect.take();
        while let Some(mut node) = next {
            next = node.hidden_effect.take();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddEffectStatus {
    Refused,
    Added,
    Updated,
    Unchanged,
}

impl AddEffectStatus {
    #[must_use]
    pub const fn packet_flags(self, stack: &EffectStack) -> Option<u8> {
        match self {
            Self::Added => Some(stack.packet_flags() | 0x08),
            Self::Updated => Some(stack.packet_flags()),
            Self::Refused | Self::Unchanged => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct ActiveEffects {
    effects: Vec<ActiveEffect>,
}

impl ActiveEffects {
    #[must_use]
    pub fn get(&self, effect: EffectId) -> Option<&ActiveEffect> {
        self.effects.iter().find(|entry| entry.effect == effect)
    }

    pub fn add(&mut self, incoming: ActiveEffect) -> AddEffectStatus {
        self.add_with_immunity(incoming, EffectImmunities::NONE)
    }

    pub fn add_with_immunity(
        &mut self,
        incoming: ActiveEffect,
        immunities: EffectImmunities,
    ) -> AddEffectStatus {
        if immunities.is_immune(incoming.effect) {
            return AddEffectStatus::Refused;
        }
        for existing in &mut self.effects {
            if existing.effect == incoming.effect {
                return if existing.stack.merge(&incoming.stack) {
                    AddEffectStatus::Updated
                } else {
                    AddEffectStatus::Unchanged
                };
            }
        }
        self.effects.push(incoming);
        AddEffectStatus::Added
    }

    pub fn insert_loaded(&mut self, loaded: ActiveEffect) {
        let _ = self.remove(loaded.effect);
        self.effects.push(loaded);
    }

    pub fn remove(&mut self, effect: EffectId) -> Option<ActiveEffect> {
        self.effects
            .iter()
            .position(|entry| entry.effect == effect)
            .map(|index| self.effects.remove(index))
    }

    pub fn drain(&mut self) -> Drain<'_, ActiveEffect> {
        self.effects.drain(..)
    }

    pub fn tick(&mut self, input: EffectTickInput, on_event: impl FnMut(EffectTickEvent)) {
        tick_effects(&mut self.effects, input, on_event);
    }
}

const _: () = {
    let valid = has_layout::<EffectStack>(8 + PTR_SIZE, PTR_SIZE)
        && has_layout::<ActiveEffect>(8 + 2 * PTR_SIZE, PTR_SIZE)
        && has_layout::<AddEffectStatus>(1, 1)
        && has_layout::<ActiveEffects>(3 * PTR_SIZE, PTR_SIZE);
    assert!(valid, "active effects public types layout mismatch");
};
