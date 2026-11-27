#![no_std]

extern crate alloc;

mod blocking_rules;
mod death_protection_rules;
mod weapon_rules;

#[cfg(feature = "item-stack")]
pub use blocking_rules::evaluate_blocks_attacks;
pub use blocking_rules::{
    BlockingOutcome, BlockingUseState, block_delay_ticks, horizontal_attack_angle_degrees,
    shield_disable_ticks, shield_item_damage,
};
#[cfg(feature = "item-stack")]
pub use death_protection_rules::{
    DeathProtectionEffectsOutcome, evaluate_death_protection_component,
};
pub use death_protection_rules::{SURVIVAL_HEALTH, TOTEM_ENTITY_EVENT};
pub use weapon_rules::{
    ReachBounds, ReachMode, ReachVerdict, TargetBounds, attack_strength_scale, check_reach_bounds,
    has_minimum_attack_charge, scaled_attack_damage,
};
#[cfg(feature = "item-stack")]
pub use weapon_rules::{evaluate_attack_range, weapon_attack_item_damage};
