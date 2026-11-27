use core::hint::black_box;
use mcrs_minecraft_beds::{
    BED_TOP_OFFSET, BedFacing, BedPair, BedPart, BedRuleCondition, BedState, BedUseOutcome,
    CLEAR_NIGHT_END_TICK, CLEAR_NIGHT_START_TICK, CreativeBypass, EXPLODING_BED_RULE, EntityAabb,
    FULL_RAIN_NIGHT_END_TICK, FULL_RAIN_NIGHT_START_TICK, OVERWORLD_BED_RULE,
    OVERWORLD_STRAW_BED_RULE, ObstructionState, STRAW_BED_TOP_OFFSET, SleepCheckInput,
    SleepExitReason, SleepRejection, SleepSkipInput, SleepSkipStats, WorldPoint, bed_in_reach,
    can_condition_pass, day_timeline_sky_factor, effective_sky_light_level, entity_blocks_sleep,
    evaluate_bed_use, evaluate_night_skip, evaluate_sleep_wake, explosion_origin,
    is_dark_for_sleep, is_within_bed_reach, next_morning_tick, required_sleeping_players,
    sleep_anchor, step_sleep_timer,
};
use mcrs_minecraft_pose::Pose;
use mcrs_test_alloc::{CountingAllocator, allocations};
use mcrs_voxel::BlockPos;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn point(x: f64, y: f64, z: f64) -> WorldPoint {
    WorldPoint { x, y, z }
}

fn sky_dark(tick: i64, rain: f32, thunder: f32) -> bool {
    let base = 15.0 * day_timeline_sky_factor(tick);
    is_dark_for_sleep(effective_sky_light_level(base, rain, thunder))
}

fn zombie_blocks(pair: BedPair, x: f64, y: f64, z: f64) -> bool {
    let monster = EntityAabb {
        min_x: x - 0.3,
        min_y: y,
        min_z: z - 0.3,
        max_x: x + 0.3,
        max_y: y + 1.95,
        max_z: z + 0.3,
    };
    entity_blocks_sleep(pair.head, monster)
}

fn check_facing(facing: BedFacing, foot_pos: BlockPos, head_pos: BlockPos) {
    let pair = BedPair::from_placement(foot_pos, facing);
    let foot = BedState::new(facing, BedPart::Foot);
    let head = BedState::new(facing, BedPart::Head);
    assert_eq!(pair.head, head_pos, "head offset from facing");
    assert_eq!(foot.partner_pos(foot_pos), head_pos, "foot partner");
    assert_eq!(head.partner_pos(head_pos), foot_pos, "head partner");
    assert_eq!(BedPair::from_block(head_pos, head), pair, "round trip");
    assert!(foot.is_expected_partner(head), "valid partner");
    assert!(!foot.is_expected_partner(foot), "self is not partner");
    let occupied = head.occupy();
    let vacated = occupied.vacate();
    assert!(occupied.occupied && !vacated.occupied, "occupied toggle");
}

#[derive(Clone, Copy)]
enum SpawnUpdate {
    Sets,
    Keeps,
}

fn assert_rejected(input: &SleepCheckInput, reason: SleepRejection, spawn: SpawnUpdate) {
    let sets_spawn = matches!(spawn, SpawnUpdate::Sets);
    let expected = BedUseOutcome::Rejected { reason, sets_spawn };
    assert_eq!(evaluate_bed_use(input), expected, "rejected");
}

fn assert_sky(rain: f32, thunder: f32, expected: f32, label: &str) {
    let sky = effective_sky_light_level(15.0, rain, thunder);
    assert!((sky - expected).abs() < 1.0e-5, "{label}");
}

fn skip_stats(
    total_players: u32,
    sleeping_players: u32,
    deep_sleeping_players: u32,
) -> SleepSkipStats {
    SleepSkipStats {
        total_players,
        sleeping_players,
        deep_sleeping_players,
    }
}

