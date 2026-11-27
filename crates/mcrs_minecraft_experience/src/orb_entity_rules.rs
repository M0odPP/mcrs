use core::mem::{align_of, size_of};

use glam::DVec3;
use mcrs_minecraft_entity_key::EntityUuid;

use crate::constants::{
    ORB_AIR_DRAG, ORB_ATTRACTION_ACCELERATION, ORB_ATTRACTION_RANGE, ORB_ATTRACTION_RANGE_SQUARED,
    ORB_BOUNCE_FACTOR, ORB_BOUNCE_MIN_Y_VELOCITY, ORB_GRAVITY, ORB_LAVA_Y_VELOCITY,
    ORB_MERGE_SCAN_INTERVAL, ORB_WATER_BUOYANCY, ORB_WATER_DRAG_XZ, ORB_WATER_MAX_Y_VELOCITY,
};
use crate::experience_orb::{ExperienceOrb, OrbLifecycle};
use crate::experience_state::ExperienceState;
use crate::experience_tracker::ExperienceTracker;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum OrbMedium {
    #[default]
    Air,
    InSolidBlock,
    EyeInWater,
    InLava {
        random_x: f32,
        random_z: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum OrbContact {
    #[default]
    Airborne,
    Grounded {
        friction: f32,
    },
    Landed {
        friction: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbTargetStatus {
    Active,
    DyingOrDead,
    Spectator,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbAttractionTarget {
    pub id: EntityUuid,
    pub position: DVec3,
    pub eye_height: f32,
    pub status: OrbTargetStatus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OrbStepInput<'a> {
    pub position: DVec3,
    pub velocity: DVec3,
    pub medium: OrbMedium,
    pub contact: OrbContact,
    pub targets: &'a [OrbAttractionTarget],
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct OrbStepOutcome {
    pub position: DVec3,
    pub velocity: DVec3,
    pub lifecycle: OrbLifecycle,
    pub merge_scan_due: bool,
    pub needs_sync: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[must_use]
pub enum OrbPickupOutcome {
    Blocked,
    PickedUp {
        awarded_points: i32,
        orb_lifecycle: OrbLifecycle,
    },
}

#[must_use]
pub const fn merge_due(tick_count: u32) -> bool {
    tick_count % ORB_MERGE_SCAN_INTERVAL == 1
}

#[must_use]
pub const fn can_merge_orbs(first: &ExperienceOrb, second: &ExperienceOrb) -> bool {
    first.is_alive()
        && second.is_alive()
        && first.value == second.value
        && first.merge_group == second.merge_group
}

pub fn merge_orbs(target: &mut ExperienceOrb, source: &mut ExperienceOrb) -> bool {
    if !can_merge_orbs(target, source) {
        return false;
    }
    target.count = target.count.saturating_add(source.count);
    target.age = target.age.min(source.age);
    source.count = 0;
    true
}

#[must_use]
pub fn select_followed_player(
    current: Option<EntityUuid>,
    orb_position: DVec3,
    targets: &[OrbAttractionTarget],
) -> Option<OrbAttractionTarget> {
    if let Some(id) = current
        && let Some(kept) = targets.iter().copied().find(|target| {
            let within =
                target.position.distance_squared(orb_position) <= ORB_ATTRACTION_RANGE_SQUARED;
            target.id == id && target.status != OrbTargetStatus::Spectator && within
        })
    {
        return Some(kept);
    }
    let mut nearest: Option<(f64, OrbAttractionTarget)> = None;
    for &target in targets {
        if target.status == OrbTargetStatus::Spectator {
            continue;
        }
        let distance_squared = target.position.distance_squared(orb_position);
        if distance_squared >= ORB_ATTRACTION_RANGE_SQUARED {
            continue;
        }
        let closer = match nearest {
            None => true,
            Some((best, prior)) => {
                distance_squared < best || (distance_squared == best && target.id < prior.id)
            }
        };
        if closer {
            nearest = Some((distance_squared, target));
        }
    }
    nearest.and_then(|(_, found)| (found.status == OrbTargetStatus::Active).then_some(found))
}

#[must_use]
pub fn attraction_acceleration(orb_position: DVec3, target: &OrbAttractionTarget) -> DVec3 {
    let center_y = target.position.y + f64::from(target.eye_height) * 0.5;
    let delta = DVec3::new(target.position.x, center_y, target.position.z) - orb_position;
    let distance_squared = delta.length_squared();
    if distance_squared <= 0.0 || distance_squared >= ORB_ATTRACTION_RANGE_SQUARED {
        return DVec3::ZERO;
    }
    let distance = libm::sqrt(distance_squared);
    let scale = 1.0 - distance / ORB_ATTRACTION_RANGE;
    (delta / distance) * (scale * scale * ORB_ATTRACTION_ACCELERATION)
}

#[must_use = "apply updated orb position, velocity, and lifecycle"]
pub fn step_orb(orb: &mut ExperienceOrb, input: OrbStepInput<'_>) -> OrbStepOutcome {
    orb.tick_count = orb.tick_count.wrapping_add(1);
    let mut velocity = input.velocity;
    let mut needs_sync = false;
    match input.medium {
        OrbMedium::Air => velocity.y -= ORB_GRAVITY,
        OrbMedium::InSolidBlock => {}
        OrbMedium::EyeInWater => {
            velocity.x *= f64::from(ORB_WATER_DRAG_XZ);
            velocity.z *= f64::from(ORB_WATER_DRAG_XZ);
            let boosted = velocity.y + f64::from(ORB_WATER_BUOYANCY);
            velocity.y = boosted.min(f64::from(ORB_WATER_MAX_Y_VELOCITY));
        }
        OrbMedium::InLava { random_x, random_z } => {
            let lava_y = f64::from(ORB_LAVA_Y_VELOCITY);
            velocity = DVec3::new(f64::from(random_x), lava_y, f64::from(random_z));
            needs_sync = true;
        }
    }
    let followed = select_followed_player(orb.following_player, input.position, input.targets);
    orb.following_player = followed.map(|target| target.id);
    if let Some(target) = followed {
        velocity += attraction_acceleration(input.position, &target);
    }
    let previous_y = velocity.y;
    let position = input.position + velocity;
    let (drag, landed) = match input.contact {
        OrbContact::Airborne => (f64::from(ORB_AIR_DRAG), false),
        OrbContact::Grounded { friction } => (f64::from(ORB_AIR_DRAG * friction), false),
        OrbContact::Landed { friction } => (f64::from(ORB_AIR_DRAG * friction), true),
    };
    velocity *= drag;
    if landed && previous_y < ORB_BOUNCE_MIN_Y_VELOCITY {
        velocity.y = previous_y * ORB_BOUNCE_FACTOR;
    }
    orb.age = orb.age.saturating_add(1);
    let merge_scan_due = merge_due(orb.tick_count);
    OrbStepOutcome {
        position,
        velocity,
        lifecycle: orb.lifecycle(),
        merge_scan_due,
        needs_sync,
    }
}

#[must_use = "handle pickup outcome and orb lifecycle"]
pub fn collect_one(
    orb: &mut ExperienceOrb,
    player: &mut ExperienceState,
    tracker: &mut ExperienceTracker,
) -> OrbPickupOutcome {
    if !tracker.can_take_orb() || !orb.is_alive() {
        return OrbPickupOutcome::Blocked;
    }
    tracker.record_orb_taken();
    tracker.apply_update(player.add_points(orb.value));
    let orb_lifecycle = orb.take_one();
    OrbPickupOutcome::PickedUp {
        awarded_points: orb.value,
        orb_lifecycle,
    }
}

const _: () = {
    const BYTES: usize = 72 + size_of::<&[OrbAttractionTarget]>();
    let valid = size_of::<OrbTargetStatus>() == 1 && align_of::<OrbTargetStatus>() == 1;
    assert!(valid, "OrbTargetStatus layout");
    let valid = size_of::<OrbMedium>() == 12 && align_of::<OrbMedium>() == 4;
    assert!(valid, "OrbMedium layout");
    let valid = size_of::<OrbContact>() == 8 && align_of::<OrbContact>() == 4;
    assert!(valid, "OrbContact layout");
    let valid = size_of::<OrbPickupOutcome>() == 8 && align_of::<OrbPickupOutcome>() == 4;
    assert!(valid, "OrbPickupOutcome layout");
    let valid = size_of::<OrbAttractionTarget>() == 48 && align_of::<OrbAttractionTarget>() == 8;
    assert!(valid, "OrbAttractionTarget layout");
    let valid = size_of::<OrbStepInput<'_>>() == BYTES && align_of::<OrbStepInput<'_>>() == 8;
    assert!(valid, "OrbStepInput layout");
    let valid = size_of::<OrbStepOutcome>() == 56 && align_of::<OrbStepOutcome>() == 8;
    assert!(valid, "OrbStepOutcome layout");
};
