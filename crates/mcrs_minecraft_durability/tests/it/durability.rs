use core::hint::black_box;

use mcrs_minecraft_durability::ArmourBypass;
use mcrs_minecraft_durability::HurtWearPolicy;
use mcrs_minecraft_durability::SlotBreakEvent;
use mcrs_minecraft_durability::WearKind;
use mcrs_minecraft_durability::armour_hit_wear;
use mcrs_minecraft_durability::attack_wear;
use mcrs_minecraft_durability::block_break_wear;
use mcrs_minecraft_durability::roll_unbreaking;
use mcrs_minecraft_durability::roll_unbreaking_salted;
use mcrs_minecraft_entity_key::EntityUuid;
use mcrs_minecraft_entity_key::KeyedStream;
use mcrs_test_alloc::allocations;

#[test]
fn durability_operations_allocate_nothing() {
    let stream = KeyedStream::new(&EntityUuid::from_u128(42), 17);
    let before = allocations();
    let b = black_box(block_break_wear(2, 1.5));
    let a = black_box(attack_wear(1));
    let r = black_box(armour_hit_wear(
        8.0,
        HurtWearPolicy::DamagesOnHurt,
        ArmourBypass::AppliesToArmour,
    ));
    let u = black_box(roll_unbreaking(b + a + r, 3, WearKind::Tool, &stream));
    assert_eq!(
        allocations() - before,
        0,
        "durability operations must perform zero heap allocations"
    );
    assert!(u >= 0, "unbreaking wear must be non-negative, got {u}");
}

#[test]
fn block_break_attack_and_armour_formulas_match_spec() {
    assert_eq!(block_break_wear(1, 0.0), 0);
    assert_eq!(block_break_wear(2, 0.0), 0);
    assert_eq!(block_break_wear(1, -1.0), 0);
    assert_eq!(block_break_wear(1, f32::NAN), 0);
    assert_eq!(block_break_wear(1, f32::INFINITY), 0);
    assert_eq!(block_break_wear(0, 1.5), 0);
    assert_eq!(block_break_wear(-3, 1.5), 0);
    assert_eq!(block_break_wear(1, 1.5), 1);
    assert_eq!(block_break_wear(2, 1.5), 2);

    assert_eq!(attack_wear(-1), 0);
    assert_eq!(attack_wear(0), 0);
    assert_eq!(attack_wear(1), 1);
    assert_eq!(attack_wear(3), 3);

    let hurt = HurtWearPolicy::DamagesOnHurt;
    let ignore = HurtWearPolicy::IgnoresHurt;
    let apply = ArmourBypass::AppliesToArmour;
    let bypass = ArmourBypass::BypassesArmour;

    assert_eq!(armour_hit_wear(4.0, hurt, bypass), 0);
    assert_eq!(armour_hit_wear(4.0, ignore, apply), 0);
    assert_eq!(armour_hit_wear(0.0, hurt, apply), 0);
    assert_eq!(armour_hit_wear(-1.0, hurt, apply), 0);
    assert_eq!(armour_hit_wear(f32::NAN, hurt, apply), 0);
    assert_eq!(armour_hit_wear(0.5, hurt, apply), 1);
    assert_eq!(armour_hit_wear(3.9, hurt, apply), 1);
    assert_eq!(armour_hit_wear(4.0, hurt, apply), 1);
    assert_eq!(armour_hit_wear(7.9, hurt, apply), 1);
    assert_eq!(armour_hit_wear(8.0, hurt, apply), 2);
    assert_eq!(armour_hit_wear(20.0, hurt, apply), 5);
    assert_eq!(armour_hit_wear(f32::MAX, hurt, apply), i32::MAX);
}

#[test]
fn unbreaking_rolls_follow_tool_and_armour_distributions() {
    let stream = KeyedStream::new(&EntityUuid::from_u128(99), 5);
    assert_eq!(roll_unbreaking(0, 3, WearKind::Tool, &stream), 0);
    assert_eq!(roll_unbreaking(-2, 3, WearKind::Tool, &stream), 0);
    assert_eq!(roll_unbreaking(4, 0, WearKind::Tool, &stream), 4);
    assert_eq!(roll_unbreaking(4, -1, WearKind::Armour, &stream), 4);

    let cases = [
        (WearKind::Tool, 1, 0.50),
        (WearKind::Tool, 3, 0.25),
        (WearKind::Armour, 1, 0.80),
        (WearKind::Armour, 3, 0.70),
    ];
    for (kind, level, expected) in cases {
        let mut total = 0_i64;
        let samples = 10_000_i64;
        for tick in 0..samples {
            let s = KeyedStream::new(&EntityUuid::from_u128(12345), tick as u64);
            total += i64::from(roll_unbreaking_salted(1, level, kind, &s, 0));
        }
        let observed = (total as f64) / (samples as f64);
        assert!(
            (observed - expected).abs() < 0.03,
            "kind={kind:?} level={level}: observed {observed} vs expected {expected}"
        );
    }

    let zero_draw = (0..10_000_u64).find(|&tick| {
        let s = KeyedStream::new(&EntityUuid::from_u128(7), tick);
        roll_unbreaking(4, 3, WearKind::Tool, &s) == 0
    });
    assert!(
        zero_draw.is_some(),
        "unbreaking 3 roll should be able to keep 0 of 4 durability units"
    );
}

#[test]
fn slot_break_event_statuses_match_wire_values() {
    assert_eq!(SlotBreakEvent::MainHand.status(), 47);
    assert_eq!(SlotBreakEvent::OffHand.status(), 48);
    assert_eq!(SlotBreakEvent::Feet.status(), 49);
    assert_eq!(SlotBreakEvent::Legs.status(), 50);
    assert_eq!(SlotBreakEvent::Chest.status(), 51);
    assert_eq!(SlotBreakEvent::Head.status(), 52);
}
