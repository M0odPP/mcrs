use core::hint::black_box;

use glam::DVec3;
use mcrs_minecraft_combat_rules as combat;
use mcrs_minecraft_combat_rules::BlockingUseState::{Active, Disabled};
use mcrs_minecraft_combat_rules::ReachMode::{Creative, Mob, Survival};
use mcrs_minecraft_combat_rules::ReachVerdict::{InRange, InvalidRange, TooClose, TooFar};
use mcrs_minecraft_damage::{
    DamageTags, DamageTypeRules, Hit, HitOutcome, HitRecord, HitReport, Vitals,
};
use mcrs_minecraft_item_stack as item;
use mcrs_test_alloc::CountingAllocator;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const MELEE: u32 = 0;
const BYPASS: u32 = 1;
const VOID: u32 = 2;

fn hit(kind: u32, amount: f32, position: Option<DVec3>) -> Hit {
    let mut source = mcrs_minecraft_damage::DamageSource::of(kind);
    source.position = position;
    Hit::new(amount, source)
}

fn tag_match(tag_id: u16, kind: u32) -> bool {
    tag_id == 0 && kind == BYPASS
}

fn hurt_at(rules: &DamageTypeRules, vitals: &mut Vitals, kind: u32, amount: f32) -> HitReport {
    let mut record = HitRecord::default();
    let (empty, rule) = (Default::default(), Default::default());
    let incoming = hit(kind, amount, None);
    mcrs_minecraft_damage::hurt(vitals, &mut record, &empty, rules, &rule, incoming, 1)
}

fn verify_blocking(shield: &item::BlocksAttacks, axe: &item::Weapon) {
    let offset = |x, z| Some(DVec3::new(x, 0.0, z));
    let (front, side, back) = (offset(0.0, 3.0), offset(3.0, 0.0), offset(0.0, -3.0));
    let (weapon, zero, limit) = (Some(axe), DVec3::ZERO, f32::MAX);
    let evaluate_block = combat::evaluate_blocks_attacks;
    let delay_ok =
        combat::block_delay_ticks(0.25) == 5 && combat::block_delay_ticks(limit) == u32::MAX;
    let disable_ok = combat::shield_disable_ticks(5.0, 0.5) == 50
        && combat::shield_disable_ticks(limit, 2.0) == u32::MAX;
    let saturated = combat::shield_item_damage(0.0, limit, 1.0, limit);
    let wear_ok = combat::shield_item_damage(3.0, 1.0, 1.0, 3.0) == 4 && saturated == i32::MAX;
    let yaw_ok = combat::horizontal_attack_angle_degrees(zero, 90.0, offset(-3.0, 0.0)) < 0.01;
    let scalar_ok = delay_ok && disable_ok && wear_ok && yaw_ok;
    assert!(scalar_ok, "delay disable wear and yaw");

    let check_block = |blocks, state, kind, amount, source, blocked, damage, disable| {
        let incoming = hit(kind, amount, source);
        let outcome = evaluate_block(blocks, weapon, state, zero, 0.0, incoming, tag_match);
        let checked = outcome.blocked_damage == blocked
            && outcome.item_damage == damage
            && outcome.disable_ticks == disable;
        assert!(checked, "blocking outcome matches expected");
    };
    check_block(shield, Active(4), MELEE, 8.0, front, 0.0, 0, 0);
    check_block(shield, Disabled, MELEE, 8.0, front, 0.0, 0, 0);
    check_block(shield, Active(5), MELEE, 2.9, side, 2.9, 0, 100);
    check_block(shield, Active(5), MELEE, 8.0, back, 0.0, 0, 0);
    check_block(shield, Active(5), BYPASS, 8.0, front, 0.0, 0, 0);
    check_block(shield, Active(5), MELEE, 6.0, None, 0.0, 0, 0);

    let damage_set = |ids| Some(item::DamageTypeSet::Types(std::borrow::Cow::Borrowed(ids)));
    let mut multi = item::BlocksAttacks {
        disable_cooldown_scale: 0.5,
        bypassed_by: damage_set(&[item::DamageTypeId(2)]),
        damage_reductions: Vec::new(),
        ..Default::default()
    };
    for (kind, base) in [(damage_set(&[item::DamageTypeId(0)]), 4.0), (None, 6.0)] {
        let mut entry = item::DamageReduction::new();
        (entry.kind, entry.base, entry.factor) = (kind, base, 0.0);
        multi.damage_reductions.push(entry);
    }
    check_block(&multi, Active(5), MELEE, 8.0, front, 8.0, 8, 50);
    check_block(&multi, Active(5), VOID, 8.0, front, 0.0, 0, 0);
}

