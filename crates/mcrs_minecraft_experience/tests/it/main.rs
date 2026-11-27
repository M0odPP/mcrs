use core::hint::black_box;

use glam::DVec3;
use mcrs_minecraft_entity_key::EntityUuid;
use mcrs_minecraft_experience::{
    ExperienceOrb, ExperienceState, ExperienceTracker, ORB_AIR_DRAG, ORB_LIFETIME, ORB_VALUE_TIERS,
    OrbAttractionTarget, OrbContact, OrbLifecycle, OrbMedium, OrbPickupOutcome, OrbStepInput,
    OrbTargetStatus, collect_one, death_drop_points, largest_orb_value,
    level_and_progress_for_total_xp, merge_orbs, split_orb_values, step_orb, total_xp_at,
    total_xp_for_level, xp_needed_for_next_level,
};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn target(id: u128, x: f64, status: OrbTargetStatus) -> OrbAttractionTarget {
    OrbAttractionTarget {
        id: EntityUuid::from_u128(id),
        position: DVec3::new(x, 0.0, 0.0),
        eye_height: 1.62,
        status,
    }
}

#[test]
fn level_formulas_and_conversions() {
    for (level, expected) in [(-3, 7), (0, 7), (1, 9), (15, 37), (30, 112), (50, 292)] {
        assert_eq!(xp_needed_for_next_level(level), expected);
    }
    assert_eq!(xp_needed_for_next_level(i32::MAX), i32::MAX);
    let totals = [(0, 0_i64), (1, 7), (15, 315), (30, 1395), (100, 30970)];
    for (level, expected) in totals {
        assert_eq!(total_xp_for_level(level), expected);
        let mapped = level_and_progress_for_total_xp(expected as i32);
        assert_eq!((mapped.level, mapped.progress), (level, 0.0));
    }
    let at_ten = level_and_progress_for_total_xp(10);
    assert_eq!(at_ten.level, 1);
    assert!((at_ten.progress - 3.0 / 9.0).abs() < 1.0e-7, "fraction");
    assert_eq!(total_xp_at(1, 3.0 / 9.0), 10);
    let restored = ExperienceState::from_total_points(10, 42, 777);
    assert_eq!((restored.level, restored.total), (1, 10));
    assert_eq!((restored.score, restored.xp_seed), (42, 777));
}

#[test]
fn add_points_spend_levels_and_death_drop() {
    let mut state = ExperienceState::with_seed(12345);
    assert!(state.add_points(0).needs_sync, "zero points marks sync");
    assert!(state.add_points(7).needs_sync, "level 1");
    assert_eq!(state.level, 1);
    assert_eq!(state.total, 7);
    assert_eq!(state.score, 7);
    assert!(state.add_points(350).leveled_up, "multi-level jump");
    assert_eq!((state.level, state.total), (16, 357));

    assert!(state.spend_levels(4).needs_sync, "spend 4");
    assert_eq!((state.level, state.total), (12, 357));
    assert!(!state.spend_levels(0).needs_sync, "zero spend is no-op");
    assert!(state.spend_levels(20).needs_sync, "overspend resets");
    assert_eq!((state.level, state.total), (0, 0));

    state.add_points(10);
    state.add_points(-5);
    assert_eq!((state.level, state.total), (0, 5));
    assert!((state.progress - 5.0 / 7.0).abs() < 1.0e-6, "fraction");
    state.add_points(-100);
    assert_eq!(state.progress, 0.0);

    let mut capped = ExperienceState::with_seed(0);
    capped.total = i32::MAX - 5;
    capped.add_points(100);
    assert_eq!(capped.total, i32::MAX);
    assert_eq!((death_drop_points(10), death_drop_points(50)), (70, 100));
    assert_eq!(state.death_drop_points(), 0);
    assert_eq!(state.reset_on_death(), ExperienceState::with_seed(12345));
}

#[test]
fn orb_split_decomposes_using_tier_thresholds() {
    assert_eq!((largest_orb_value(0), split_orb_values(-5).count()), (0, 0));
    let five: Vec<i32> = split_orb_values(5).collect();
    assert_eq!(five, [3, 1, 1]);
    for amount in [1, 2, 5, 10, 99, 5000] {
        let parts: Vec<i32> = split_orb_values(amount).collect();
        assert_eq!(parts.iter().sum::<i32>(), amount);
        let valid = parts.iter().all(|part| ORB_VALUE_TIERS.contains(part));
        assert!(valid, "tier");
    }
}

