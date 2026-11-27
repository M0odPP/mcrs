use alloc::vec::Vec;

use crate::active_effects::ActiveEffect;
use crate::effect_id::{EffectId, SYNC_INTERVAL_TICKS, U64_ALIGN, has_layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EffectDamageKind {
    Magic,
    Wither,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HealingPolarity {
    #[default]
    Living,
    Inverted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectUpdate {
    pub effect: EffectId,
    pub amplifier: u8,
    pub duration: i32,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct EffectTickInput {
    pub entity_tick_count: u64,
    pub health: f32,
    pub max_health: f32,
    pub absorption: f32,
    pub dying: bool,
    pub polarity: HealingPolarity,
}

impl EffectTickInput {
    #[must_use]
    pub const fn new(tick: u64, health: f32, max_health: f32, absorption: f32) -> Self {
        Self {
            entity_tick_count: tick,
            health,
            max_health,
            absorption,
            dying: false,
            polarity: HealingPolarity::Living,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum EffectTickEvent {
    Heal(EffectId, f32),
    Damage {
        effect: EffectId,
        kind: EffectDamageKind,
        amount: f32,
    },
    Exhaustion(f32),
    Saturation(u8),
    Promoted(EffectUpdate),
    Sync(EffectUpdate),
    Removed(EffectId, u8),
}

#[must_use]
pub const fn effect_acts_on_tick(effect: EffectId, amplifier: u8, tick: u64) -> bool {
    let shift = amplifier as u32;
    match effect {
        EffectId::REGENERATION => interval_matches(50, shift, tick),
        EffectId::POISON => interval_matches(25, shift, tick),
        EffectId::WITHER => interval_matches(40, shift, tick),
        EffectId::INSTANT_HEALTH | EffectId::INSTANT_DAMAGE | EffectId::SATURATION => tick >= 1,
        EffectId::HUNGER | EffectId::ABSORPTION => true,
        _ => false,
    }
}

const fn interval_matches(base: u64, shift: u32, tick: u64) -> bool {
    match base.checked_shr(shift) {
        Some(0) | None => true,
        Some(interval) => tick.is_multiple_of(interval),
    }
}

#[must_use]
pub const fn hunger_exhaustion_per_tick(amplifier: u8) -> f32 {
    0.005 * ((amplifier as u16 + 1) as f32)
}

#[must_use]
pub fn instant_effect_with_potency(
    effect: EffectId,
    polarity: HealingPolarity,
    amplifier: u8,
    potency: f64,
) -> Option<EffectTickEvent> {
    let (heal, base) = match (effect, polarity) {
        (EffectId::INSTANT_HEALTH, HealingPolarity::Living)
        | (EffectId::INSTANT_DAMAGE, HealingPolarity::Inverted) => (true, 4_u64),
        (EffectId::INSTANT_HEALTH, HealingPolarity::Inverted)
        | (EffectId::INSTANT_DAMAGE, HealingPolarity::Living) => (false, 6_u64),
        _ => return None,
    };
    let clamped = (base << u32::from(amplifier).min(30)).min(i32::MAX as u64);
    let amount = if !potency.is_finite() || potency <= 0.0 {
        0.0
    } else if potency == 1.0 {
        clamped as i32 as f32
    } else {
        ((potency * (clamped as f64) + 0.5) as i32).max(0) as f32
    };
    Some(if heal {
        EffectTickEvent::Heal(effect, amount)
    } else {
        damage(effect, EffectDamageKind::Magic, amount)
    })
}

const fn snapshot(entry: &ActiveEffect) -> EffectUpdate {
    EffectUpdate {
        effect: entry.effect,
        amplifier: entry.stack.amplifier,
        duration: entry.stack.duration,
        flags: entry.stack.packet_flags(),
    }
}

const fn damage(effect: EffectId, kind: EffectDamageKind, amount: f32) -> EffectTickEvent {
    EffectTickEvent::Damage {
        effect,
        kind,
        amount,
    }
}

pub(crate) fn tick_effects(
    effects: &mut Vec<ActiveEffect>,
    input: EffectTickInput,
    mut on_event: impl FnMut(EffectTickEvent),
) {
    use EffectDamageKind::{Magic, Wither};
    use EffectTickEvent::{Exhaustion, Heal, Promoted, Removed, Saturation, Sync};
    let mut index = 0_usize;
    while let Some(entry) = effects.get_mut(index) {
        let no_absorption = !input.absorption.is_finite() || input.absorption <= 0.0;
        let mut expired =
            entry.stack.is_expired() || (entry.effect == EffectId::ABSORPTION && no_absorption);
        if !expired {
            let tick = if entry.stack.is_infinite() {
                input.entity_tick_count
            } else {
                entry.stack.duration as u64
            };
            let amplifier = entry.stack.amplifier;
            if effect_acts_on_tick(entry.effect, amplifier, tick) {
                let can_heal = !input.dying && input.health > 0.0;
                let healable = can_heal && input.health < input.max_health;
                let poisonable = !input.dying && input.health > 1.0;
                let effect = entry.effect;
                let event = match effect {
                    EffectId::REGENERATION if healable => Some(Heal(effect, 1.0)),
                    EffectId::POISON if poisonable => Some(damage(effect, Magic, 1.0)),
                    EffectId::WITHER => Some(damage(effect, Wither, 1.0)),
                    EffectId::INSTANT_HEALTH | EffectId::INSTANT_DAMAGE => {
                        instant_effect_with_potency(effect, input.polarity, amplifier, 1.0)
                            .filter(|event| !matches!(event, Heal(..)) || can_heal)
                    }
                    EffectId::HUNGER => Some(Exhaustion(hunger_exhaustion_per_tick(amplifier))),
                    EffectId::SATURATION => Some(Saturation(amplifier)),
                    _ => None,
                };
                if let Some(event) = event {
                    on_event(event);
                }
            }
            entry.stack.decrement_durations();
            if entry.stack.promote_hidden() {
                on_event(Promoted(snapshot(entry)));
            }
            expired = entry.stack.is_expired();
        }
        if expired {
            let removed = effects.remove(index);
            on_event(Removed(removed.effect, removed.stack.amplifier));
            continue;
        }
        if !entry.stack.is_infinite() && entry.stack.duration % SYNC_INTERVAL_TICKS == 0 {
            on_event(Sync(snapshot(entry)));
        }
        index = index.saturating_add(1);
    }
}

const _: () = {
    let valid = has_layout::<EffectDamageKind>(1, 1)
        && has_layout::<HealingPolarity>(1, 1)
        && has_layout::<EffectUpdate>(8, 4)
        && has_layout::<EffectTickInput>(24, U64_ALIGN)
        && has_layout::<EffectTickEvent>(12, 4);
    assert!(valid, "effect tick public types layout mismatch");
};
