#![no_std]

mod living_flags;
mod use_spec;
mod use_state;

pub use living_flags::LIVING_FLAG_OFFHAND;
pub use living_flags::LIVING_FLAG_SPIN_ATTACK;
pub use living_flags::LIVING_FLAG_USING_ITEM;
pub use living_flags::living_flags_bits;
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
