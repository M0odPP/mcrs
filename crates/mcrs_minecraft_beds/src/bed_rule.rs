use core::mem::{align_of, size_of};

pub const BASE_DAY_SKY_LIGHT: f32 = 15.0;
pub const WEATHER_TARGET_SKY_LIGHT: f32 = 4.0;
pub const WEATHER_RAIN_ALPHA: f32 = 0.3125;
pub const WEATHER_THUNDER_ALPHA: f32 = 0.527_343_75;
pub const SKY_LIGHT_SLEEP_THRESHOLD: f32 = 11.0;
pub const NIGHT_SKY_LIGHT_FACTOR: f32 = 0.266_666_68;
pub const CLEAR_NIGHT_START_TICK: u32 = 12_523;
pub const CLEAR_NIGHT_END_TICK: u32 = 23_477;
pub const FULL_RAIN_NIGHT_START_TICK: u32 = 12_002;
pub const FULL_RAIN_NIGHT_END_TICK: u32 = 23_998;
pub const BED_EXPLOSION_SPEC: BedExplosionSpec = BedExplosionSpec {
    power: 5.0,
    creates_fire: true,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BedRuleCondition {
    Always,
    WhenDark,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BedRuleSpec {
    pub can_sleep: BedRuleCondition,
    pub can_set_spawn: BedRuleCondition,
    pub destroy_on_use: bool,
    pub destroy_on_leave: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BedExplosionSpec {
    pub power: f32,
    pub creates_fire: bool,
}

impl BedRuleSpec {
    #[must_use]
    pub const fn explosion_spec(self) -> Option<BedExplosionSpec> {
        if self.destroy_on_use {
            Some(BED_EXPLOSION_SPEC)
        } else {
            None
        }
    }
}

pub const OVERWORLD_BED_RULE: BedRuleSpec = BedRuleSpec {
    can_sleep: BedRuleCondition::WhenDark,
    can_set_spawn: BedRuleCondition::Always,
    destroy_on_use: false,
    destroy_on_leave: false,
};

pub const OVERWORLD_STRAW_BED_RULE: BedRuleSpec = BedRuleSpec {
    can_sleep: BedRuleCondition::WhenDark,
    can_set_spawn: BedRuleCondition::Never,
    destroy_on_use: false,
    destroy_on_leave: true,
};

pub const EXPLODING_BED_RULE: BedRuleSpec = BedRuleSpec {
    can_sleep: BedRuleCondition::Never,
    can_set_spawn: BedRuleCondition::Never,
    destroy_on_use: true,
    destroy_on_leave: false,
};

fn alpha_blend(subject: f32, target: f32, alpha: f32) -> f32 {
    subject + alpha * (target - subject)
}

#[must_use]
pub fn day_timeline_sky_factor(day_time: i64) -> f32 {
    let wrapped = day_time.rem_euclid(24_000);
    if (133..=11_867).contains(&wrapped) {
        return 1.0;
    }
    if (13_670..=22_330).contains(&wrapped) {
        return NIGHT_SKY_LIGHT_FACTOR;
    }
    if wrapped > 11_867 && wrapped < 13_670 {
        return alpha_blend(
            1.0,
            NIGHT_SKY_LIGHT_FACTOR,
            (wrapped - 11_867) as f32 / 1_803.0,
        );
    }
    let offset = if wrapped > 22_330 {
        wrapped - 22_330
    } else {
        1_670 + wrapped
    } as f32;
    alpha_blend(NIGHT_SKY_LIGHT_FACTOR, 1.0, offset / 1_803.0)
}

#[must_use]
pub fn effective_sky_light_level(
    timeline_sky_light: f32,
    rain_level: f32,
    thunder_level: f32,
) -> f32 {
    let rain = rain_level.clamp(0.0, 1.0);
    let storm = rain * thunder_level.clamp(0.0, 1.0);
    let rain_only = rain - storm;
    let mut level = timeline_sky_light;
    if rain_only > 0.0 {
        let target = alpha_blend(level, WEATHER_TARGET_SKY_LIGHT, WEATHER_RAIN_ALPHA);
        level = alpha_blend(level, target, rain_only);
    }
    if storm > 0.0 {
        let target = alpha_blend(level, WEATHER_TARGET_SKY_LIGHT, WEATHER_THUNDER_ALPHA);
        level = alpha_blend(level, target, storm);
    }
    level.clamp(0.0, BASE_DAY_SKY_LIGHT)
}

#[must_use]
pub fn is_dark_for_sleep(sky_light_level: f32) -> bool {
    sky_light_level < SKY_LIGHT_SLEEP_THRESHOLD
}

#[must_use]
pub fn can_condition_pass(condition: BedRuleCondition, sky_light_level: f32) -> bool {
    match condition {
        BedRuleCondition::Always => true,
        BedRuleCondition::WhenDark => is_dark_for_sleep(sky_light_level),
        BedRuleCondition::Never => false,
    }
}

const _: () = assert!(
    size_of::<BedRuleSpec>() == 4 && align_of::<BedRuleSpec>() == 1,
    "BedRuleSpec layout"
);
