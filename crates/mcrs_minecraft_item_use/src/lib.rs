#![no_std]

extern crate alloc;

mod consume_effects;
mod cooldown;
mod living_flags;
mod remainder;
mod use_spec;
mod use_state;

pub use consume_effects::{
    AppliedConsumeEffect, ConsumableEffectInstance, DEFAULT_POTION_DURATION_SCALE, EffectTiming,
    INFINITE_EFFECT_DURATION_TICKS, OMINOUS_BOTTLE_DURATION_TICKS, OMINOUS_BOTTLE_EFFECT_ID,
    OMINOUS_BOTTLE_MAX_AMPLIFIER, SUSPICIOUS_STEW_DEFAULT_DURATION_TICKS, StewEffectEntry,
    collect_potion_effects, collect_stew_effects, ominous_bottle_effect, scale_potion_duration,
};
pub use cooldown::{
    CooldownEntry, CooldownMap, cooldown_ticks, remaining_ticks, resolve_cooldown_group,
};
pub use living_flags::{
    LIVING_FLAG_OFFHAND, LIVING_FLAG_SPIN_ATTACK, LIVING_FLAG_USING_ITEM, living_flags_bits,
};
pub use remainder::{ConsumeMode, RemainderOutcome, compute_consume_remainder};
pub use use_spec::DEFAULT_CONSUME_SECONDS;
pub use use_spec::DEFAULT_USE_SPEED_MULTIPLIER;
pub use use_spec::SHIELD_USE_DURATION_TICKS;
pub use use_spec::SprintPolicy;
pub use use_spec::UseHand;
pub use use_spec::UseKind;
pub use use_spec::UseSpec;
pub use use_spec::consume_ticks;
pub use use_spec::should_emit_consume_tick_effects;
pub use use_state::ItemUseState;
pub use use_state::UseOutcome;
pub use use_state::cancel_on_slot_change;
pub use use_state::release_use;
pub use use_state::start_use;
pub use use_state::tick_use;
