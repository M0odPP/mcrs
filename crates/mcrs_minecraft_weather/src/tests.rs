use core::mem::{align_of, size_of};
use mcrs_test_alloc::CountingAllocator;

use super::*;
use crate::StrengthTarget::{Decay, Rise};
use crate::WeatherAdvance::{Frozen, Normal};
use crate::WeatherGameEvent::{BeginRaining, EndRaining};
use crate::WeatherKind::{Clear, Rain, Thunder};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const fn layout<T>(s: usize, a: usize) -> bool {
    size_of::<T>() == s && align_of::<T>() == a
}

const _: () = {
    assert!(layout::<WeatherKind>(1, 1), "WeatherKind layout");
    assert!(layout::<WeatherState>(16, 4), "WeatherState layout");
    assert!(layout::<WeatherCycleRng>(8, 8), "WeatherCycleRng layout");
    assert!(layout::<WeatherAdvance>(1, 1), "WeatherAdvance layout");
    assert!(layout::<StrengthTarget>(1, 1), "StrengthTarget layout");
    assert!(layout::<WeatherGameEvent>(8, 4), "WeatherGameEvent layout");
    assert!(layout::<WeatherEvents>(44, 4), "WeatherEvents layout");
    assert!(layout::<WeatherLevelStep>(52, 4), "WeatherLevelStep layout");
};

fn st(c: u32, r: u32, t: u32, f: u8) -> WeatherState {
    WeatherState {
        clear_ticks: c,
        rain_ticks: r,
        thunder_ticks: t,
        raining: f & 1 != 0,
        thundering: f & 2 != 0,
    }
}

#[test]
fn cycle_timers_rollover_and_freeze_follow_rules() {
    let mut rng = WeatherCycleRng::new(7);
    assert!(!rng.one_in(0) && rng.one_in(1), "one_in edge failed");
    assert_eq!(rng.next_u32_bound(1), 0, "unit bound must return zero");
    let forced = st(2, 9, 8, 3);
    assert_eq!(step_cycle(forced, Frozen, &mut rng), forced, "frozen");
    let c1 = step_cycle(forced, Normal, &mut rng);
    assert_eq!(c1, st(1, 0, 0, 0), "clear tick 1");
    let c0 = step_cycle(c1, Normal, &mut rng);
    assert_eq!(c0, WeatherState::default(), "clear tick 0");
    let on = step_cycle(c0, Normal, &mut rng);
    let r_on = (RAIN_ON_MIN..=RAIN_ON_MAX).contains(&on.rain_ticks);
    let t_on = (THUNDER_ON_MIN..=THUNDER_ON_MAX).contains(&on.thunder_ticks);
    assert!(on.raining && on.thundering && r_on && t_on, "rollover on");
    let down = step_cycle(st(0, 1, 1, 3), Normal, &mut rng);
    assert_eq!(down, st(0, 0, 0, 3), "timer must reach zero first");
    let off = step_cycle(down, Normal, &mut rng);
    let r_off = (RAIN_OFF_MIN..=RAIN_OFF_MAX).contains(&off.rain_ticks);
    let t_off = (THUNDER_OFF_MIN..=THUNDER_OFF_MAX).contains(&off.thunder_ticks);
    let ok = !off.raining && !off.thundering && r_off && t_off;
    assert!(ok, "rollover off");
}

#[test]
fn command_durations_and_rules_cover_bounds() {
    let mut rng = WeatherCycleRng::new(99);
    let mut chk = |k, lo, hi, want| {
        for _ in 0..8 {
            assert!((lo..=hi).contains(&default_duration(k, &mut rng)), "{k:?}");
        }
        assert_eq!(apply_weather_command(k, 1200), want, "cmd {k:?}");
    };
    chk(Clear, CLEAR_TICKS_MIN, CLEAR_TICKS_MAX, st(1200, 0, 0, 0));
    chk(Rain, RAIN_ON_MIN, RAIN_ON_MAX, st(0, 1200, 1200, 1));
    let storm = st(0, 1200, 1200, 3);
    chk(Thunder, THUNDER_ON_MIN, THUNDER_ON_MAX, storm);
    for (a, w) in [(0, (0.0, 0.0)), (1, (1.0, 0.0)), (3, (1.0, 1.0))] {
        assert_eq!(initial_levels(&st(0, 0, 0, a)), w, "initial_levels {a}");
    }
    assert_eq!(step_level(0.995, Rise), 1.0, "rise clamp");
    assert_eq!(step_level(0.005, Decay), 0.0, "decay clamp");
    assert!(!is_wet(0.2) && is_wet(0.20000002), "wet boundary");
    for (r, t, w) in [(0.2, 0.8, false), (0.8, 0.2, false), (0.25, 0.25, true)] {
        assert_eq!(is_thunderstorm(r, t), w, "is_thunderstorm({r},{t})");
    }
    let rod = within_rod_radius_sq;
    assert_eq!(rod(0, 0, 0), Some(0), "rod 0");
    assert_eq!(rod(128, 0, 0), Some(16_384), "rod 128");
    assert_eq!(rod(-90, 90, 0), Some(16_200), "rod diag");
    assert_eq!(rod(91, 91, 0), None, "rod out diag");
    assert_eq!(rod(128, 1, 0), None, "rod out axis");
    assert_eq!(rod(i32::MIN, i32::MIN, i32::MIN), None, "rod min");
    let (w0, w1) = (warm_enough_for_rain(0.149), warm_enough_for_rain(0.15));
    assert!(!w0 && w1, "rain temp");
}

#[test]
fn binary32_ramps_emit_exact_events_without_allocation() {
    let before = mcrs_test_alloc::allocations();
    let (rain_on, clear) = (st(0, 0, 0, 1), WeatherState::default());
    let (mut rain, mut thunder, mut total) = (0.0f32, 0.0f32, 0usize);
    for tick in 1..=101 {
        let s = step_weather_levels(rain, thunder, &rain_on);
        (rain, thunder, total) = (s.rain, s.thunder, total + s.events.len());
        if tick == 20 {
            assert_eq!((rain, s.events.len()), (0.20000002, 4), "t20 r");
            assert_eq!(s.events.as_slice().get(1), Some(&BeginRaining), "t20 e");
        }
    }
    assert_eq!((rain, total), (1.0, 104), "rise total");
    for tick in 1..=101 {
        let s = step_weather_levels(rain, thunder, &clear);
        (rain, thunder) = (s.rain, s.thunder);
        if tick == 81 {
            let e = s.events.as_slice().get(1).copied();
            assert_eq!((rain, e), (0.19000068, Some(EndRaining)), "t81");
        }
    }
    assert_eq!(rain, 0.0, "decay clamp");
    assert!(join_weather_events(0.2, 1.0).is_empty(), "join at 0.2");
    assert_eq!(join_weather_events(0.25, 0.5).len(), 3, "join > 0.2");
    assert_eq!(mcrs_test_alloc::allocations(), before, "zero alloc");
}