fn box_at(z: f64) -> combat::TargetBounds {
    let min = DVec3::new(-0.3, 0.0, z - 0.3);
    combat::TargetBounds::new(min, DVec3::new(0.3, 1.8, z + 0.3))
}

fn verify_reach(spear: &item::AttackRange, totem: &item::DeathProtection, rules: &DamageTypeRules) {
    let eye = DVec3::new(0.0, 1.62, 0.0);
    let of_body = combat::TargetBounds::of_body;
    let check_reach = |mode, z, tolerance, expected| {
        let outcome = combat::evaluate_attack_range(spear, mode, eye, box_at(z), tolerance);
        assert_eq!(outcome, expected, "attack reach matches bounds");
    };
    check_reach(Survival, 0.0, 0.0, TooClose);
    check_reach(Survival, 4.7, 0.0, InRange);
    check_reach(Survival, 6.0, 0.0, TooFar);
    check_reach(Creative, 6.0, 0.0, InRange);
    check_reach(Mob, 2.5, 0.0, InRange);
    check_reach(Survival, 2.0, -1.0, InvalidRange);

    let valid_body = of_body(DVec3::new(0.0, 0.0, 4.0), 0.6, 1.8) == Some(box_at(4.0));
    let invalid_body = of_body(DVec3::ZERO, -0.1, 1.0).or(of_body(DVec3::ZERO, 0.6, f64::NAN));
    assert!(
        valid_body && invalid_body.is_none(),
        "of_body validates box dimensions"
    );
    let bounds = combat::ReachBounds::new(0.0, 4.5, 0.3);
    let nan_box = combat::TargetBounds::new(DVec3::splat(f64::NAN), DVec3::ONE);
    let flipped_box = combat::TargetBounds::new(DVec3::ZERO, DVec3::new(-1.0, 1.0, 1.0));
    let invalid_target = |target_bounds: combat::TargetBounds| {
        combat::check_reach_bounds(bounds, eye, target_bounds, 0.0) == InvalidRange
            && target_bounds.nearest_distance(eye).is_nan()
    };
    let invalid_ok = invalid_target(nan_box) && invalid_target(flipped_box);
    assert!(invalid_ok, "invalid target fails");

    let scale_ok = combat::attack_strength_scale(4, 1.0E-40) == 0.0
        && !combat::has_minimum_attack_charge(10, 1.0E-40, 0.5)
        && !combat::has_minimum_attack_charge(7, 20.0 / 13.0, 1.0);
    let wear_ok = scale_ok && combat::weapon_attack_item_damage(None) == 1;
    assert!(wear_ok, "weapon scale charge wear");

    let evaluate_death = combat::evaluate_death_protection_component;
    for (kind, amount) in [(MELEE, 5.0), (VOID, 100.0)] {
        let mut vitals = Vitals::new(20.0);
        let mut report = hurt_at(rules, &mut vitals, kind, amount);
        let outcome = evaluate_death(totem, &mut vitals, &mut report, kind, rules);
        assert!(!outcome.triggered, "component respects bypass rules");
    }

    let mut dead = Vitals::new(20.0);
    (dead.health, dead.death_time) = (0.0, Some(7));
    let mut rejected = hurt_at(rules, &mut dead, MELEE, 10.0);
    let revived = evaluate_death(totem, &mut dead, &mut rejected, MELEE, rules);
    let rejected_ok = rejected.outcome == HitOutcome::Rejected && !revived.triggered;
    let time_ok = rejected_ok && dead.death_time == Some(7);
    assert!(time_ok, "rejected preserves death time");
}

