use crate::constants::{
    CLEAR_TICKS_MAX, CLEAR_TICKS_MIN, RAIN_OFF_MAX, RAIN_OFF_MIN, RAIN_ON_MAX, RAIN_ON_MIN,
    THUNDER_OFF_MAX, THUNDER_OFF_MIN, THUNDER_ON_MAX, THUNDER_ON_MIN,
};
use crate::weather_advance::WeatherAdvance;
use crate::weather_cycle_rng::WeatherCycleRng;
use crate::weather_kind::WeatherKind::{self, Clear, Rain, Thunder};
use crate::weather_state::WeatherState;

const OFF: [(u32, u32); 2] = [
    (THUNDER_OFF_MIN, THUNDER_OFF_MAX),
    (RAIN_OFF_MIN, RAIN_OFF_MAX),
];
const ON: [(u32, u32); 2] = [(THUNDER_ON_MIN, THUNDER_ON_MAX), (RAIN_ON_MIN, RAIN_ON_MAX)];

#[must_use]
pub fn default_duration(kind: WeatherKind, rng: &mut WeatherCycleRng) -> u32 {
    match kind {
        Clear => rng.inclusive_range(CLEAR_TICKS_MIN, CLEAR_TICKS_MAX),
        Rain => rng.inclusive_range(RAIN_ON_MIN, RAIN_ON_MAX),
        Thunder => rng.inclusive_range(THUNDER_ON_MIN, THUNDER_ON_MAX),
    }
}

#[must_use]
pub fn apply_weather_command(kind: WeatherKind, ticks: u32) -> WeatherState {
    let mut s = WeatherState::default();
    match kind {
        Clear => s.clear_ticks = ticks,
        Rain | Thunder => {
            (s.rain_ticks, s.thunder_ticks, s.raining) = (ticks, ticks, true);
            s.thundering = kind == Thunder;
        }
    }
    s
}

fn timer(flag: &mut bool, ticks: &mut u32, b: [(u32, u32); 2], rng: &mut WeatherCycleRng) {
    if *ticks == 0 {
        *flag = !*flag;
        let [off, on] = b;
        let (lo, hi) = if *flag { on } else { off };
        *ticks = rng.inclusive_range(lo, hi);
    } else {
        *ticks -= 1;
    }
}

#[must_use]
pub fn step_cycle(
    mut s: WeatherState,
    adv: WeatherAdvance,
    rng: &mut WeatherCycleRng,
) -> WeatherState {
    if adv == WeatherAdvance::Frozen {
        return s;
    }
    if s.clear_ticks > 0 {
        s.clear_ticks -= 1;
        (s.rain_ticks, s.thunder_ticks, s.raining, s.thundering) = (0, 0, false, false);
        return s;
    }
    let (tb, rb) = ([OFF[0], ON[0]], [OFF[1], ON[1]]);
    timer(&mut s.thundering, &mut s.thunder_ticks, tb, rng);
    timer(&mut s.raining, &mut s.rain_ticks, rb, rng);
    s
}
