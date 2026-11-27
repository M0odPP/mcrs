use core::hint::black_box;

use glam::DVec3;
use mcrs_minecraft_entity_key::EntityId;
use mcrs_minecraft_projectiles::{
    ARROW_DESPAWN_TICKS, ArrowHitOutcome, CollectorAccess, ENDER_PEARL_FALL_DAMAGE, EntityTarget,
    FlightCategory, LaunchInput, PickupOutcome, PickupPolicy, ProjectileAabb, ProjectileKind,
    ProjectileState, ProjectileWorld, StepOutcome, TridentReturnOutcome, arrow_damage,
    bounce_arrow, bow_launch, can_pickup_arrow, finish_arrow_flight_step, hit_margin,
    launch_projectile, potion_splash_scale, raycast_blocks, resolve_arrow_entity_hit,
    resolve_trident_entity_hit, step_projectile, step_trident_loyalty, sweep_entity_hit,
    thrown_launch, trident_launch,
};
use mcrs_minecraft_random::legacy::LegacyRandom;
use mcrs_minecraft_random::xoroshiro::XoroshiroRandom;
use mcrs_test_alloc::{CountingAllocator, allocations};
use mcrs_voxel::{BlockPos, BlockStateId};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

struct Wall(Option<BlockPos>);

impl ProjectileWorld for Wall {
    fn block_state(&self, position: BlockPos) -> BlockStateId {
        if self.0 == Some(position) {
            return BlockStateId::checked(1, 2).unwrap_or(BlockStateId::AIR);
        }
        BlockStateId::AIR
    }

    fn visit_block_boxes(&self, block: BlockPos, mut visit: impl FnMut(ProjectileAabb) -> bool) {
        if self.0 == Some(block) {
            let _ = visit(ProjectileAabb::new(DVec3::ZERO, DVec3::ONE));
        }
    }
}

fn target(id: u64, coord_x: f64, coord_z: f64, owner: bool) -> EntityTarget {
    let position = DVec3::new(coord_x, 64.0, coord_z);
    let bounds = ProjectileAabb::of_body(position, 0.6, 1.8);
    let mut entry = EntityTarget::new(EntityId(id), bounds, position);
    entry.in_owner_group = owner;
    entry
}

fn arrow(coord_x: f64, coord_y: f64, speed_x: f64) -> ProjectileState {
    let origin = DVec3::new(coord_x, coord_y, 0.5);
    ProjectileState::new(ProjectileKind::Arrow, origin, DVec3::X * speed_x, None)
}

fn step(state: &mut ProjectileState, world: &Wall, list: &[EntityTarget]) -> StepOutcome {
    step_projectile(state, world, list, &mut LegacyRandom::new(1))
}

#[test]
fn projectile_rules_and_zero_alloc() {
    let empty = Wall(None);
    verify_launch_and_specs();
    verify_flight_and_collision(&empty);
    verify_hit_and_pickup_rules(&empty);
    verify_zero_alloc_hot_paths(&empty);
}

fn verify_launch_and_specs() {
    use FlightCategory::{Arrow as ArrowFlight, Thrown as ThrownFlight};
    use ProjectileKind::{
        Arrow, Egg, EnderPearl, LingeringPotion, Snowball, SpectralArrow, SplashPotion, Trident,
    };
    let (arrow_spec, ball, potion) = (Arrow.spec(), Snowball.spec(), SplashPotion.spec());
    let spec_ok = arrow_spec == SpectralArrow.spec()
        && ball == Egg.spec()
        && ball == EnderPearl.spec()
        && potion == LingeringPotion.spec()
        && (arrow_spec.air_drag, arrow_spec.water_drag) == (0.99, 0.6)
        && arrow_spec.base_damage == 2.0
        && (ball.gravity, ball.width, ball.water_drag) == (0.03, 0.25, 0.8)
        && (arrow_spec.category, ball.category) == (ArrowFlight, ThrownFlight)
        && (potion.category, potion.gravity) == (ThrownFlight, 0.05)
        && (Trident.spec().water_drag, Trident.spec().base_damage) == (0.99, 8.0);
    let full = bow_launch(20).unwrap_or(thrown_launch(Arrow));
    let mut input = LaunchInput::new(Arrow, DVec3::new(0.0, 64.0, 0.0), full);
    (input.shooter_eye_height, input.shooter_velocity) = (1.62, DVec3::new(0.1, -0.5, 0.2));
    input.owner = Some(EntityId(1));
    let shot = launch_projectile(&input, &mut LegacyRandom::new(7));
    (input.kind, input.profile) = (EnderPearl, thrown_launch(EnderPearl));
    let pearl = launch_projectile(&input, &mut XoroshiroRandom::new(7));
    (input.shooter_yaw, input.shooter_position) = (f32::NAN, DVec3::splat(f64::INFINITY));
    let safe = launch_projectile(&input, &mut LegacyRandom::new(7));
    let eye = 64.0 + f64::from(1.62_f32) - f64::from(0.1_f32);
    let checks = [
        spec_ok && full.critical && !pearl.left_owner,
        bow_launch(2).is_none() && trident_launch(9).is_none() && trident_launch(10).is_some(),
        bow_launch(3).is_some_and(|profile| (profile.speed - 0.3225).abs() < 1e-4),
        (full.speed - 3.0).abs() < 1e-6 && thrown_launch(SplashPotion).pitch_offset == -20.0,
        (shot.position.y - eye).abs() < 1e-12 && (shot.velocity.z - 3.2).abs() < 0.2,
        safe.position.is_finite() && safe.velocity.is_finite(),
    ];
    assert_eq!(checks, [true; 6], "specs and launch rules");
}

