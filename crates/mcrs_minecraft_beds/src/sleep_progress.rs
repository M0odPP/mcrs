use core::mem::{align_of, size_of};
use mcrs_minecraft_pose::Pose;

use crate::bed_rule::BedRuleSpec;

pub const SLEEP_TIMER_MAX_TICKS: i16 = 100;
pub const TICKS_PER_DAY: i64 = 24_000;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SleepExitReason {
    StopSleepingPacket,
    NightSkipped,
    BedMissing,
    RuleDisallowed,
    DimensionUnloaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SleepProgress {
    pub timer: i16,
    pub deep_sleep: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SleepWakeOutcome {
    pub next_pose: Pose,
    pub destroy_bed: bool,
    pub reset_sleep_timer: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SleepSkipStats {
    pub total_players: u32,
    pub sleeping_players: u32,
    pub deep_sleeping_players: u32,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SleepSkipInput {
    pub stats: SleepSkipStats,
    pub percentage: i32,
    pub current_day_time: i64,
    pub advance_time: bool,
    pub raining_or_thundering: bool,
    pub advance_weather: bool,
}

impl SleepSkipInput {
    #[must_use]
    pub const fn new(stats: SleepSkipStats, percentage: i32, current_day_time: i64) -> Self {
        Self {
            stats,
            percentage,
            current_day_time,
            advance_time: true,
            raining_or_thundering: false,
            advance_weather: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NightSkipDecision {
    pub should_skip: bool,
    pub next_day_time: i64,
    pub clear_weather: bool,
}

#[must_use]
pub fn step_sleep_timer(current: i16, delta: i16) -> SleepProgress {
    let timer = current
        .saturating_add(delta)
        .clamp(0, SLEEP_TIMER_MAX_TICKS);
    SleepProgress {
        timer,
        deep_sleep: timer >= SLEEP_TIMER_MAX_TICKS,
    }
}

#[must_use]
pub const fn evaluate_sleep_wake(rule: BedRuleSpec, reason: SleepExitReason) -> SleepWakeOutcome {
    let destroy_bed = !matches!(reason, SleepExitReason::BedMissing) && rule.destroy_on_leave;
    SleepWakeOutcome {
        next_pose: Pose::Standing,
        destroy_bed,
        reset_sleep_timer: 0,
    }
}

#[must_use]
pub fn required_sleeping_players(total_players: u32, percentage: i32) -> Option<u32> {
    if total_players == 0 {
        return None;
    }
    if percentage <= 0 {
        return Some(1);
    }
    let rounded = u64::from(total_players)
        .saturating_mul(percentage as u64)
        .saturating_add(99)
        / 100;
    if rounded > u64::from(total_players) {
        return None;
    }
    u32::try_from(rounded).ok().map(|count| count.max(1))
}

#[must_use]
pub fn next_morning_tick(current_day_time: i64) -> i64 {
    let time_of_day = current_day_time.rem_euclid(TICKS_PER_DAY);
    current_day_time
        .saturating_add(TICKS_PER_DAY)
        .saturating_sub(time_of_day)
}

#[must_use]
pub fn evaluate_night_skip(input: &SleepSkipInput) -> NightSkipDecision {
    let required = required_sleeping_players(input.stats.total_players, input.percentage);
    let enough = required.is_some_and(|needed| {
        input.stats.sleeping_players >= needed && input.stats.deep_sleeping_players >= needed
    });
    if !enough {
        return NightSkipDecision {
            should_skip: false,
            next_day_time: input.current_day_time,
            clear_weather: false,
        };
    }
    let next_day_time = if input.advance_time {
        next_morning_tick(input.current_day_time)
    } else {
        input.current_day_time
    };
    NightSkipDecision {
        should_skip: true,
        next_day_time,
        clear_weather: input.advance_weather && input.raining_or_thundering,
    }
}

const _: () = {
    const fn layout<T>(size: usize, align: usize) -> bool {
        size_of::<T>() == size && align_of::<T>() == align
    }
    assert!(layout::<SleepProgress>(4, 2), "SleepProgress layout");
    assert!(
        layout::<NightSkipDecision>(16, 8),
        "NightSkipDecision layout"
    );
};
