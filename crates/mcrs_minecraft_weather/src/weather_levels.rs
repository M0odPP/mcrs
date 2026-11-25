use crate::constants::{THUNDER_THRESHOLD, WET_THRESHOLD};
use crate::strength_target::StrengthTarget::{self, Decay, Rise};
use crate::weather_events::WeatherEvents;
use crate::weather_game_event::WeatherGameEvent::{
    BeginRaining, EndRaining, RainLevelChange, ThunderLevelChange,
};
use crate::weather_level_step::WeatherLevelStep;
use crate::weather_state::WeatherState;

#[must_use]
pub fn step_level(current: f32, target: StrengthTarget) -> f32 {
    (current + target.delta()).clamp(0.0, 1.0)
}

#[must_use]
pub fn is_wet(rain: f32) -> bool {
    rain > WET_THRESHOLD
}

#[must_use]
pub fn is_thunderstorm(rain: f32, thunder: f32) -> bool {
    is_wet(rain) && thunder > THUNDER_THRESHOLD
}

#[must_use]
pub const fn initial_levels(state: &WeatherState) -> (f32, f32) {
    let r = if state.raining { 1.0 } else { 0.0 };
    let t = if state.thundering { 1.0 } else { 0.0 };
    (r, t)
}

#[must_use]
pub fn step_weather_levels(
    prev_rain: f32,
    prev_thunder: f32,
    state: &WeatherState,
) -> WeatherLevelStep {
    let rain = step_level(prev_rain, if state.raining { Rise } else { Decay });
    let thunder = step_level(prev_thunder, if state.thundering { Rise } else { Decay });
    let mut events = WeatherEvents::default();
    if rain != prev_rain {
        events.push(RainLevelChange(rain));
    }
    if thunder != prev_thunder {
        events.push(ThunderLevelChange(thunder));
    }
    let wet = is_wet(rain);
    if is_wet(prev_rain) != wet {
        let ev = if wet { BeginRaining } else { EndRaining };
        events.push(ev);
        events.push(RainLevelChange(rain));
        events.push(ThunderLevelChange(thunder));
    }
    WeatherLevelStep {
        rain,
        thunder,
        events,
    }
}

#[must_use]
pub fn join_weather_events(rain: f32, thunder: f32) -> WeatherEvents {
    let mut events = WeatherEvents::default();
    if is_wet(rain) {
        events.push(BeginRaining);
        events.push(RainLevelChange(rain));
        events.push(ThunderLevelChange(thunder));
    }
    events
}