fn verify_flight_and_collision(empty: &Wall) {
    use ProjectileKind::{Snowball, SpectralArrow};
    let mut world = Wall(Some(BlockPos::new(5, 65, 0)));
    let (owner, enemy) = (target(1, 0.0, 0.0, true), target(2, 2.5, 0.5, false));
    let mut owned = arrow(0.0, 65.0, 2.0);
    (owned.owner, owned.left_owner) = (Some(EntityId(1)), false);
    let near = step(&mut owned, &world, &[owner]) == StepOutcome::Flew && !owned.left_owner;
    let clear = step(&mut owned, &world, &[owner]) == StepOutcome::Flew && owned.left_owner;
    let mut snowball = arrow(1.0, 64.5, 2.0);
    (snowball.kind, snowball.age_ticks) = (Snowball, 7);
    let edge = enemy.bounds.min.x - f64::from(0.3_f32);
    let near_eq = |value: f64| (value - edge).abs() < 1e-12;
    let snow_ok = matches!(
        step(&mut snowball, empty, &[enemy]),
        StepOutcome::HitEntity { hit, next_position }
            if near_eq(hit.point.x) && near_eq(next_position.x)
    ) && near_eq(snowball.position.x);
    let mut graze = arrow(1.0, 64.5, 3.0);
    (graze.position.z, graze.age_ticks) = (-0.05, 7);
    let mut blocked = graze;
    let grazed = step(&mut graze, empty, &[enemy]);
    let (mut inside, mut stuck) = (arrow(5.5, 65.5, 0.0), arrow(4.5, 65.5, 1.0));
    inside.despawn_ticks = 99;
    let stuck_in = matches!(step(&mut inside, &world, &[]), StepOutcome::StuckInBlock(_));
    let stuck_wall = matches!(step(&mut stuck, &world, &[]), StepOutcome::StuckInBlock(_));
    let flags = stuck.in_ground
        && stuck.shake_ticks == 7
        && inside.shake_ticks == 0
        && inside.despawn_ticks == 0;
    (0..7).for_each(|_| _ = step(&mut stuck, &world, &[]));
    world.0 = None;
    let released = step(&mut stuck, &world, &[]) == StepOutcome::ReleasedFromGround;
    (stuck.in_ground, stuck.in_block_state) = (true, None);
    let saved_rel = step(&mut stuck, &world, &[]) == StepOutcome::ReleasedFromGround;
    (stuck.in_ground, stuck.in_block_state) = (true, Some(BlockStateId::AIR));
    stuck.despawn_ticks = ARROW_DESPAWN_TICKS - 1;
    let despawned = step(&mut stuck, &world, &[]) == StepOutcome::DespawnedInGround;
    world.0 = Some(BlockPos::new(2, 64, 0));
    let los_blocked = step(&mut blocked, &world, &[enemy]) == StepOutcome::Flew;
    let far_pos = BlockPos::new(1_000_000_001, 64, 0);
    let far_ray = DVec3::new(1e9, 64.5, 0.5);
    let far_hit = raycast_blocks(&Wall(Some(far_pos)), far_ray, far_ray + DVec3::X * 3.0);
    let clip = |min, max| ProjectileAabb::new(min, max).clip_segment(DVec3::ZERO, DVec3::ONE);
    let (start, end) = (DVec3::new(0.0, 65.0, 0.0), DVec3::new(4.0, 65.0, 0.0));
    let inv_bounds = ProjectileAabb::new(DVec3::new(1.6, 64.0, 0.0), DVec3::new(1.5, 65.8, 0.6));
    let inv_target = EntityTarget::new(EntityId(9), inv_bounds, DVec3::new(1.55, 64.0, 0.3));
    let inv_none = sweep_entity_hit(&snowball, empty, &[inv_target], start, end).is_none();
    let mut pierced = ProjectileState::new(SpectralArrow, DVec3::ZERO, DVec3::X, None);
    (
        pierced.pierce_level,
        pierced.pierced[0],
        pierced.pierced_count,
    ) = (1, Some(EntityId(10)), 1);
    let targets = [target(10, 1.5, 0.0, false), target(11, 2.5, 0.0, false)];
    let next_id = sweep_entity_hit(&pierced, empty, &targets, start, end).map(|hit| hit.entity_id);
    let checks = [
        near && clear && (owned.velocity.x - 1.9602).abs() < 1e-6,
        (hit_margin(2), hit_margin(8)) == (0.0, f64::from(0.3_f32)) && snow_ok && inv_none,
        matches!(grazed, StepOutcome::HitEntity { .. }) && los_blocked,
        stuck_in && stuck_wall && flags && released && stuck.shake_ticks == 0 && saved_rel,
        despawned && stuck.removed && far_hit.is_some_and(|hit| hit.block_position == far_pos),
        clip(DVec3::ONE, DVec3::ZERO).is_none() && clip(DVec3::NAN, DVec3::ONE).is_none(),
        raycast_blocks(empty, DVec3::ZERO, DVec3::MAX).is_none() && next_id == Some(EntityId(11)),
    ];
    assert_eq!(checks, [true; 7], "flight, dda, and collision rules");
}

