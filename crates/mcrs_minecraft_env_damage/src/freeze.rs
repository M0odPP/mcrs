pub const MAX_FROZEN_TICKS: i32 = 140;
pub const FREEZE_DAMAGE_INTERVAL: u64 = 40;
pub const FREEZE_GAIN_PER_TICK: i32 = 1;
pub const FREEZE_THAW_PER_TICK: i32 = 2;
pub const FREEZE_DAMAGE_DEFAULT: f32 = 1.0;
pub const FREEZE_DAMAGE_EFFECT: f32 = 1.5;
pub const FREEZE_DAMAGE_FIRE_SENSITIVE: f32 = 5.0;
pub const FREEZE_SPEED_MAX_PENALTY: f32 = -0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowderSnowExposure {
    Outside,
    Inside,
    FreezingEffect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeImmunity {
    Normal,
    ColdImmune,
    FireSensitive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnFireState {
    NotBurning,
    Burning,
    FireOrLavaContact,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreezeStepOutcome {
    pub next_ticks_frozen: i32,
    pub damage: Option<f32>,
    pub speed_modifier: f64,
}

crate::check_layout!(PowderSnowExposure, 1, 1, "PowderSnowExposure");
crate::check_layout!(FreezeImmunity, 1, 1, "FreezeImmunity");
crate::check_layout!(OnFireState, 1, 1, "OnFireState");
crate::check_layout!(FreezeStepOutcome, 24, 8, "FreezeStepOutcome");

#[must_use]
pub fn powder_snow_speed_modifier(ticks_frozen: i32) -> f64 {
    if ticks_frozen <= 0 {
        return 0.0;
    }
    let ratio = ticks_frozen.min(MAX_FROZEN_TICKS) as f32 / MAX_FROZEN_TICKS as f32;
    f64::from(FREEZE_SPEED_MAX_PENALTY * ratio)
}

#[must_use]
pub fn step_freeze(
    ticks_frozen: i32,
    exposure: PowderSnowExposure,
    on_fire: OnFireState,
    tick_age: u64,
    immunity: FreezeImmunity,
) -> FreezeStepOutcome {
    let can_freeze = matches!(on_fire, OnFireState::NotBurning)
        && !matches!(immunity, FreezeImmunity::ColdImmune);
    let next_ticks_frozen = match on_fire {
        OnFireState::FireOrLavaContact => 0,
        OnFireState::Burning => ticks_frozen.saturating_sub(FREEZE_THAW_PER_TICK).max(0),
        OnFireState::NotBurning if can_freeze => match exposure {
            PowderSnowExposure::Inside => ticks_frozen
                .saturating_add(FREEZE_GAIN_PER_TICK)
                .min(MAX_FROZEN_TICKS),
            PowderSnowExposure::FreezingEffect => ticks_frozen.max(0),
            PowderSnowExposure::Outside => ticks_frozen.saturating_sub(FREEZE_THAW_PER_TICK).max(0),
        },
        OnFireState::NotBurning => ticks_frozen.saturating_sub(FREEZE_THAW_PER_TICK).max(0),
    };
    let damage = if can_freeze
        && next_ticks_frozen >= MAX_FROZEN_TICKS
        && tick_age.is_multiple_of(FREEZE_DAMAGE_INTERVAL)
    {
        let base_damage = match exposure {
            PowderSnowExposure::FreezingEffect => FREEZE_DAMAGE_EFFECT,
            PowderSnowExposure::Outside | PowderSnowExposure::Inside => FREEZE_DAMAGE_DEFAULT,
        };
        let multiplier = match immunity {
            FreezeImmunity::FireSensitive => FREEZE_DAMAGE_FIRE_SENSITIVE,
            FreezeImmunity::Normal => 1.0,
            FreezeImmunity::ColdImmune => 0.0,
        };
        Some(base_damage * multiplier)
    } else {
        None
    };
    FreezeStepOutcome {
        next_ticks_frozen,
        damage,
        speed_modifier: powder_snow_speed_modifier(next_ticks_frozen),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_accumulation_thaw_and_speed() {
        use FreezeImmunity::{ColdImmune, FireSensitive, Normal};
        use OnFireState::{Burning, FireOrLavaContact, NotBurning};
        use PowderSnowExposure::{FreezingEffect, Inside};
        let inside = step_freeze(139, Inside, NotBurning, 40, Normal);
        let capped = inside.next_ticks_frozen == 140
            && inside.damage == Some(1.0)
            && inside.speed_modifier < 0.0;
        let immune = step_freeze(70, Inside, NotBurning, 40, ColdImmune);
        let snow_ok = capped && immune.next_ticks_frozen == 68 && immune.damage.is_none();
        assert!(snow_ok, "snow caps/hurts while immune thaws");
        let thawed = step_freeze(50, Inside, Burning, 40, Normal).next_ticks_frozen == 48;
        let cleared = step_freeze(50, Inside, FireOrLavaContact, 40, Normal).next_ticks_frozen == 0;
        let heat_ok = thawed && cleared;
        assert!(heat_ok, "burning thaws and fire clears freeze");
        let effect_full = step_freeze(142, FreezingEffect, NotBurning, 40, Normal);
        let effect_extra = step_freeze(142, FreezingEffect, NotBurning, 40, FireSensitive);
        let effect_partial = step_freeze(100, FreezingEffect, NotBurning, 40, Normal);
        let effect_burning = step_freeze(142, FreezingEffect, Burning, 40, Normal);
        let effect_ok = effect_full.next_ticks_frozen == 142
            && effect_full.damage == Some(1.5)
            && effect_extra.damage == Some(7.5)
            && effect_partial.next_ticks_frozen == 100
            && effect_partial.damage.is_none()
            && effect_burning.next_ticks_frozen == 140
            && effect_burning.damage.is_none();
        assert!(
            effect_ok,
            "freezing effect preserves ticks, scales, skips fire"
        );
    }
}
