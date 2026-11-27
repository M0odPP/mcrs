use glam::DVec3;
#[cfg(feature = "item-stack")]
use mcrs_minecraft_item_stack::{AttackRange, Weapon};

use crate::blocking_rules::fits;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReachMode {
    #[default]
    Survival,
    Creative,
    Mob,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReachVerdict {
    InRange,
    TooClose,
    TooFar,
    InvalidRange,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ReachBounds {
    pub min_reach: f32,
    pub max_reach: f32,
    pub hitbox_margin: f32,
}

impl ReachBounds {
    #[must_use]
    pub const fn new(min_reach: f32, max_reach: f32, hitbox_margin: f32) -> Self {
        Self {
            min_reach,
            max_reach,
            hitbox_margin,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TargetBounds {
    pub min: DVec3,
    pub max: DVec3,
}

const _: () = {
    assert!(
        fits::<ReachMode>(1, 1),
        "ReachMode size 1 byte and 1-byte alignment"
    );
    assert!(
        fits::<ReachVerdict>(1, 1),
        "ReachVerdict size 1 byte and 1-byte alignment"
    );
    assert!(
        fits::<ReachBounds>(12, 4),
        "ReachBounds size 12 bytes and 4-byte alignment"
    );
    assert!(
        fits::<TargetBounds>(48, 8),
        "TargetBounds size 48 bytes and 8-byte alignment"
    );
};

impl TargetBounds {
    #[must_use]
    pub const fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.min.cmple(self.max).all()
    }

    #[must_use]
    pub fn of_body(position: DVec3, width: f64, height: f64) -> Option<Self> {
        let finite = position.is_finite() && width.is_finite() && height.is_finite();
        let non_negative = width >= 0.0 && height >= 0.0;
        let half = width * 0.5;
        let bounds = Self {
            min: DVec3::new(position.x - half, position.y, position.z - half),
            max: DVec3::new(position.x + half, position.y + height, position.z + half),
        };
        (finite && non_negative && bounds.is_valid()).then_some(bounds)
    }

    #[must_use]
    pub fn nearest_distance(self, eye: DVec3) -> f64 {
        if !eye.is_finite() || !self.is_valid() {
            return f64::NAN;
        }
        eye.distance(eye.min(self.max).max(self.min))
    }

    #[must_use]
    pub fn farthest_distance(self, eye: DVec3) -> f64 {
        if !eye.is_finite() || !self.is_valid() {
            return f64::NAN;
        }
        (eye - self.min).abs().max((eye - self.max).abs()).length()
    }
}

#[must_use]
pub fn check_reach_bounds(
    bounds: ReachBounds,
    eye: DVec3,
    target: TargetBounds,
    tolerance: f64,
) -> ReachVerdict {
    let valid = target.is_valid() && eye.is_finite() && tolerance.is_finite() && tolerance >= 0.0;
    let finite = bounds.max_reach.is_finite() && bounds.hitbox_margin.is_finite();
    let ordered = bounds.min_reach >= 0.0
        && bounds.max_reach >= bounds.min_reach
        && bounds.hitbox_margin >= 0.0;
    if !valid || !finite || !ordered {
        return ReachVerdict::InvalidRange;
    }
    let margin = f64::from(bounds.hitbox_margin) + tolerance;
    if target.nearest_distance(eye) > f64::from(bounds.max_reach) + margin {
        return ReachVerdict::TooFar;
    }
    if target.farthest_distance(eye) < f64::from(bounds.min_reach) - margin {
        return ReachVerdict::TooClose;
    }
    ReachVerdict::InRange
}

fn delay_ticks(attack_speed: f64) -> Option<f32> {
    (attack_speed.is_finite() && attack_speed > 0.0).then(|| (1.0 / attack_speed * 20.0) as f32)
}

#[must_use]
pub fn attack_strength_scale(counter: u32, attack_speed: f64) -> f32 {
    delay_ticks(attack_speed).map_or(1.0, |delay| {
        ((counter as f32 + 0.5) / delay).clamp(0.0, 1.0)
    })
}

#[must_use]
pub fn has_minimum_attack_charge(counter: u32, attack_speed: f64, minimum_charge: f32) -> bool {
    let ready = delay_ticks(attack_speed)
        .is_some_and(|delay| (counter as f32 + 5.0) / delay >= minimum_charge);
    minimum_charge.is_finite() && (minimum_charge <= 0.0 || ready)
}

#[must_use]
pub fn scaled_attack_damage(base_damage: f64, attack_strength: f32) -> f32 {
    let valid = base_damage.is_finite() && base_damage > 0.0 && attack_strength.is_finite();
    let strength = attack_strength.clamp(0.0, 1.0);
    let amount = base_damage as f32 * (0.2 + strength * strength * 0.8);
    if valid && amount.is_finite() && amount > 0.0 {
        amount
    } else {
        0.0
    }
}

#[cfg(feature = "item-stack")]
#[must_use]
pub fn evaluate_attack_range(
    range: &AttackRange,
    mode: ReachMode,
    eye: DVec3,
    target: TargetBounds,
    tolerance: f64,
) -> ReachVerdict {
    let mob_factor = range.mob_factor;
    let (min_reach, max_reach) = match mode {
        ReachMode::Survival => (range.min_reach, range.max_reach),
        ReachMode::Creative => (range.min_creative_reach, range.max_creative_reach),
        ReachMode::Mob => (range.min_reach * mob_factor, range.max_reach * mob_factor),
    };
    let bounds = ReachBounds::new(min_reach, max_reach, range.hitbox_margin);
    check_reach_bounds(bounds, eye, target, tolerance)
}

#[cfg(feature = "item-stack")]
#[must_use]
pub fn weapon_attack_item_damage(weapon: Option<&Weapon>) -> i32 {
    let fallback = Weapon::DEFAULT_ITEM_DAMAGE_PER_ATTACK;
    weapon.map_or(fallback, |entry| entry.item_damage_per_attack.max(0))
}
