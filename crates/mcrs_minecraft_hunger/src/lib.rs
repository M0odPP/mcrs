#![no_std]

mod exhaustion;
mod hunger_difficulty;
mod hunger_state;
mod hunger_tick_input;
mod hunger_tick_output;

pub use exhaustion::ATTACK_EXHAUSTION;
pub use exhaustion::EXHAUSTION_DRAIN_THRESHOLD;
pub use exhaustion::HARVEST_BREAK_EXHAUSTION;
pub use exhaustion::JUMP_EXHAUSTION;
pub use exhaustion::MAX_EXHAUSTION;
pub use exhaustion::SPRINT_JUMP_EXHAUSTION;
pub use exhaustion::SprintState;
pub use exhaustion::hunger_effect_exhaustion;
pub use exhaustion::jump_exhaustion;
pub use exhaustion::sprint_move_exhaustion;
pub use hunger_difficulty::HungerDifficulty;
pub use hunger_state::HungerState;
pub use hunger_tick_input::HungerTickInput;
pub use hunger_tick_output::HungerTickOutput;
