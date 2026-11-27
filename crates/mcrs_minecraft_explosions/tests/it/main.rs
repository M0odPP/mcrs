use glam::DVec3;
use mcrs_minecraft_explosions::{
    BlockInteraction, CHAIN_TNT_FUSE_MAX, CHAIN_TNT_FUSE_MIN, DEFAULT_TNT_FUSE, Difficulty,
    EntityBox, EntityExplosionInput, ExplosionRandom, ExplosionScratch, ExplosionWorld,
    TNT_GROUND_VELOCITY_FACTOR, TntFuseOutcome, TntGravityState, TntGroundState, chain_tnt_fuse,
    collect_affected_blocks, evaluate_entity_impact, scale_damage_for_difficulty, seen_fraction,
    step_tnt_fuse, step_tnt_velocity_after_move, step_tnt_velocity_before_move,
    tnt_explosion_center, tnt_spawn_position, tnt_spawn_velocity,
};
use mcrs_test_alloc::{CountingAllocator, allocations};
use mcrs_voxel::BlockPos;

#[global_allocator]
static TEST_ALLOCATOR: CountingAllocator = CountingAllocator;

struct TestRandom(f64, u32);
impl ExplosionRandom for TestRandom {
    fn next_f32(&mut self) -> f32 {
        self.0 as f32
    }
    fn next_f64(&mut self) -> f64 {
        self.0
    }
    fn next_u32_bounded(&mut self, bound: u32) -> u32 {
        self.1 % bound.max(1)
    }
}

struct FloorWorld(f32, f64, Option<i32>);
impl ExplosionWorld for FloorWorld {
    fn is_in_bounds(&self, position: BlockPos) -> bool {
        (-64..320).contains(&position.y)
    }
    fn block_and_fluid_resistance(&self, position: BlockPos) -> Option<f32> {
        if self.2 == Some(position.x) {
            return Some(f32::INFINITY);
        }
        if position.y == -64 {
            Some(3_600_000.0)
        } else if position.y <= -61 {
            Some(self.0)
        } else {
            None
        }
    }
    fn is_segment_occluded(&self, from: DVec3, to: DVec3) -> bool {
        self.1.is_finite() && (from.x - self.1) * (to.x - self.1) <= 0.0
    }
}

#[test]
fn raycast_deduplicates_respects_infinity_resistance_and_is_zero_alloc_on_reuse() {
    use BlockInteraction::{Destroy, DestroyWithDecay, None as Keep};
    let mut scratch = ExplosionScratch::new();
    assert_eq!(scratch.ray_count(), 1352);
    let (mut random, center) = (TestRandom(0.5, 0), DVec3::new(0.5, -59.93875, 0.5));
    let mut run = |target: &mut ExplosionScratch, origin, mode, resistance, barrier| {
        let world = FloorWorld(resistance, f64::NAN, barrier);
        collect_affected_blocks(origin, 4.0, mode, &world, &mut random, target).len()
    };

    let dirt_len = run(&mut scratch, center, DestroyWithDecay, 0.5, None);
    let hit_bedrock = scratch
        .affected_blocks()
        .iter()
        .any(|position| position.y <= -64);
    assert!(
        dirt_len > 300 && dirt_len < 1200 && !hit_bedrock,
        "dirt {dirt_len}"
    );
    let stone_len = run(&mut scratch, center, Destroy, 6.0, None);
    assert!(stone_len > 100 && stone_len < dirt_len, "stone >= dirt");
    assert!(
        run(&mut scratch, center, Destroy, -10.0, None) <= dirt_len * 2,
        "clamp"
    );
    run(&mut scratch, center, Destroy, 0.5, Some(1));
    let crossed_wall = scratch
        .affected_blocks()
        .iter()
        .any(|position| position.x >= 1);
    assert!(!crossed_wall, "+inf must stop rays");

    let alloc_before = allocations();
    for offset in 0..8 {
        let shifted = DVec3::new(f64::from(offset * 17) + 0.5, -59.93875, 2.5);
        assert_eq!(run(&mut scratch, shifted, Destroy, 0.5, None), dirt_len);
    }
    assert_eq!(allocations() - alloc_before, 0, "reuse must not allocate");
    assert_eq!(run(&mut scratch, center, Keep, 0.5, None), 0);
}

