#![no_std]

mod constants;
mod lightning_rules;
mod strength_target;
mod weather_advance;
mod weather_cycle;
mod weather_cycle_rng;
mod weather_events;
mod weather_game_event;
mod weather_kind;
mod weather_level_step;
mod weather_levels;
mod weather_state;

#[cfg(test)]
mod tests;

pub use constants::{
    CLEAR_TICKS_MAX, CLEAR_TICKS_MIN, LIGHTNING_ROD_RADIUS, RAIN_OFF_MAX, RAIN_OFF_MIN,
    RAIN_ON_MAX, RAIN_ON_MIN, RAIN_TEMPERATURE_MIN, STRENGTH_STEP, THUNDER_OFF_MAX,
    THUNDER_OFF_MIN, THUNDER_ON_MAX, THUNDER_ON_MIN, THUNDER_THRESHOLD, WET_THRESHOLD,
};
pub use lightning_rules::{warm_enough_for_rain, within_rod_radius_sq};
pub use strength_target::StrengthTarget;
pub use weather_advance::WeatherAdvance;
pub use weather_cycle::{apply_weather_command, default_duration, step_cycle};
pub use weather_cycle_rng::WeatherCycleRng;
pub use weather_events::WeatherEvents;
pub use weather_game_event::WeatherGameEvent;
pub use weather_kind::WeatherKind;
pub use weather_level_step::WeatherLevelStep;
pub use weather_levels::{
    initial_levels, is_thunderstorm, is_wet, join_weather_events, step_level, step_weather_levels,
};
pub use weather_state::WeatherState;
