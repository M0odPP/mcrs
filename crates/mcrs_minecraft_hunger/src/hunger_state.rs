use core::mem::{align_of, size_of};

use crate::EXHAUSTION_DRAIN_THRESHOLD;
use crate::HungerDifficulty;
use crate::HungerTickInput;
use crate::HungerTickOutput;
use crate::MAX_EXHAUSTION;

const DEFAULT_FOOD_LEVEL: i32 = 20;
const DEFAULT_SATURATION: f32 = 5.0;
const NORMAL_REGEN_MIN_FOOD: i32 = 18;
const SATURATED_REGEN_TICKS: i32 = 10;
const SLOW_TICKS: i32 = 80;
const SATURATED_EXHAUSTION_CAP: f32 = 6.0;
const NORMAL_REGEN_EXHAUSTION: f32 = 6.0;
const PEACEFUL_HEAL_INTERVAL: u64 = 20;
const PEACEFUL_FOOD_INTERVAL: u64 = 10;
const PEACEFUL_MAX_SATURATION: f32 = 20.0;
const EASY_STARVE_FLOOR: f32 = 10.0;
const NORMAL_STARVE_FLOOR: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct HungerState {
    pub food_level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub tick_timer: i32,
}

const _: () = assert!(
    size_of::<HungerState>() == 16,
    "HungerState stays four packed 32-bit fields"
);
const _: () = assert!(
    align_of::<HungerState>() == 4,
    "HungerState stays 4-byte aligned"
);

impl Default for HungerState {
    fn default() -> Self {
        Self {
            food_level: DEFAULT_FOOD_LEVEL,
            saturation: DEFAULT_SATURATION,
            exhaustion: 0.0,
            tick_timer: 0,
        }
    }
}

impl HungerState {
    #[must_use]
    pub const fn new(food_level: i32, saturation: f32, exhaustion: f32, tick_timer: i32) -> Self {
        Self {
            food_level,
            saturation,
            exhaustion,
            tick_timer,
        }
    }

    pub fn add_exhaustion(&mut self, amount: f32) {
        if !amount.is_finite() || amount <= 0.0 {
            return;
        }
        let base = if self.exhaustion.is_finite() {
            self.exhaustion
        } else {
            0.0
        };
        self.exhaustion = (base + amount).clamp(0.0, MAX_EXHAUSTION);
    }

    #[must_use]
    pub fn tick(&mut self, input: HungerTickInput) -> HungerTickOutput {
        let mut health = input.health;
        let max_health = input.max_health;

        if input.difficulty == HungerDifficulty::Peaceful && input.natural_regeneration {
            if input.lived_ticks.is_multiple_of(PEACEFUL_HEAL_INTERVAL) {
                if health > 0.0 && health < max_health {
                    health = (health + 1.0).min(max_health);
                }
                if self.saturation < PEACEFUL_MAX_SATURATION {
                    self.saturation += 1.0;
                }
            }
            if input.lived_ticks.is_multiple_of(PEACEFUL_FOOD_INTERVAL)
                && self.food_level < DEFAULT_FOOD_LEVEL
            {
                self.food_level = self.food_level.saturating_add(1);
            }
        }

        if self.exhaustion > EXHAUSTION_DRAIN_THRESHOLD {
            self.exhaustion -= EXHAUSTION_DRAIN_THRESHOLD;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else if input.difficulty != HungerDifficulty::Peaceful {
                self.food_level = self.food_level.saturating_sub(1).max(0);
            }
        }

        let mut starve = false;
        let needs_healing = health > 0.0 && health < max_health;
        if input.natural_regeneration
            && self.saturation > 0.0
            && self.food_level >= DEFAULT_FOOD_LEVEL
            && needs_healing
        {
            self.tick_timer = self.tick_timer.saturating_add(1);
            if self.tick_timer >= SATURATED_REGEN_TICKS {
                let step = self.saturation.min(SATURATED_EXHAUSTION_CAP);
                health = (health + step / SATURATED_EXHAUSTION_CAP).min(max_health);
                if !input.invulnerable {
                    self.add_exhaustion(step);
                }
                self.tick_timer = 0;
            }
        } else if input.natural_regeneration
            && self.food_level >= NORMAL_REGEN_MIN_FOOD
            && needs_healing
        {
            self.tick_timer = self.tick_timer.saturating_add(1);
            if self.tick_timer >= SLOW_TICKS {
                health = (health + 1.0).min(max_health);
                if !input.invulnerable {
                    self.add_exhaustion(NORMAL_REGEN_EXHAUSTION);
                }
                self.tick_timer = 0;
            }
        } else if self.food_level <= 0 {
            self.tick_timer = self.tick_timer.saturating_add(1);
            if self.tick_timer >= SLOW_TICKS {
                starve = can_starve(health, input.difficulty);
                self.tick_timer = 0;
            }
        } else {
            self.tick_timer = 0;
        }

        let next_health = (health != input.health).then_some(health);
        HungerTickOutput::new(next_health, starve)
    }
}

const fn can_starve(health: f32, difficulty: HungerDifficulty) -> bool {
    health > EASY_STARVE_FLOOR
        || matches!(difficulty, HungerDifficulty::Hard)
        || (health > NORMAL_STARVE_FLOOR && matches!(difficulty, HungerDifficulty::Normal))
}