#[test]
fn exposure_damage_difficulty_and_knockback_match_spec_without_allocating() {
    use Difficulty::{Easy, Hard, Normal, Peaceful};
    let center = DVec3::new(1.5, -59.938_749_998_807_91, 2.5);
    let player_position = DVec3::new(-0.5, -60.0, 2.5);
    let eye_offset = DVec3::new(0.0, f64::from(1.62_f32), 0.0);
    let bounds = EntityBox::from_foot_position(player_position, 0.6, 1.8);
    let evaluate = |wall_x, position: DVec3, resistance| {
        let input = EntityExplosionInput {
            center,
            radius: 4.0,
            entity_position: position,
            knockback_origin: position + eye_offset,
            bounds,
            knockback_resistance: resistance,
        };
        evaluate_entity_impact(input, &FloorWorld(0.0, wall_x, None))
    };

    let alloc_before = allocations();
    let open = evaluate(f64::NAN, player_position, 0.0).expect("in reach");
    assert_eq!(allocations() - alloc_before, 0, "must not allocate");
    let expected = DVec3::new(
        f64::from_bits(0xbfe2_ed44_ef96_4111),
        f64::from_bits(0x3fdd_8090_421c_9ad6),
        0.0,
    );
    let open_ok = open.seen_fraction == 1.0 && open.knockback.distance(expected) < 1e-12;
    assert!(open_ok, "open explosion impact mismatch");

    let blocked = evaluate(0.5, player_position, 0.0).expect("occluded");
    let blocked_tuple = (blocked.seen_fraction, blocked.raw_damage, blocked.knockback);
    assert_eq!(blocked_tuple, (0.0, 1.0, DVec3::ZERO));
    let reduced = evaluate(f64::NAN, player_position, 0.5).expect("resistant");
    assert!(reduced.knockback.distance(expected * 0.5) < 1e-12, "half");
    let far = center + DVec3::new(8.01, 0.0, 0.0);
    assert!(evaluate(f64::NAN, far, 0.0).is_none(), "beyond diameter");

    let huge = EntityBox::from_foot_position(DVec3::ZERO, 1e18, 1.0);
    let huge_seen = seen_fraction(center, huge, &FloorWorld(0.0, f64::NAN, None));
    assert_eq!(huge_seen, 0.0);
    let scaled = [Peaceful, Easy, Normal, Hard].map(|d| scale_damage_for_difficulty(20.0, d));
    assert_eq!(scaled, [0.0, 11.0, 20.0, 30.0]);
    assert_eq!(scale_damage_for_difficulty(1.0, Easy), 1.0);
}

#[test]
fn tnt_constants_spawn_motion_and_fuse_match_spec() {
    use TntFuseOutcome::{Counting, Detonate};
    use TntGravityState::Enabled;
    use TntGroundState::{Airborne, OnGround};
    let constants = (DEFAULT_TNT_FUSE, CHAIN_TNT_FUSE_MIN, CHAIN_TNT_FUSE_MAX);
    assert_eq!(constants, (80, 10, 29));
    let spawn = tnt_spawn_position(BlockPos::new(1, -60, 2));
    assert_eq!(spawn, DVec3::new(1.5, -60.0, 2.5));
    assert_eq!(
        tnt_explosion_center(spawn).y.to_bits(),
        0xc04d_f828_f5c0_0000
    );
    let (mut low, mut high) = (TestRandom(0.0, 0), TestRandom(0.25, 19));
    assert_eq!(
        (chain_tnt_fuse(&mut low), chain_tnt_fuse(&mut high)),
        (10, 29)
    );
    let hop = tnt_spawn_velocity(&mut low);
    let hop_ok = hop.x.abs() < 1e-12 && hop.y == 0.2 && (hop.z + 0.02).abs() < 1e-12;
    assert!(hop_ok, "hop");
    let after_gravity = step_tnt_velocity_before_move(DVec3::new(0.1, 0.2, 0.0), Enabled);
    let drag = f64::from(0.98_f32);
    assert!((after_gravity.y - 0.16).abs() < 1e-12, "gravity");
    let step_after = step_tnt_velocity_after_move;
    assert_eq!(step_after(after_gravity, Airborne), after_gravity * drag);
    assert_eq!(
        step_after(after_gravity, OnGround),
        after_gravity * drag * TNT_GROUND_VELOCITY_FACTOR
    );
    let fuses = (step_tnt_fuse(2), step_tnt_fuse(1), step_tnt_fuse(0));
    assert_eq!(fuses, (Counting(1), Detonate, Detonate));
}
