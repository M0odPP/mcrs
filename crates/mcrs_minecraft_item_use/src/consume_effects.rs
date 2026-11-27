use core::mem::{align_of, size_of};

pub const DEFAULT_POTION_DURATION_SCALE: f32 = 1.0;
pub const INFINITE_EFFECT_DURATION_TICKS: i32 = -1;
pub const SUSPICIOUS_STEW_DEFAULT_DURATION_TICKS: i32 = 160;
pub const OMINOUS_BOTTLE_DURATION_TICKS: i32 = 120_000;
pub const OMINOUS_BOTTLE_MAX_AMPLIFIER: u8 = 4;
pub const OMINOUS_BOTTLE_EFFECT_ID: &str = "minecraft:bad_omen";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EffectTiming {
    #[default]
    Timed,
    Instant,
}

const _: () = assert!(
    size_of::<EffectTiming>() == 1 && align_of::<EffectTiming>() == 1,
    "EffectTiming fits in one byte with 1-byte alignment"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ConsumableEffectInstance<I> {
    pub effect: I,
    pub amplifier: u8,
    pub duration: i32,
    pub ambient: bool,
    pub show_particles: bool,
    pub show_icon: bool,
    pub timing: EffectTiming,
}

impl<I> ConsumableEffectInstance<I> {
    #[must_use]
    pub const fn new(effect: I, duration: i32, timing: EffectTiming) -> Self {
        Self {
            effect,
            amplifier: 0,
            duration,
            ambient: false,
            show_particles: true,
            show_icon: true,
            timing,
        }
    }
}

const _: () = assert!(
    size_of::<ConsumableEffectInstance<()>>() == 12
        && align_of::<ConsumableEffectInstance<()>>() == 4,
    "ConsumableEffectInstance<()> packs duration and five byte fields into 12 bytes"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct StewEffectEntry<I> {
    pub effect: I,
    pub duration: i32,
}

impl<I> StewEffectEntry<I> {
    #[must_use]
    pub const fn new(effect: I, duration: i32) -> Self {
        Self { effect, duration }
    }
}

const _: () = assert!(
    size_of::<StewEffectEntry<()>>() == 4 && align_of::<StewEffectEntry<()>>() == 4,
    "StewEffectEntry<()> wraps duration in 4 bytes"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct AppliedConsumeEffect<I> {
    pub effect: I,
    pub amplifier: u8,
    pub duration: i32,
    pub ambient: bool,
    pub show_particles: bool,
    pub show_icon: bool,
}

impl<I> AppliedConsumeEffect<I> {
    #[must_use]
    pub const fn new(effect: I, duration: i32) -> Self {
        Self {
            effect,
            amplifier: 0,
            duration,
            ambient: false,
            show_particles: true,
            show_icon: true,
        }
    }
}

const _: () = assert!(
    size_of::<AppliedConsumeEffect<()>>() == 8 && align_of::<AppliedConsumeEffect<()>>() == 4,
    "AppliedConsumeEffect<()> packs duration and four byte fields into 8 bytes"
);

#[must_use]
pub const fn scale_potion_duration(
    duration: i32,
    timing: EffectTiming,
    duration_scale: f32,
) -> i32 {
    if matches!(timing, EffectTiming::Instant)
        || duration <= 0
        || duration_scale.to_bits() == DEFAULT_POTION_DURATION_SCALE.to_bits()
        || !duration_scale.is_finite()
        || duration_scale < 0.0
    {
        return duration;
    }
    let raw = (duration as f32) * duration_scale;
    if raw >= i32::MAX as f32 {
        i32::MAX
    } else if raw >= 8_388_608.0 {
        raw as i32
    } else {
        let rounded = (raw + 0.5) as i32;
        if rounded <= 1 { 1 } else { rounded }
    }
}

pub fn collect_potion_effects<I>(
    base_effects: impl IntoIterator<Item = ConsumableEffectInstance<I>>,
    custom_effects: impl IntoIterator<Item = ConsumableEffectInstance<I>>,
    duration_scale: f32,
    mut emit: impl FnMut(AppliedConsumeEffect<I>),
) {
    let effects = base_effects.into_iter().chain(custom_effects);
    for effect in effects {
        emit(AppliedConsumeEffect {
            effect: effect.effect,
            amplifier: effect.amplifier,
            duration: scale_potion_duration(effect.duration, effect.timing, duration_scale),
            ambient: effect.ambient,
            show_particles: effect.show_particles,
            show_icon: effect.show_icon,
        });
    }
}

pub fn collect_stew_effects<I>(
    entries: impl IntoIterator<Item = StewEffectEntry<I>>,
    mut emit: impl FnMut(AppliedConsumeEffect<I>),
) {
    for entry in entries {
        emit(AppliedConsumeEffect::new(entry.effect, entry.duration));
    }
}

#[must_use]
pub fn ominous_bottle_effect(amplifier: i32) -> AppliedConsumeEffect<&'static str> {
    let clamped = amplifier.clamp(0, i32::from(OMINOUS_BOTTLE_MAX_AMPLIFIER)) as u8;
    AppliedConsumeEffect {
        effect: OMINOUS_BOTTLE_EFFECT_ID,
        amplifier: clamped,
        duration: OMINOUS_BOTTLE_DURATION_TICKS,
        ambient: false,
        show_particles: false,
        show_icon: true,
    }
}