#[test]
fn combat_rules_and_zero_allocations() {
    let probe_before = mcrs_test_alloc::allocations();
    black_box(vec![0_u8; 8]);
    let probed = mcrs_test_alloc::allocations() > probe_before;
    assert!(probed, "allocator tracks heap activity");

    let mut item_damage = item::ItemDamage::DEFAULT;
    (item_damage.threshold, item_damage.base, item_damage.factor) = (3.0, 1.0, 1.0);
    let shield = item::BlocksAttacks {
        block_delay_seconds: 0.25,
        bypassed_by: Some(item::DamageTypeSet::Tag(item::DamageTypeTagId(0))),
        item_damage,
        ..Default::default()
    };
    let mut spear = item::AttackRange::DEFAULT;
    (spear.min_reach, spear.max_reach, spear.max_creative_reach) = (2.0, 4.5, 6.5);
    (spear.hitbox_margin, spear.mob_factor) = (0.125, 0.5);
    let mut axe = item::Weapon::DEFAULT;
    (axe.item_damage_per_attack, axe.disable_blocking_for_seconds) = (2, 5.0);
    let (empty, void_tag) = (DamageTags::empty(), DamageTags::BYPASSES_INVULNERABILITY);
    let rules = DamageTypeRules::new([empty, empty, void_tag]);
    let (eye, target) = (DVec3::new(0.0, 1.62, 0.0), box_at(4.0));
    let frontal = hit(MELEE, 8.0, Some(DVec3::new(0.0, 0.0, 3.0)));
    let mut vitals = Vitals::new(20.0);
    let mut report = hurt_at(&rules, &mut vitals, MELEE, 25.0);
    let totem = item::DeathProtection {
        death_effects: vec![item::ConsumeEffect::ClearAllEffects],
    };
    let evaluate_block = combat::evaluate_blocks_attacks;
    let evaluate_death = combat::evaluate_death_protection_component;
    let (weapon, state, zero) = (Some(&axe), Active(5), DVec3::ZERO);

    let before = mcrs_test_alloc::allocations();
    let blocked = evaluate_block(&shield, weapon, state, zero, 0.0, frontal, tag_match);
    let reach = combat::evaluate_attack_range(&spear, Survival, eye, target, 0.0);
    let charged =
        combat::has_minimum_attack_charge(8, 20.0 / 13.0, item::MinimumAttackCharge(1.0).0);
    let scaled = combat::scaled_attack_damage(7.0, combat::attack_strength_scale(4, 4.0));
    let wear = combat::weapon_attack_item_damage(weapon);
    let saved = evaluate_death(&totem, &mut vitals, &mut report, MELEE, &rules);

    let allocated = mcrs_test_alloc::allocations() - before;
    assert_eq!(allocated, 0, "hot paths allocate nothing");
    let (block, saved) = (black_box(blocked), black_box(saved));
    let block_ok = block.blocked_damage == 8.0 && block.remaining_damage == 0.0;
    let wear_ok = block.item_damage == 9 && block.disable_ticks == 100;
    let scale_ok = (black_box(scaled) - 5.936).abs() < 1.0E-4 && black_box(reach) == InRange;
    let charge_ok = black_box(charged) && black_box(wear) == 2 && saved.triggered && !report.died;
    let alive_ok = vitals.is_alive() && vitals.health == 1.0 && saved.entity_event == Some(35);
    let totem_ok = alive_ok && saved.death_effects.len() == 1;
    let rules_ok = block_ok && wear_ok && scale_ok && charge_ok && totem_ok;
    assert!(rules_ok, "core combat rules check");

    verify_blocking(&shield, &axe);
    verify_reach(&spear, &totem, &rules);
}
