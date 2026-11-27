use glam::DVec3;
use mcrs_minecraft_entity_key::EntityId;

use crate::{
    ProjectileKind, ProjectileRandom, ProjectileState, finite_dvec3, finite_f32, has_layout,
};

pub const TRIDENT_MIN_USE_TICKS: u32 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShooterFooting {
    #[default]
    Grounded,
    Airborne,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct LaunchProfile {
    pub speed: f32,
    pub inaccuracy: f32,
    pub pitch_offset: f32,
    pub critical: bool,
}

const fn preset(speed: f32, pitch_offset: f32) -> LaunchProfile {
    LaunchProfile {
        speed,
        inaccuracy: 1.0,
        pitch_offset,
        critical: false,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RotationAngles {
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct LaunchInput {
    pub kind: ProjectileKind,
    pub shooter_position: DVec3,
    pub shooter_eye_height: f32,
    pub shooter_yaw: f32,
    pub shooter_pitch: f32,
    pub shooter_velocity: DVec3,
    pub footing: ShooterFooting,
    pub owner: Option<EntityId>,
    pub profile: LaunchProfile,
}

impl LaunchInput {
    #[must_use]
    pub fn new(kind: ProjectileKind, shooter_position: DVec3, profile: LaunchProfile) -> Self {
        let mut input = Self::default();
        (input.kind, input.shooter_position, input.profile) = (kind, shooter_position, profile);
        input
    }
}

#[must_use]
pub fn bow_launch(draw_ticks: u32) -> Option<LaunchProfile> {
    let charge = draw_ticks as f32 / 20.0;
    let power = ((charge * charge + charge * 2.0) / 3.0).min(1.0);
    let mut profile = preset(power * 3.0, 0.0);
    profile.critical = power == 1.0;
    (power >= 0.1).then_some(profile)
}

#[must_use]
pub const fn trident_launch(draw_ticks: u32) -> Option<LaunchProfile> {
    if draw_ticks >= TRIDENT_MIN_USE_TICKS {
        return Some(preset(2.5, 0.0));
    }
    None
}

#[must_use]
pub const fn thrown_launch(kind: ProjectileKind) -> LaunchProfile {
    use ProjectileKind::{Arrow, LingeringPotion, SpectralArrow, SplashPotion, Trident};
    match kind {
        SplashPotion | LingeringPotion => preset(0.5, -20.0),
        Trident => preset(2.5, 0.0),
        Arrow | SpectralArrow => preset(3.0, 0.0),
        _ => preset(1.5, 0.0),
    }
}

#[must_use]
pub fn rotation_from_velocity(velocity: DVec3) -> RotationAngles {
    if !velocity.is_finite() || velocity == DVec3::ZERO {
        return RotationAngles::default();
    }
    let horizontal = libm::hypot(velocity.x, velocity.z);
    let yaw = finite_f32(libm::atan2(velocity.x, velocity.z).to_degrees() as f32);
    let pitch = finite_f32(libm::atan2(velocity.y, horizontal).to_degrees() as f32);
    RotationAngles { yaw, pitch }
}

fn wrap_degrees(delta: f32) -> f32 {
    let mut wrapped = delta % 360.0;
    if wrapped >= 180.0 {
        wrapped -= 360.0;
    } else if wrapped < -180.0 {
        wrapped += 360.0;
    }
    finite_f32(wrapped)
}

#[must_use]
pub fn step_rotation(yaw: f32, pitch: f32, velocity: DVec3) -> RotationAngles {
    let (yaw, pitch) = (finite_f32(yaw), finite_f32(pitch));
    if !velocity.is_finite() || velocity == DVec3::ZERO {
        return RotationAngles { yaw, pitch };
    }
    let target = rotation_from_velocity(velocity);
    RotationAngles {
        yaw: finite_f32(yaw + 0.2 * wrap_degrees(target.yaw - yaw)),
        pitch: finite_f32(pitch + 0.2 * wrap_degrees(target.pitch - pitch)),
    }
}

#[must_use]
pub fn launch_projectile(
    input: &LaunchInput,
    random: &mut (impl ProjectileRandom + ?Sized),
) -> ProjectileState {
    let yaw = finite_f32(input.shooter_yaw).to_radians();
    let pitch_deg = finite_f32(input.shooter_pitch) + finite_f32(input.profile.pitch_offset);
    let pitch = finite_f32(pitch_deg).to_radians();
    let (sin_yaw, cos_yaw) = (libm::sinf(yaw), libm::cosf(yaw));
    let (sin_pitch, cos_pitch) = (libm::sinf(pitch), libm::cosf(pitch));
    let (raw_x, raw_y, raw_z) = (-sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch);
    let raw = DVec3::new(f64::from(raw_x), f64::from(raw_y), f64::from(raw_z)).normalize_or_zero();
    let spread = 0.017_227_5_f64 * f64::from(finite_f32(input.profile.inaccuracy).max(0.0));
    let mut sample = || (random.next_f64() - random.next_f64()) * spread;
    let speed = f64::from(finite_f32(input.profile.speed));
    let base = finite_dvec3((raw + DVec3::new(sample(), sample(), sample())) * speed);
    let angles = rotation_from_velocity(base);
    let mut inherited = finite_dvec3(input.shooter_velocity);
    if input.footing == ShooterFooting::Grounded {
        inherited.y = 0.0;
    }
    let eye = f64::from(finite_f32(input.shooter_eye_height)) - f64::from(0.1_f32);
    let spawn = finite_dvec3(finite_dvec3(input.shooter_position) + DVec3::Y * eye);
    let velocity = finite_dvec3(base + inherited);
    let mut state = ProjectileState::new(input.kind, spawn, velocity, input.owner);
    (state.yaw, state.pitch, state.critical) = (angles.yaw, angles.pitch, input.profile.critical);
    state
}

const _: () = assert!(
    has_layout::<LaunchProfile>(16, 4)
        && has_layout::<RotationAngles>(8, 4)
        && has_layout::<LaunchInput>(96, 8),
    "launch types must keep expected size and alignment"
);
