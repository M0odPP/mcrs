use glam::DVec3;
use mcrs_voxel::BlockPos;

use crate::interaction::ExplosionRandom;

pub const DEFAULT_TNT_FUSE: i16 = 80;
pub const CHAIN_TNT_FUSE_MIN: i16 = 10;
pub const CHAIN_TNT_FUSE_MAX: i16 = 29;
pub const DEFAULT_TNT_POWER: f32 = 4.0;
pub const MAX_TNT_EXPLOSION_POWER: f32 = 128.0;
pub const TNT_WIDTH: f32 = 0.98;
pub const TNT_HEIGHT: f32 = 0.98;
pub const TNT_EYE_HEIGHT: f32 = 0.15;
pub const TNT_GRAVITY: f64 = 0.04;
pub const TNT_DRAG: f32 = 0.98;
pub const TNT_GROUND_VELOCITY_FACTOR: DVec3 = DVec3::new(0.7, -0.5, 0.7);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TntGravityState {
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TntGroundState {
    Airborne,
    OnGround,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TntFuseOutcome {
    Counting(i16),
    Detonate,
}

const _: () = {
    use core::mem::{align_of, size_of};
    assert!(
        size_of::<TntGravityState>() == 1 && align_of::<TntGravityState>() == 1,
        "TntGravityState layout"
    );
    assert!(
        size_of::<TntGroundState>() == 1 && align_of::<TntGroundState>() == 1,
        "TntGroundState layout"
    );
    assert!(
        size_of::<TntFuseOutcome>() == 4 && align_of::<TntFuseOutcome>() == 2,
        "TntFuseOutcome layout"
    );
};

#[must_use]
pub fn tnt_spawn_position(block_position: BlockPos) -> DVec3 {
    DVec3::new(
        f64::from(block_position.x) + 0.5,
        f64::from(block_position.y),
        f64::from(block_position.z) + 0.5,
    )
}

#[must_use]
pub fn tnt_explosion_center(entity_position: DVec3) -> DVec3 {
    DVec3::new(
        entity_position.x,
        entity_position.y + f64::from(TNT_HEIGHT) * 0.0625,
        entity_position.z,
    )
}

#[must_use]
pub fn tnt_spawn_velocity(random: &mut impl ExplosionRandom) -> DVec3 {
    let angle = random.next_f64() * (core::f64::consts::PI * 2.0);
    DVec3::new(-libm::sin(angle) * 0.02, 0.2, -libm::cos(angle) * 0.02)
}

#[must_use]
pub fn chain_tnt_fuse(random: &mut impl ExplosionRandom) -> i16 {
    let span = (CHAIN_TNT_FUSE_MAX - CHAIN_TNT_FUSE_MIN + 1) as u32;
    let offset = random.next_u32_bounded(span).min(span - 1);
    CHAIN_TNT_FUSE_MIN + (offset as i16)
}

#[must_use]
pub fn step_tnt_velocity_before_move(velocity: DVec3, gravity: TntGravityState) -> DVec3 {
    match gravity {
        TntGravityState::Disabled => velocity,
        TntGravityState::Enabled => DVec3::new(velocity.x, velocity.y - TNT_GRAVITY, velocity.z),
    }
}

#[must_use]
pub fn step_tnt_velocity_after_move(velocity: DVec3, ground: TntGroundState) -> DVec3 {
    let dragged = velocity * f64::from(TNT_DRAG);
    match ground {
        TntGroundState::Airborne => dragged,
        TntGroundState::OnGround => dragged * TNT_GROUND_VELOCITY_FACTOR,
    }
}

#[must_use]
pub const fn step_tnt_fuse(fuse: i16) -> TntFuseOutcome {
    let next_fuse = fuse.saturating_sub(1);
    if next_fuse <= 0 {
        TntFuseOutcome::Detonate
    } else {
        TntFuseOutcome::Counting(next_fuse)
    }
}
