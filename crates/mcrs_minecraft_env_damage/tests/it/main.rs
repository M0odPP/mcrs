use core::hint::black_box;
use mcrs_minecraft_env_damage as env_damage;
use mcrs_minecraft_env_damage::{
    AxisAlignedBounds, BreathableMedium, ContactHazard, FallBlockKind, FallResetMode,
    FallStepInput, FireImmunity, FireSource, FreezeImmunity, GroundState, OnFireState,
    PLAYER_FIRE_GRACE, PLAYER_MAX_AIR, PowderSnowExposure, SuffocationState, WetExposure,
    WorldBorderBounds, WorldBorderDamageConfig,
};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn env_damage_rules_allocate_zero_bytes() {
    let border = WorldBorderBounds {
        west: -9.5,
        north: -9.5,
        east: 10.5,
        south: 10.5,
    };
    let bounds = AxisAlignedBounds::from_feet(20.5, 64.0, 0.5, 0.6, 1.8);
    let config = WorldBorderDamageConfig::default();
    let input = FallStepInput {
        fall_distance: 9.0,
        delta_y: -1.0,
        ground: GroundState::OnGround,
        reset: FallResetMode::Active,
    };

    let before = allocations();
    let step = env_damage::step_player_fall(black_box(input));
    let landed = black_box(step.landed_distance.unwrap_or(0.0));
    let fall = env_damage::compute_fall_outcome(landed, FallBlockKind::Default, 3.0, 1.0);
    let points = env_damage::fall_damage(black_box(10.0), 3.0, 1.0, 1.0);
    let eye = env_damage::suffocation_eye_box(black_box(0.5), 65.62, 0.5, 0.6);
    let cactus = env_damage::contact_damage(black_box(ContactHazard::Cactus));
    let void = env_damage::void_damage(black_box(-129.0), -64);
    let clear = SuffocationState::Clear;
    let border_hit = env_damage::border_damage(black_box(20.5), 0.5, bounds, border, config, clear);
    let (dry, vulnerable) = (WetExposure::Dry, FireImmunity::Vulnerable);
    let fire = env_damage::step_fire(
        black_box(40),
        PLAYER_FIRE_GRACE,
        FireSource::InFire,
        dry,
        vulnerable,
    );
    let air = env_damage::step_air(black_box(-19), PLAYER_MAX_AIR, BreathableMedium::Underwater);
    let (in_snow, normal) = (PowderSnowExposure::Inside, FreezeImmunity::Normal);
    let freeze =
        env_damage::step_freeze(black_box(139), in_snow, OnFireState::NotBurning, 40, normal);
    let speed = env_damage::powder_snow_speed_modifier(black_box(70));
    assert_eq!(allocations(), before, "pure rules allocate zero bytes");

    let ok = fall.damage == Some(7.0)
        && points == 7
        && cactus == Some(1.0)
        && void == Some(4.0)
        && border_hit == Some(1.0)
        && eye.has_positive_volume()
        && fire.burning_damage == Some(1.0)
        && air.damage == Some(2.0)
        && freeze.damage == Some(1.0)
        && speed < 0.0;
    assert!(ok, "all pure rule outcomes match expected values");
}
