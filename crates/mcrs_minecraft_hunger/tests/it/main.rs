use mcrs_minecraft_hunger::{
    ATTACK_EXHAUSTION, HARVEST_BREAK_EXHAUSTION, HungerDifficulty, HungerState, HungerTickInput,
    HungerTickOutput, JUMP_EXHAUSTION, MAX_EXHAUSTION, SPRINT_JUMP_EXHAUSTION, SprintState,
    hunger_effect_exhaustion, jump_exhaustion, sprint_move_exhaustion,
};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn tick_in(health: f32, diff: HungerDifficulty, regen: bool, t: u64) -> HungerTickInput {
    HungerTickInput::new(health, 20.0, diff, regen, false, t)
}

#[test]
fn defaults_and_exhaustion_clamping_and_formulas() {
    let mut s = HungerState::default();
    assert_eq!(
        (s.food_level, s.saturation, s.exhaustion, s.tick_timer),
        (20, 5.0, 0.0, 0)
    );
    for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
        s.add_exhaustion(bad);
    }
    assert_eq!(s.exhaustion, 0.0);
    s.add_exhaustion(39.5);
    s.add_exhaustion(f32::MAX);
    assert_eq!(s.exhaustion, MAX_EXHAUSTION);

    assert_eq!(
        (
            ATTACK_EXHAUSTION,
            HARVEST_BREAK_EXHAUSTION,
            JUMP_EXHAUSTION,
            SPRINT_JUMP_EXHAUSTION
        ),
        (0.1, 0.005, 0.05, 0.2)
    );
    assert_eq!(
        (
            jump_exhaustion(SprintState::Idle),
            jump_exhaustion(SprintState::Sprinting)
        ),
        (0.05, 0.2)
    );
    assert_eq!(sprint_move_exhaustion(0.004), 0.0);
    assert!((sprint_move_exhaustion(1.0) - 0.1).abs() < 1.0e-6);
    assert!((hunger_effect_exhaustion(79) - 0.4).abs() < 1.0e-6);
    assert!((hunger_effect_exhaustion(119) - 0.6).abs() < 1.0e-6);
}

#[test]
fn drain_regen_peaceful_and_starvation_rules() {
    let mut exact = HungerState::new(20, 5.0, 4.0, 0);
    let _ = exact.tick(tick_in(20.0, HungerDifficulty::Normal, true, 1));
    assert_eq!((exact.exhaustion, exact.saturation), (4.0, 5.0));

    let mut heavy = HungerState::new(20, 5.0, 10.0, 0);
    let _ = heavy.tick(tick_in(20.0, HungerDifficulty::Normal, true, 1));
    assert_eq!((heavy.exhaustion, heavy.saturation), (6.0, 4.0));

    let mut fast = HungerState::new(20, 5.0, 0.55, 0);
    let mut h = 16.0_f32;
    for t in 1..=40 {
        h = fast
            .tick(tick_in(h, HungerDifficulty::Normal, true, t))
            .next_health
            .unwrap_or(h);
    }
    assert!(
        (h - 18.333334).abs() < 1.0e-5
            && (fast.exhaustion - 2.55).abs() < 1.0e-4
            && fast.saturation == 2.0
    );

    let mut slow = HungerState::new(18, 0.0, 0.550001, 0);
    let mut sh = 15.333332_f32;
    for t in 1..=81 {
        let out = slow.tick(tick_in(sh, HungerDifficulty::Normal, true, t));
        if t < 80 {
            assert_eq!(out, HungerTickOutput::default());
        }
        sh = out.next_health.unwrap_or(sh);
    }
    assert!(
        (sh - 16.333332).abs() < 1.0e-5
            && slow.food_level == 17
            && (slow.exhaustion - 2.5500011).abs() < 1.0e-5
    );
    slow.tick_timer = 15;
    assert_eq!(
        slow.tick(tick_in(14.0, HungerDifficulty::Normal, false, 82)),
        HungerTickOutput::default()
    );
    assert_eq!(slow.tick_timer, 0);

    let mut peaceful = HungerState::new(19, 1.0, 0.0, 9);
    let p_out = peaceful.tick(tick_in(15.5, HungerDifficulty::Peaceful, true, 20));
    assert_eq!(
        (
            peaceful.food_level,
            peaceful.saturation,
            peaceful.tick_timer
        ),
        (20, 2.0, 0)
    );
    assert!(
        p_out
            .next_health
            .is_some_and(|v| (v - 16.833334).abs() < 1.0e-5)
            && !p_out.starve
    );

    for (diff, hp, expected) in [
        (HungerDifficulty::Peaceful, 10.5, true),
        (HungerDifficulty::Peaceful, 10.0, false),
        (HungerDifficulty::Easy, 10.5, true),
        (HungerDifficulty::Easy, 10.0, false),
        (HungerDifficulty::Normal, 2.0, true),
        (HungerDifficulty::Normal, 1.0, false),
        (HungerDifficulty::Hard, 1.0, true),
    ] {
        let mut stv = HungerState::new(0, 0.0, 0.0, 79);
        assert_eq!(stv.tick(tick_in(hp, diff, true, 1)).starve, expected);
        assert_eq!(stv.tick_timer, 0);
    }
}

#[test]
fn warm_hunger_operations_allocate_nothing() {
    let before = allocations();
    let mut s = HungerState::default();
    s.add_exhaustion(ATTACK_EXHAUSTION + HARVEST_BREAK_EXHAUSTION);
    s.add_exhaustion(jump_exhaustion(SprintState::Sprinting) + sprint_move_exhaustion(1.25));
    s.add_exhaustion(hunger_effect_exhaustion(79));
    for t in 0..200_u64 {
        let _ = s.tick(tick_in(12.0, HungerDifficulty::Normal, true, t));
    }
    assert_eq!(
        allocations() - before,
        0,
        "pure hunger ops must not allocate"
    );
}