fn verify_hit_and_pickup_rules(empty: &Wall) {
    use ProjectileKind::{SpectralArrow, Trident};
    let mut random = XoroshiroRandom::new(9);
    let mut damage = arrow(0.0, 0.0, 3.00000005);
    let base = arrow_damage(&damage, &mut random);
    damage.critical = true;
    let crit = arrow_damage(&damage, &mut random);
    damage.base_damage = f64::NAN;
    let nan_damage = arrow_damage(&damage, &mut random);
    let mut pierce = ProjectileState::new(SpectralArrow, DVec3::ZERO, DVec3::X, None);
    pierce.pierce_level = 1;
    let targets = [target(10, 1.5, 0.0, false), target(11, 2.5, 0.0, false)];
    let (start, end) = (DVec3::new(0.0, 65.0, 0.0), DVec3::new(4.0, 65.0, 0.0));
    let origin = DVec3::new(5.0, 64.0, 0.0);
    let mut trident = ProjectileState::new(Trident, origin + DVec3::X * 0.8, DVec3::ZERO, None);
    trident.loyalty_level = 3;
    let owner_box = ProjectileAabb::of_body(origin - DVec3::Y * 0.5, 1.0, 2.0);
    let mut drop_trident = ProjectileState::new(Trident, origin, DVec3::ZERO, None);
    (drop_trident.loyalty_level, drop_trident.pickup) = (1, PickupPolicy::Allowed);
    let mut pickup = arrow(5.0, 64.0, 0.0);
    (pickup.in_ground, pickup.pickup) = (true, PickupPolicy::CreativeOnly);
    let first_hit = resolve_arrow_entity_hit(&mut pierce, EntityId(10));
    let same_hit = resolve_arrow_entity_hit(&mut pierce, EntityId(10));
    let next_id = sweep_entity_hit(&pierce, empty, &targets, start, end).map(|hit| hit.entity_id);
    let second_hit = resolve_arrow_entity_hit(&mut pierce, EntityId(11));
    let mut single = arrow(0.0, 65.0, 1.0);
    let single_hit = resolve_arrow_entity_hit(&mut single, EntityId(10));
    let nan_loyalty = step_trident_loyalty(&mut trident, Some(DVec3::NAN), Some(&owner_box));
    let loyalty = step_trident_loyalty(&mut trident, Some(origin), Some(&owner_box));
    let dropped_loyalty = step_trident_loyalty(&mut drop_trident, None, None);
    let survival = can_pickup_arrow(&pickup, &owner_box, CollectorAccess::SurvivalOwner);
    let creative = can_pickup_arrow(&pickup, &owner_box, CollectorAccess::Creative);
    pickup.removed = true;
    let gone = can_pickup_arrow(&pickup, &owner_box, CollectorAccess::Creative);
    let mut trident_pickup = ProjectileState::new(Trident, origin, DVec3::ZERO, None);
    (trident_pickup.in_ground, trident_pickup.pickup) = (true, PickupPolicy::Allowed);
    let trident_other =
        can_pickup_arrow(&trident_pickup, &owner_box, CollectorAccess::SurvivalOther);
    let trident_owner =
        can_pickup_arrow(&trident_pickup, &owner_box, CollectorAccess::SurvivalOwner);
    let mut slow = arrow(0.0, 64.0, 0.001);
    slow.pickup = PickupPolicy::Allowed;
    let slow_drop = bounce_arrow(&mut slow, &mut random);
    let hit_checks = [
        base == 6.0 && (6.0..=10.0).contains(&crit) && nan_damage == 0.0,
        ENDER_PEARL_FALL_DAMAGE == 5.0 && nan_loyalty == TridentReturnOutcome::InFlight,
        loyalty == TridentReturnOutcome::ReturnedToOwner
            && trident.removed
            && dropped_loyalty == TridentReturnOutcome::DroppedItem
            && drop_trident.removed,
        resolve_trident_entity_hit(&mut trident, 2.5) == 10.5
            && resolve_trident_entity_hit(&mut trident, f32::NAN) == 8.0,
        first_hit == ArrowHitOutcome::Pierced
            && same_hit == ArrowHitOutcome::Pierced
            && next_id == Some(EntityId(11))
            && second_hit == ArrowHitOutcome::StoppedConsumed
            && pierce.removed
            && single_hit == ArrowHitOutcome::StoppedInBody
            && single.removed,
        potion_splash_scale(DVec3::ZERO, DVec3::X * 2.0) == Some(0.5)
            && potion_splash_scale(DVec3::ZERO, DVec3::X * 4.0).is_none()
            && potion_splash_scale(DVec3::NAN, DVec3::ZERO).is_none(),
        (survival, creative, gone)
            == (
                PickupOutcome::None,
                PickupOutcome::CreativeDestroy,
                PickupOutcome::None,
            ),
        (trident_other, trident_owner) == (PickupOutcome::None, PickupOutcome::GiveItem),
        slow_drop == ArrowHitOutcome::BouncedAndDropped && slow.removed,
    ];
    assert_eq!(hit_checks, [true; 9], "hit resolution and pickup rules");
}

