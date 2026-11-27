use glam::DVec3;

use crate::interaction::{Difficulty, ExplosionWorld};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityBox {
    pub min: DVec3,
    pub max: DVec3,
}

impl EntityBox {
    #[must_use]
    pub fn from_foot_position(foot_position: DVec3, width: f64, height: f64) -> Self {
        let half = width * 0.5;
        let extent = DVec3::new(half, 0.0, half);
        Self {
            min: foot_position - extent,
            max: foot_position + DVec3::new(half, height, half),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityExplosionInput {
    pub center: DVec3,
    pub radius: f32,
    pub entity_position: DVec3,
    pub knockback_origin: DVec3,
    pub bounds: EntityBox,
    pub knockback_resistance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityExplosionOutcome {
    pub seen_fraction: f32,
    pub raw_damage: f32,
    pub knockback: DVec3,
}

const _: () = {
    use core::mem::{align_of, size_of};
    assert!(
        size_of::<EntityBox>() == 48 && align_of::<EntityBox>() == 8,
        "EntityBox layout"
    );
    assert!(
        size_of::<EntityExplosionInput>() == 136 && align_of::<EntityExplosionInput>() == 8,
        "EntityExplosionInput layout"
    );
    assert!(
        size_of::<EntityExplosionOutcome>() == 32 && align_of::<EntityExplosionOutcome>() == 8,
        "EntityExplosionOutcome layout"
    );
};

#[must_use]
pub fn seen_fraction(center: DVec3, bounds: EntityBox, world: &impl ExplosionWorld) -> f32 {
    let size = bounds.max - bounds.min;
    let step = DVec3::ONE / (size * 2.0 + DVec3::ONE);
    if !size.is_finite()
        || size.min_element() < 0.0
        || !step.is_finite()
        || step.min_element() <= f64::EPSILON
    {
        return 0.0;
    }
    let offset_x = (1.0 - libm::floor(1.0 / step.x) * step.x) * 0.5;
    let offset_z = (1.0 - libm::floor(1.0 / step.z) * step.z) * 0.5;
    let (mut unobstructed, mut total, mut fraction_x) = (0_u64, 0_u64, 0.0);
    while fraction_x <= 1.0 {
        let sample_x = bounds.min.x + size.x * fraction_x + offset_x;
        let mut fraction_y = 0.0;
        while fraction_y <= 1.0 {
            let sample_y = bounds.min.y + size.y * fraction_y;
            let mut fraction_z = 0.0;
            while fraction_z <= 1.0 {
                let sample = DVec3::new(
                    sample_x,
                    sample_y,
                    bounds.min.z + size.z * fraction_z + offset_z,
                );
                if !world.is_segment_occluded(sample, center) {
                    unobstructed = unobstructed.saturating_add(1);
                }
                total = total.saturating_add(1);
                let next_z = fraction_z + step.z;
                if next_z <= fraction_z {
                    break;
                }
                fraction_z = next_z;
            }
            let next_y = fraction_y + step.y;
            if next_y <= fraction_y {
                break;
            }
            fraction_y = next_y;
        }
        let next_x = fraction_x + step.x;
        if next_x <= fraction_x {
            break;
        }
        fraction_x = next_x;
    }
    if total == 0 {
        0.0
    } else {
        (unobstructed as f64 / total as f64) as f32
    }
}

#[must_use]
pub fn distance_falloff(distance: f64, radius: f32) -> f64 {
    if !radius.is_finite() || radius <= 0.0 || !distance.is_finite() || distance < 0.0 {
        return 0.0;
    }
    let diameter = 2.0 * f64::from(radius);
    if distance > diameter {
        0.0
    } else {
        1.0 - (distance / diameter)
    }
}

#[must_use]
pub fn explosion_damage(radius: f32, impact: f64) -> f32 {
    if !radius.is_finite() || radius <= 0.0 || !impact.is_finite() {
        return 0.0;
    }
    ((impact * impact + impact) * 0.5 * 7.0 * (2.0 * f64::from(radius)) + 1.0) as f32
}

#[must_use]
pub fn scale_damage_for_difficulty(damage: f32, difficulty: Difficulty) -> f32 {
    if !damage.is_finite() || damage <= 0.0 {
        return 0.0;
    }
    match difficulty {
        Difficulty::Peaceful => 0.0,
        Difficulty::Easy => (damage * 0.5 + 1.0).min(damage),
        Difficulty::Normal => damage,
        Difficulty::Hard => damage * 1.5,
    }
}

#[must_use]
pub fn explosion_knockback(
    center: DVec3,
    knockback_origin: DVec3,
    impact: f64,
    knockback_resistance: f64,
) -> DVec3 {
    if !impact.is_finite() || impact <= 0.0 || !knockback_resistance.is_finite() {
        return DVec3::ZERO;
    }
    let delta = knockback_origin - center;
    let length = delta.length();
    if !length.is_finite() || length <= 0.0 {
        return DVec3::ZERO;
    }
    (delta / length) * (impact * (1.0 - knockback_resistance.clamp(0.0, 1.0)))
}

#[must_use]
pub fn evaluate_entity_impact(
    input: EntityExplosionInput,
    world: &impl ExplosionWorld,
) -> Option<EntityExplosionOutcome> {
    if !input.radius.is_finite()
        || input.radius <= 0.0
        || !input.center.is_finite()
        || !input.entity_position.is_finite()
    {
        return None;
    }
    let distance = input.entity_position.distance(input.center);
    if !distance.is_finite() || distance > 2.0 * f64::from(input.radius) {
        return None;
    }
    let seen = seen_fraction(input.center, input.bounds, world);
    let impact = distance_falloff(distance, input.radius) * f64::from(seen);
    Some(EntityExplosionOutcome {
        seen_fraction: seen,
        raw_damage: explosion_damage(input.radius, impact),
        knockback: explosion_knockback(
            input.center,
            input.knockback_origin,
            impact,
            input.knockback_resistance,
        ),
    })
}