#[test]
fn bed_rules_allocate_zero_bytes() {
    let foot = BlockPos::new(0, -60, 2);
    let pair = BedPair::from_placement(foot, BedFacing::South);
    let mut input = SleepCheckInput::new(OVERWORLD_BED_RULE, 4.0);
    input.within_reach = bed_in_reach(point(0.5, -60.0, 1.0), pair);
    input.monster_nearby = zombie_blocks(pair, 12.0, -60.0, 3.5);
    let mut skip_input = SleepSkipInput::new(skip_stats(2, 2, 2), 100, 14_000);
    skip_input.raining_or_thundering = true;
    let before = allocations();
    let placed = BedPair::from_placement(black_box(foot), BedFacing::South);
    let anchor = sleep_anchor(placed.head, BED_TOP_OFFSET);
    let base = 15.0 * day_timeline_sky_factor(black_box(14_000));
    let sky = effective_sky_light_level(base, 0.0, 0.0);
    let outcome = evaluate_bed_use(black_box(&input));
    let timer = step_sleep_timer(black_box(99), 1);
    let stop = SleepExitReason::StopSleepingPacket;
    let wake = evaluate_sleep_wake(OVERWORLD_STRAW_BED_RULE, stop);
    let decision = evaluate_night_skip(black_box(&skip_input));
    assert_eq!(allocations(), before, "pure bed rules allocate zero bytes");
    assert_eq!(placed.head, BlockPos::new(0, -60, 3), "south head offset");
    assert_eq!(anchor.y, -59.3125, "standard bed anchor y");
    assert!(is_dark_for_sleep(sky), "night sky light allows sleep");
    assert_eq!(outcome, BedUseOutcome::SleepStarted, "eligible bed sleeps");
    assert!(timer.deep_sleep, "timer reaches deep sleep at 100 ticks");
    assert!(wake.destroy_bed, "straw bed destroys on leave");
    assert!(decision.should_skip, "all sleeping skips night");
}

#[test]
fn two_part_bed_geometry_matches_facing_and_heights() {
    let block = BlockPos::new;
    check_facing(BedFacing::North, block(10, 64, 10), block(10, 64, 9));
    check_facing(BedFacing::South, block(0, -60, 2), block(0, -60, 3));
    check_facing(BedFacing::West, block(4, 64, -2), block(3, 64, -2));
    check_facing(BedFacing::East, block(-5, 64, 7), block(-4, 64, 7));
    let straw = sleep_anchor(block(-3, -60, 3), STRAW_BED_TOP_OFFSET);
    assert_eq!(straw.y, -59.625, "straw height");
}

#[test]
fn sky_light_when_dark_matches_clear_rain_and_thunder_boundaries() {
    let clear_start = i64::from(CLEAR_NIGHT_START_TICK);
    let clear_end = i64::from(CLEAR_NIGHT_END_TICK);
    let rain_start = i64::from(FULL_RAIN_NIGHT_START_TICK);
    let rain_end = i64::from(FULL_RAIN_NIGHT_END_TICK);
    assert!(!sky_dark(6_000, 0.0, 0.0), "noon clear");
    assert!(!sky_dark(clear_start - 1, 0.0, 0.0), "before clear start");
    assert!(sky_dark(clear_start, 0.0, 0.0), "clear start");
    assert!(sky_dark(clear_end, 0.0, 0.0), "clear end");
    assert!(!sky_dark(clear_end + 1, 0.0, 0.0), "after clear end");
    assert!(!sky_dark(6_000, 1.0, 0.0), "noon rain");
    assert!(!sky_dark(rain_start - 1, 1.0, 0.0), "before rain start");
    assert!(sky_dark(rain_start, 1.0, 0.0), "rain start");
    assert!(sky_dark(rain_end, 1.0, 0.0), "rain end");
    assert!(!sky_dark(rain_end + 1, 1.0, 0.0), "after rain end");
    assert!(!sky_dark(24_000, 1.0, 0.0), "day wrap rain");
    assert!(!sky_dark(6_000, 1.0, 0.25), "partial storm is too bright");
    assert!(!sky_dark(6_000, 0.0, 1.0), "thunder only bright");
    assert!(sky_dark(6_000, 1.0, 1.0), "full storm allows noon sleep");
    assert_sky(0.5, 0.0, 13.28125, "half rain");
    assert_sky(1.0, 0.25, 11.311_569, "partial storm");
    assert_sky(1.0, 1.0, 9.199_219, "full storm");
    assert!(can_condition_pass(BedRuleCondition::Always, 15.0), "always");
    assert!(!can_condition_pass(BedRuleCondition::Never, 4.0), "never");
}