fn verify_zero_alloc_hot_paths(empty: &Wall) {
    use ProjectileKind::Trident;
    let mut random = XoroshiroRandom::new(9);
    let targets = [target(10, 1.5, 0.0, false), target(11, 2.5, 0.0, false)];
    let end = DVec3::new(4.0, 65.0, 0.0);
    let origin = DVec3::new(5.0, 64.0, 0.0);
    let owner_box = ProjectileAabb::of_body(origin - DVec3::Y * 0.5, 1.0, 2.0);
    let mut trident_pickup = ProjectileState::new(Trident, origin, DVec3::ZERO, None);
    (trident_pickup.in_ground, trident_pickup.pickup) = (true, PickupPolicy::Allowed);
    let mut hot = arrow(0.0, 65.0, 4.0);
    let mut hot_trident = ProjectileState::new(Trident, origin + DVec3::X * 0.8, DVec3::X, None);
    hot_trident.loyalty_level = 2;
    hot.position.z = 0.0;
    let before = allocations();
    let outcome = black_box(step(&mut hot, empty, &targets));
    finish_arrow_flight_step(&mut hot, end);
    let damage_out = black_box(arrow_damage(&hot, &mut random));
    let resolved = black_box(resolve_arrow_entity_hit(&mut hot, EntityId(11)));
    let bounced = black_box(bounce_arrow(&mut hot, &mut random));
    let scale = black_box(potion_splash_scale(DVec3::ZERO, DVec3::X));
    let trident_damage = black_box(resolve_trident_entity_hit(&mut hot_trident, 1.0));
    let trident_return = black_box(step_trident_loyalty(
        &mut hot_trident,
        Some(origin),
        Some(&owner_box),
    ));
    let pickup_out = black_box(can_pickup_arrow(
        &trident_pickup,
        &owner_box,
        CollectorAccess::SurvivalOwner,
    ));
    let zero_ok = allocations() - before == 0
        && matches!(outcome, StepOutcome::HitEntity { .. })
        && damage_out > 0.0
        && resolved == ArrowHitOutcome::StoppedInBody
        && bounced == ArrowHitOutcome::Bounced
        && scale.is_some()
        && trident_damage == 9.0
        && trident_return == TridentReturnOutcome::ReturnedToOwner
        && pickup_out == PickupOutcome::GiveItem;
    assert!(zero_ok, "hit hot paths allocate nothing");
}