#[test]
fn orb_step_attraction_merge_pickup_and_lifetime() {
    let mut orb = ExperienceOrb::new(7, 45);
    assert_eq!(orb.merge_group, 5);
    let mut input = OrbStepInput::default();
    let air = step_orb(&mut orb, input);
    assert!(air.merge_scan_due, "tick 1 merge scan");
    assert!((air.position.y + 0.03).abs() < 1.0e-12, "gravity");
    let drag_error = air.velocity.y + 0.03 * f64::from(ORB_AIR_DRAG);
    assert!(drag_error.abs() < 1.0e-12, "drag");

    input.velocity = DVec3::X;
    input.medium = OrbMedium::EyeInWater;
    let mut water = ExperienceOrb::new(3, 1);
    assert!(step_orb(&mut water, input).velocity.y > 0.0, "buoyancy");

    input.velocity = DVec3::ZERO;
    input.medium = OrbMedium::InLava {
        random_x: 0.1,
        random_z: -0.1,
    };
    let mut lava = ExperienceOrb::new(3, 1);
    assert!(step_orb(&mut lava, input).needs_sync, "lava sync");

    input.medium = OrbMedium::Air;
    input.velocity = DVec3::new(0.0, -0.5, 0.0);
    input.contact = OrbContact::Landed { friction: 0.6 };
    let mut bounce = ExperienceOrb::new(3, 1);
    let bounce_y = step_orb(&mut bounce, input).velocity.y;
    assert!((bounce_y - 0.212).abs() < 1.0e-9, "bounce");

    let targets = [
        target(11, 2.0, OrbTargetStatus::DyingOrDead),
        target(10, 4.0, OrbTargetStatus::Active),
    ];
    let mut attracted = ExperienceOrb::new(3, 1);
    input.velocity = DVec3::ZERO;
    input.contact = OrbContact::Airborne;
    input.targets = &targets;
    assert_eq!(step_orb(&mut attracted, input).velocity.x, 0.0);
    assert!(attracted.following_player.is_none(), "dead blocks lock");
    input.targets = &targets[1..];
    let pulled = step_orb(&mut attracted, input);
    assert_eq!(attracted.following_player, Some(targets[1].id));
    assert!(pulled.velocity.x > 0.0, "attraction velocity");

    let mut source = ExperienceOrb::new(3, 41);
    source.age = 10;
    source.count = 2;
    assert!(merge_orbs(&mut attracted, &mut source), "merge");
    assert_eq!((attracted.count, source.is_alive()), (3, false));

    let (mut player, mut tracker) = (ExperienceState::default(), ExperienceTracker::new());
    let picked = collect_one(&mut attracted, &mut player, &mut tracker);
    assert_ne!(picked, OrbPickupOutcome::Blocked);
    assert_eq!((player.total, attracted.count), (3, 2));
    let blocked = collect_one(&mut attracted, &mut player, &mut tracker);
    assert_eq!(blocked, OrbPickupOutcome::Blocked);
    tracker.tick_take_delay();
    tracker.tick_take_delay();
    assert!(tracker.can_take_orb(), "cooldown clears");

    let mut aged = ExperienceOrb::new(1, 0);
    aged.age = ORB_LIFETIME - 1;
    let expired = step_orb(&mut aged, OrbStepInput::default()).lifecycle;
    assert_eq!(expired, OrbLifecycle::Despawn);
}

#[test]
fn hot_path_rules_allocate_zero_bytes() {
    let targets = [target(1, 3.0, OrbTargetStatus::Active)];
    let (mut state, mut tracker) = (ExperienceState::with_seed(99), ExperienceTracker::new());
    let (mut orb_a, mut orb_b) = (ExperienceOrb::new(7, 5), ExperienceOrb::new(7, 45));
    let mut input = OrbStepInput::default();

    let before = allocations();
    let _ = black_box(xp_needed_for_next_level(black_box(35)));
    let _ = black_box(total_xp_for_level(black_box(35)));
    let _ = black_box(level_and_progress_for_total_xp(black_box(1395)));
    tracker.apply_update(state.add_points(black_box(250)));
    let _ = (state.spend_levels(black_box(2)), state.death_drop_points());
    let sum: i32 = split_orb_values(black_box(1500)).sum();
    let _ = step_orb(&mut orb_a, input);
    input.targets = &targets;
    let _ = step_orb(&mut orb_a, input);
    let _ = merge_orbs(&mut orb_a, &mut orb_b);
    let _ = collect_one(&mut orb_a, &mut state, &mut tracker);
    tracker.tick_take_delay();
    assert_eq!(allocations() - before, 0, "hot paths must not allocate");
    assert_eq!(sum, 1500);
}
