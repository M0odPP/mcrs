pub const FALL_DAMAGE_EPSILON: f64 = 1.0e-6;
pub const STALAGMITE_DISTANCE_BONUS: f64 = 2.5;
pub const BIG_FALL_DAMAGE_THRESHOLD: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundState {
    Airborne,
    OnGround,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallResetMode {
    Active,
    ResetToZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallDamageKind {
    Fall,
    Stalagmite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallBlockKind {
    Default,
    HayBlock,
    HoneyBlock,
    SlimeBlock,
    Bed,
    ShelfMushroom,
    PowderSnow,
    PointedDripstoneTip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallSoundKind {
    None,
    Small,
    Big,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallStepInput {
    pub fall_distance: f64,
    pub delta_y: f64,
    pub ground: GroundState,
    pub reset: FallResetMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallStepOutcome {
    pub next_fall_distance: f64,
    pub landed_distance: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallOutcome {
    pub kind: FallDamageKind,
    pub damage: Option<f32>,
    pub sound: FallSoundKind,
    pub honey_slide: bool,
    pub triggers_block_fall_sound: bool,
}

crate::check_layout!(GroundState, 1, 1, "GroundState");
crate::check_layout!(FallResetMode, 1, 1, "FallResetMode");
crate::check_layout!(FallDamageKind, 1, 1, "FallDamageKind");
crate::check_layout!(FallBlockKind, 1, 1, "FallBlockKind");
crate::check_layout!(FallSoundKind, 1, 1, "FallSoundKind");
crate::check_layout!(FallStepInput, 24, 8, "FallStepInput");
crate::check_layout!(FallStepOutcome, 24, 8, "FallStepOutcome");
crate::check_layout!(FallOutcome, 12, 4, "FallOutcome");

impl FallOutcome {
    const NONE: Self = Self {
        kind: FallDamageKind::Fall,
        damage: None,
        sound: FallSoundKind::None,
        honey_slide: false,
        triggers_block_fall_sound: false,
    };
}

#[must_use]
pub const fn fall_sound_for_damage(damage_points: i32) -> FallSoundKind {
    if damage_points <= 0 {
        FallSoundKind::None
    } else if damage_points > BIG_FALL_DAMAGE_THRESHOLD {
        FallSoundKind::Big
    } else {
        FallSoundKind::Small
    }
}

#[must_use]
pub fn step_player_fall(input: FallStepInput) -> FallStepOutcome {
    if input.reset == FallResetMode::ResetToZero {
        return FallStepOutcome {
            next_fall_distance: 0.0,
            landed_distance: None,
        };
    }
    let bad = input.delta_y > 0.0 || !input.fall_distance.is_finite() || input.fall_distance < 0.0;
    let mut distance = if bad { 0.0 } else { input.fall_distance };
    if input.delta_y < 0.0 && input.delta_y.is_finite() {
        distance -= input.delta_y;
    }
    match input.ground {
        GroundState::OnGround => FallStepOutcome {
            next_fall_distance: 0.0,
            landed_distance: (distance > 0.0).then_some(distance),
        },
        GroundState::Airborne => FallStepOutcome {
            next_fall_distance: distance,
            landed_distance: None,
        },
    }
}

#[must_use]
pub fn fall_damage(
    effective_distance: f64,
    safe_fall_distance: f64,
    block_multiplier: f32,
    entity_multiplier: f64,
) -> i32 {
    let raw = libm::floor(
        (((effective_distance + FALL_DAMAGE_EPSILON) - safe_fall_distance)
            * f64::from(block_multiplier))
            * entity_multiplier,
    );
    if raw.is_nan() || raw <= 0.0 {
        0
    } else if raw >= i32::MAX as f64 {
        i32::MAX
    } else {
        raw as i32
    }
}

#[must_use]
pub fn compute_fall_outcome(
    distance: f64,
    block: FallBlockKind,
    safe_fall_distance: f64,
    fall_damage_multiplier: f64,
) -> FallOutcome {
    if distance.is_nan() || distance <= 0.0 {
        return FallOutcome::NONE;
    }
    if block == FallBlockKind::PowderSnow {
        let sound = if distance >= 7.0 {
            FallSoundKind::Big
        } else if distance >= 4.0 {
            FallSoundKind::Small
        } else {
            FallSoundKind::None
        };
        return FallOutcome {
            sound,
            ..FallOutcome::NONE
        };
    }
    let cushioned = distance * f64::from(1.0_f32 - 0.5_f32);
    let (effective, multiplier, kind, honey) = match block {
        FallBlockKind::Default => (distance, 1.0_f32, FallDamageKind::Fall, false),
        FallBlockKind::HayBlock => (distance, 0.2_f32, FallDamageKind::Fall, false),
        FallBlockKind::HoneyBlock => (distance, 0.2_f32, FallDamageKind::Fall, true),
        FallBlockKind::SlimeBlock => (distance, 0.0_f32, FallDamageKind::Fall, false),
        FallBlockKind::Bed | FallBlockKind::ShelfMushroom => {
            (cushioned, 1.0, FallDamageKind::Fall, false)
        }
        FallBlockKind::PointedDripstoneTip => (
            distance + STALAGMITE_DISTANCE_BONUS,
            2.0,
            FallDamageKind::Stalagmite,
            false,
        ),
        FallBlockKind::PowderSnow => (0.0, 0.0, FallDamageKind::Fall, false),
    };
    let points = fall_damage(
        effective,
        safe_fall_distance,
        multiplier,
        fall_damage_multiplier,
    );
    let hurt = points > 0;
    FallOutcome {
        kind,
        damage: hurt.then_some(points as f32),
        sound: fall_sound_for_damage(points),
        honey_slide: honey,
        triggers_block_fall_sound: hurt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fall_step_and_damage_table() {
        use FallResetMode::{Active, ResetToZero};
        use GroundState::{Airborne, OnGround};
        let step = |fall_distance, delta_y, ground, reset| {
            step_player_fall(FallStepInput {
                fall_distance,
                delta_y,
                ground,
                reset,
            })
        };
        let airborne = step(2.0, -3.5, Airborne, Active);
        let upward = step(5.5, 0.42, Airborne, Active);
        let landed = step(9.25, -0.75, OnGround, Active);
        let reset = step(12.0, -1.0, OnGround, ResetToZero);
        let step_ok = airborne.next_fall_distance == 5.5
            && upward.next_fall_distance == 0.0
            && landed.landed_distance == Some(10.0)
            && reset.landed_distance.is_none();
        assert!(step_ok, "fall tracks delta, landing, and reset");
        let (safe, normal, hay, infinity) = (
            fall_damage(3.0, 3.0, 1.0, 1.0),
            fall_damage(10.0, 3.0, 1.0, 1.0),
            fall_damage(10.0, 3.0, 0.2, 1.0),
            fall_damage(f64::INFINITY, 3.0, 1.0, 1.0),
        );
        let formula_ok = safe == 0 && normal == 7 && hay == 1 && infinity == i32::MAX;
        assert!(formula_ok, "damage scales by multiplier and clamps");
        let outcome = |distance, block| compute_fall_outcome(distance, block, 3.0, 1.0);
        let big = outcome(10.0, FallBlockKind::Default);
        let small = outcome(10.0, FallBlockKind::HayBlock);
        let honey = outcome(4.0, FallBlockKind::HoneyBlock);
        let slime = outcome(25.0, FallBlockKind::SlimeBlock);
        let bed = outcome(10.0, FallBlockKind::Bed);
        let shelf = outcome(10.0, FallBlockKind::ShelfMushroom);
        let dripstone = outcome(6.0, FallBlockKind::PointedDripstoneTip);
        let snow = outcome(5.0, FallBlockKind::PowderSnow);
        let cushion_ok = honey.honey_slide
            && honey.damage.is_none()
            && slime.damage.is_none()
            && bed.damage == Some(2.0)
            && shelf.damage == Some(2.0);
        let edge_ok = big.sound == FallSoundKind::Big
            && small.sound == FallSoundKind::Small
            && dripstone.kind == FallDamageKind::Stalagmite
            && dripstone.damage == Some(11.0)
            && snow.sound == FallSoundKind::Small
            && !snow.triggers_block_fall_sound
            && outcome(f64::NAN, FallBlockKind::Default) == FallOutcome::NONE;
        assert!(cushion_ok, "honey, slime, bed, shelf apply fall rules");
        assert!(edge_ok, "sounds, dripstone, powder snow, NaN match");
    }
}
