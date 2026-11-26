use core::mem::{align_of, size_of};

pub const DEFAULT_CONSUME_SECONDS: f32 = 1.6;
pub const DEFAULT_USE_SPEED_MULTIPLIER: f32 = 0.2;
pub const SHIELD_USE_DURATION_TICKS: u32 = 72_000;

const TICKS_PER_SECOND: f32 = 20.0;
const CONSUME_WARMUP_NUMERATOR: u32 = 7;
const CONSUME_WARMUP_DENOMINATOR: u32 = 32;
const CONSUME_TICK_EFFECT_INTERVAL: u32 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UseHand {
    #[default]
    Main,
    Off,
}

const _: () = assert!(
    size_of::<UseHand>() == 1 && align_of::<UseHand>() == 1,
    "UseHand fits in one byte with 1-byte alignment"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SprintPolicy {
    #[default]
    Forbid,
    Allow,
}

const _: () = assert!(
    size_of::<SprintPolicy>() == 1 && align_of::<SprintPolicy>() == 1,
    "SprintPolicy fits in one byte with 1-byte alignment"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UseKind {
    #[default]
    Consumable,
    Continuous,
}

const _: () = assert!(
    size_of::<UseKind>() == 1 && align_of::<UseKind>() == 1,
    "UseKind fits in one byte with 1-byte alignment"
);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UseSpec {
    pub use_duration_ticks: u32,
    pub speed_multiplier: f32,
    pub sprint_policy: SprintPolicy,
    pub kind: UseKind,
}

const _: () = assert!(
    size_of::<UseSpec>() == 12,
    "UseSpec stays compact across 12 bytes"
);
const _: () = assert!(align_of::<UseSpec>() == 4, "UseSpec stays 4-byte aligned");

impl UseSpec {
    #[must_use]
    pub const fn consumable(
        consume_seconds: f32,
        speed_multiplier: f32,
        sprint_policy: SprintPolicy,
    ) -> Self {
        Self {
            use_duration_ticks: consume_ticks(consume_seconds),
            speed_multiplier,
            sprint_policy,
            kind: UseKind::Consumable,
        }
    }

    #[must_use]
    pub const fn continuous(speed_multiplier: f32, sprint_policy: SprintPolicy) -> Self {
        Self {
            use_duration_ticks: SHIELD_USE_DURATION_TICKS,
            speed_multiplier,
            sprint_policy,
            kind: UseKind::Continuous,
        }
    }

    #[must_use]
    pub const fn permits_sprint(self) -> bool {
        matches!(self.sprint_policy, SprintPolicy::Allow)
    }

    #[must_use]
    pub const fn is_continuous(self) -> bool {
        matches!(self.kind, UseKind::Continuous)
    }
}

impl Default for UseSpec {
    fn default() -> Self {
        Self {
            use_duration_ticks: 0,
            speed_multiplier: DEFAULT_USE_SPEED_MULTIPLIER,
            sprint_policy: SprintPolicy::Forbid,
            kind: UseKind::Consumable,
        }
    }
}

#[must_use]
pub const fn consume_ticks(consume_seconds: f32) -> u32 {
    if consume_seconds.is_finite() && consume_seconds > 0.0 {
        (consume_seconds * TICKS_PER_SECOND) as u32
    } else {
        0
    }
}

#[must_use]
pub const fn should_emit_consume_tick_effects(duration_ticks: u32, remaining_ticks: u32) -> bool {
    let warmup =
        duration_ticks.saturating_mul(CONSUME_WARMUP_NUMERATOR) / CONSUME_WARMUP_DENOMINATOR;
    let active_threshold = duration_ticks.saturating_sub(warmup);
    remaining_ticks <= active_threshold
        && remaining_ticks.is_multiple_of(CONSUME_TICK_EFFECT_INTERVAL)
}