#[test]
fn dimension_rules_spawn_reach_and_monsters_match_probe() {
    let pair = BedPair::from_placement(BlockPos::new(0, -60, 2), BedFacing::South);
    let explode = SleepCheckInput::new(EXPLODING_BED_RULE, 4.0);
    let exploded = evaluate_bed_use(&explode) == BedUseOutcome::Explode;
    assert!(exploded, "explode");
    let spec = EXPLODING_BED_RULE.explosion_spec();
    let spec_ok = spec.is_some_and(|item| item.power == 5.0 && item.creates_fire);
    assert!(spec_ok, "explosion spec");
    let origin = explosion_origin(BlockPos::new(0, 64, 3));
    assert_eq!(origin, point(0.5, 64.5, 3.5), "origin");
    let mut never_rule = EXPLODING_BED_RULE;
    never_rule.destroy_on_use = false;
    let mut occupied = SleepCheckInput::new(OVERWORLD_BED_RULE, 4.0);
    occupied.occupied = true;
    let mut obstructed = SleepCheckInput::new(OVERWORLD_BED_RULE, 4.0);
    obstructed.obstruction = ObstructionState::Blocked;
    let cannot_sleep = SleepRejection::CannotSleepNow;
    let keeps = SpawnUpdate::Keeps;
    let day_red = SleepCheckInput::new(OVERWORLD_BED_RULE, 15.0);
    let day_straw = SleepCheckInput::new(OVERWORLD_STRAW_BED_RULE, 15.0);
    let disallowed = SleepCheckInput::new(never_rule, 4.0);
    assert_rejected(&day_red, cannot_sleep, SpawnUpdate::Sets);
    assert_rejected(&day_straw, cannot_sleep, keeps);
    assert_rejected(&disallowed, SleepRejection::RuleDisallowed, keeps);
    assert_rejected(&occupied, SleepRejection::Occupied, keeps);
    assert_rejected(&obstructed, SleepRejection::Obstructed, keeps);
    let foot_only = point(3.5, -60.0, -0.5);
    let head_only = point(3.5, -60.0, 6.5);
    let foot_hit = bed_in_reach(foot_only, pair) && !is_within_bed_reach(foot_only, pair.head);
    let head_hit = bed_in_reach(head_only, pair) && !is_within_bed_reach(head_only, pair.foot);
    let reach_failed = !bed_in_reach(point(3.6, -60.0, 3.5), pair);
    assert!(foot_hit && head_hit && reach_failed, "foot and head reach");
    assert!(!zombie_blocks(pair, 8.9, -60.0, 3.5), "outside x");
    assert!(zombie_blocks(pair, 8.7, -60.0, 3.5), "touching x");
    assert!(zombie_blocks(pair, 8.5, -60.0, 3.5), "inside x");
    assert!(!zombie_blocks(pair, 0.5, -55.0, 3.5), "above y boundary");
    assert!(zombie_blocks(pair, 0.5, -55.1, 3.5), "inside y");
    assert!(!zombie_blocks(pair, 0.5, -60.0, -5.4), "outside z");
    let mut creative = SleepCheckInput::new(OVERWORLD_BED_RULE, 4.0);
    creative.monster_nearby = true;
    creative.creative_bypass = CreativeBypass::Creative;
    let started = evaluate_bed_use(&creative) == BedUseOutcome::SleepStarted;
    assert!(started, "creative bypasses");
}

#[test]
fn multiplayer_sleep_percentage_and_wake_outcomes_work() {
    assert_eq!(required_sleeping_players(0, 100), None, "zero players");
    assert_eq!(required_sleeping_players(2, 100), Some(2), "2 at 100%");
    assert_eq!(required_sleeping_players(2, 50), Some(1), "2 at 50%");
    assert_eq!(required_sleeping_players(3, 50), Some(2), "3 at 50%");
    assert_eq!(required_sleeping_players(2, 0), Some(1), "minimum 1 at 0%");
    assert_eq!(required_sleeping_players(2, 101), None, "over 100%");
    let max_required = required_sleeping_players(4, i32::MAX);
    assert_eq!(max_required, None, "max percentage");
    assert_eq!(next_morning_tick(14_000), 24_000, "morning 0");
    assert_eq!(next_morning_tick(25_500), 48_000, "day 2 morning");
    let missing = evaluate_sleep_wake(OVERWORLD_STRAW_BED_RULE, SleepExitReason::BedMissing);
    assert_eq!(missing.next_pose, Pose::Standing, "wake pose is standing");
    assert!(!missing.destroy_bed, "missing bed does not re-destroy");
    let mut not_ready = SleepSkipInput::new(skip_stats(2, 1, 1), 100, 14_121);
    not_ready.raining_or_thundering = true;
    let decision = evaluate_night_skip(&not_ready);
    assert!(!decision.should_skip, "1/2 at 100% does not skip");
    assert!(!decision.clear_weather, "failed skip keeps weather");
}
