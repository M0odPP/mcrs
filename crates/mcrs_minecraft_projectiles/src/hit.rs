use glam::DVec3;
use mcrs_minecraft_entity_key::EntityId;

use crate::{
    PickupPolicy, ProjectileAabb, ProjectileKind, ProjectileRandom, ProjectileState, finite_f32,
    has_layout, step_rotation,
};

pub const ENDER_PEARL_FALL_DAMAGE: f32 = 5.0;
pub const POTION_SPLASH_RADIUS: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CollectorAccess {
    SurvivalOwner,
    SurvivalOther,
    Creative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArrowHitOutcome {
    Pierced,
    StoppedInBody,
    StoppedConsumed,
    Bounced,
    BouncedAndDropped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TridentReturnOutcome {
    InFlight,
    ReturnedToOwner,
    DroppedItem,
    Discarded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PickupOutcome {
    None,
    GiveItem,
    CreativeDestroy,
}

#[must_use]
pub fn arrow_damage(state: &ProjectileState, random: &mut (impl ProjectileRandom + ?Sized)) -> f32 {
    let raw = f64::from(state.velocity.length() as f32) * state.base_damage;
    if !state.velocity.is_finite() || !state.base_damage.is_finite() || !raw.is_finite() {
        return 0.0;
    }
    let mut amount = libm::ceil(raw.clamp(0.0, f64::from(i32::MAX))) as i32;
    if state.critical {
        let bound = (amount / 2).saturating_add(2);
        amount = amount.saturating_add(random.next_i32_bound(bound));
    }
    amount as f32
}

pub fn bounce_arrow(
    state: &mut ProjectileState,
    random: &mut (impl ProjectileRandom + ?Sized),
) -> ArrowHitOutcome {
    use ProjectileKind::{Arrow, SpectralArrow};
    if !state.velocity.is_finite() {
        state.velocity = DVec3::ZERO;
    }
    state.velocity *= -0.1;
    state.yaw = finite_f32(state.yaw) + 170.0 + random.next_f32() * 20.0;
    if state.velocity.length_squared() < 1.0E-7 {
        state.removed = true;
        if state.pickup == PickupPolicy::Allowed && matches!(state.kind, Arrow | SpectralArrow) {
            return ArrowHitOutcome::BouncedAndDropped;
        }
    }
    ArrowHitOutcome::Bounced
}

pub fn resolve_arrow_entity_hit(
    state: &mut ProjectileState,
    target_id: EntityId,
) -> ArrowHitOutcome {
    if state.pierce_level > 0 {
        if state.pierced.contains(&Some(target_id)) {
            return ArrowHitOutcome::Pierced;
        }
        if let Some(slot) = state.pierced.get_mut(usize::from(state.pierced_count)) {
            *slot = Some(target_id);
            state.pierced_count = state.pierced_count.saturating_add(1);
            if state.pierced_count <= state.pierce_level {
                return ArrowHitOutcome::Pierced;
            }
        }
        state.removed = true;
        return ArrowHitOutcome::StoppedConsumed;
    }
    state.removed = true;
    ArrowHitOutcome::StoppedInBody
}

#[must_use]
pub fn potion_splash_scale(potion: DVec3, target: DVec3) -> Option<f64> {
    if !potion.is_finite() || !target.is_finite() {
        return None;
    }
    let distance_squared = potion.distance_squared(target);
    let limit = POTION_SPLASH_RADIUS * POTION_SPLASH_RADIUS;
    if !distance_squared.is_finite() || distance_squared >= limit {
        return None;
    }
    Some((1.0 - libm::sqrt(distance_squared) / POTION_SPLASH_RADIUS).clamp(0.0, 1.0))
}

pub fn resolve_trident_entity_hit(state: &mut ProjectileState, impaling_bonus: f32) -> f32 {
    if !state.velocity.is_finite() {
        state.velocity = DVec3::ZERO;
    }
    state.velocity *= DVec3::new(-0.01, -0.1, -0.01);
    state.returning = state.loyalty_level > 0;
    let base = finite_f32(state.base_damage.clamp(0.0, f64::from(f32::MAX)) as f32);
    let bonus = finite_f32(impaling_bonus).max(0.0);
    base + bonus
}

pub fn step_trident_loyalty(
    state: &mut ProjectileState,
    owner_eye: Option<DVec3>,
    owner_box: Option<&ProjectileAabb>,
) -> TridentReturnOutcome {
    if state.loyalty_level == 0 {
        return TridentReturnOutcome::InFlight;
    }
    let Some(eye) = owner_eye else {
        (state.returning, state.removed) = (false, true);
        return match state.pickup {
            PickupPolicy::Allowed => TridentReturnOutcome::DroppedItem,
            _ => TridentReturnOutcome::Discarded,
        };
    };
    if !eye.is_finite() || !state.position.is_finite() || !state.velocity.is_finite() {
        return TridentReturnOutcome::InFlight;
    }
    let delta = eye - state.position;
    if !delta.is_finite() {
        return TridentReturnOutcome::InFlight;
    }
    (state.in_ground, state.returning) = (false, true);
    let loyalty = f64::from(state.loyalty_level);
    state.position.y += delta.y * 0.015 * loyalty;
    state.velocity = state.velocity * 0.95 + delta.normalize_or_zero() * (0.05 * loyalty);
    state.position += state.velocity;
    let angles = step_rotation(state.yaw, state.pitch, state.velocity);
    (state.yaw, state.pitch) = (angles.yaw, angles.pitch);
    if owner_box.is_some_and(|pickup| state.bounds().intersects(pickup)) {
        state.removed = true;
        return TridentReturnOutcome::ReturnedToOwner;
    }
    TridentReturnOutcome::InFlight
}

#[must_use]
pub fn can_pickup_arrow(
    state: &ProjectileState,
    collector: &ProjectileAabb,
    access: CollectorAccess,
) -> PickupOutcome {
    let blocked_trident =
        state.kind == ProjectileKind::Trident && access == CollectorAccess::SurvivalOther;
    let unusable = state.removed || !state.in_ground || state.shake_ticks > 0 || blocked_trident;
    if unusable || !state.bounds().intersects(collector) {
        return PickupOutcome::None;
    }
    match (state.pickup, access) {
        (PickupPolicy::Allowed, _) => PickupOutcome::GiveItem,
        (PickupPolicy::CreativeOnly, CollectorAccess::Creative) => PickupOutcome::CreativeDestroy,
        _ => PickupOutcome::None,
    }
}

const _: () = assert!(
    has_layout::<CollectorAccess>(1, 1)
        && has_layout::<ArrowHitOutcome>(1, 1)
        && has_layout::<TridentReturnOutcome>(1, 1)
        && has_layout::<PickupOutcome>(1, 1),
    "Hit outcome enums must remain single-byte one-aligned values"
);
