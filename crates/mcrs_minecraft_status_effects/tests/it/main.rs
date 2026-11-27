use mcrs_minecraft_entity::EntityTypeData;
use mcrs_minecraft_entity::entity_type_tag_keys::IMMUNE_TO_POISON;
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_entity_attribute::{
    AttributeInstances, InstanceChange, ModifierKind, ModifierOperation,
};
use mcrs_minecraft_resource::rl;
use mcrs_minecraft_status_effects::{
    ActiveEffect, ActiveEffects, AddEffectStatus, EffectDamageKind, EffectId, EffectImmunities,
    EffectStack, EffectTickEvent, EffectTickInput, EffectUpdate, HealingPolarity,
    INFINITE_DURATION, apply_effect_attribute_modifier, clamp_amplifier, effect_acts_on_tick,
    effect_attribute_modifier, effect_immunity_tag, hunger_exhaustion_per_tick,
    instant_effect_with_potency, is_shorter_duration, remove_effect_attribute_modifier,
};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

struct CadenceCase {
    effect: EffectId,
    amplifier: u8,
    period: u64,
}

const fn cadence(effect: EffectId, amplifier: u8, period: u64) -> CadenceCase {
    CadenceCase {
        effect,
        amplifier,
        period,
    }
}

fn effect(id: EffectId, duration: i32, amplifier: u8) -> ActiveEffect {
    ActiveEffect::new(id, duration, amplifier)
}

const fn update(effect: EffectId, amplifier: u8, duration: i32) -> EffectUpdate {
    EffectUpdate {
        effect,
        amplifier,
        duration,
        flags: 0x06,
    }
}

const fn damage(effect: EffectId, kind: EffectDamageKind, amount: f32) -> EffectTickEvent {
    EffectTickEvent::Damage {
        effect,
        kind,
        amount,
    }
}

fn tick_once(store: &mut ActiveEffects, state: EffectTickInput) -> Vec<EffectTickEvent> {
    let mut out = Vec::new();
    store.tick(state, |event| out.push(event));
    out
}

#[test]
fn status_effect_lifecycle_and_zero_alloc() {
    use EffectTickEvent::{Promoted, Removed, Sync};
    let clamped =
        clamp_amplifier(-5) == 0 && clamp_amplifier(42) == 42 && clamp_amplifier(999) == 255;
    assert!(clamped, "amplifier clamps into 0..=255");
    let shorter = is_shorter_duration(100, 200) && is_shorter_duration(100, -1);
    let infinite = !is_shorter_duration(-1, 100) && !is_shorter_duration(-1, -1);
    assert!(shorter && infinite, "finite shorter than infinite");

    let mut store = ActiveEffects::default();
    let refused =
        store.add_with_immunity(effect(EffectId::POISON, 100, 0), EffectImmunities::POISON);
    assert_eq!(refused, AddEffectStatus::Refused);
    let added = store.add(effect(EffectId::SPEED, 200, 0));
    assert_eq!(added, AddEffectStatus::Added);
    let mut ambient = effect(EffectId::SPEED, 100, 0);
    ambient.stack.ambient = true;
    assert_eq!(refused.packet_flags(&ambient.stack), None);
    assert_eq!(store.add(ambient.clone()), AddEffectStatus::Unchanged);
    ambient.stack.duration = 400;
    assert_eq!(store.add(ambient), AddEffectStatus::Updated);
    let active = &store.get(EffectId::SPEED).unwrap().stack;
    assert!(active.ambient, "longer ambient overwrites active flag");
    assert_eq!(added.packet_flags(active), Some(0x0F));
    let cleared = store.add(effect(EffectId::SPEED, 50, 0));
    assert_eq!(cleared, AddEffectStatus::Updated);
    let after = &store.get(EffectId::SPEED).unwrap().stack;
    assert!(!after.ambient, "non-ambient clears existing ambient flag");
    assert_eq!(cleared.packet_flags(after), Some(0x06));

    store.insert_loaded(effect(EffectId::SPEED, 601, 3));
    let _ = store.add(effect(EffectId::INSTANT_HEALTH, 0, 0));
    let _ = store.add(effect(EffectId::NIGHT_VISION, INFINITE_DURATION, 0));
    for (duration, amplifier) in [(10, 0), (3, 2), (6, 1)] {
        let _ = store.add(effect(EffectId::STRENGTH, duration, amplifier));
    }
    let mut expired = effect(EffectId::WEAKNESS, 1, 2);
    expired.stack.hidden_effect = Some(Box::new(EffectStack::new(0, 1)));
    store.insert_loaded(expired);
    let mut events = Vec::with_capacity(16);
    let before = allocations();
    for tick in 1..=10_u64 {
        let state = EffectTickInput::new(tick, 20.0, 20.0, 0.0);
        store.tick(state, |event| events.push(event));
    }
    let leaked = allocations() - before;
    assert_eq!(leaked, 0, "steady-state ticks must not allocate");
    assert_eq!(
        events,
        [
            Sync(update(EffectId::SPEED, 3, 600)),
            Removed(EffectId::INSTANT_HEALTH, 0),
            Promoted(update(EffectId::WEAKNESS, 1, 0)),
            Removed(EffectId::WEAKNESS, 1),
            Promoted(update(EffectId::STRENGTH, 1, 3)),
            Promoted(update(EffectId::STRENGTH, 0, 4)),
            Removed(EffectId::STRENGTH, 0),
        ]
    );
    let gone = store.get(EffectId::WEAKNESS).is_none();
    assert!(gone, "zero-duration hidden layer must expire");
    assert_eq!(store.remove(EffectId::SPEED).unwrap().stack.amplifier, 3);
    assert_eq!(store.drain().count(), 1);
}

#[test]
fn effect_cadences_periodic_ticks_and_polarity() {
    use EffectDamageKind::{Magic, Wither};
    use EffectTickEvent::{Heal, Removed};
    use HealingPolarity::{Inverted, Living};
    for case in [
        cadence(EffectId::REGENERATION, 0, 50),
        cadence(EffectId::REGENERATION, 4, 3),
        cadence(EffectId::POISON, 0, 25),
        cadence(EffectId::WITHER, 0, 40),
    ] {
        for tick in [1_u64, 3, 25, 40, 50, 100] {
            let active = effect_acts_on_tick(case.effect, case.amplifier, tick);
            assert_eq!(active, tick.is_multiple_of(case.period));
        }
    }
    let skip_zero = !effect_acts_on_tick(EffectId::INSTANT_HEALTH, 0, 0);
    let acts_one = effect_acts_on_tick(EffectId::INSTANT_HEALTH, 0, 1);
    assert!(skip_zero && acts_one, "instant health acts from tick one");

    let mut periodic = ActiveEffects::default();
    let _ = periodic.add(effect(EffectId::REGENERATION, 50, 5));
    let _ = periodic.add(effect(EffectId::POISON, 25, 4));
    let _ = periodic.add(effect(EffectId::WITHER, 40, 0));
    let wither = damage(EffectId::WITHER, Wither, 1.0);
    let out = tick_once(&mut periodic, EffectTickInput::new(1, 1.0, 20.0, 0.0));
    assert_eq!(out, [Heal(EffectId::REGENERATION, 1.0), wither]);
    let _ = periodic.add(effect(EffectId::INSTANT_HEALTH, 2, 0));
    let mut dying = EffectTickInput::new(2, 10.0, 20.0, 0.0);
    dying.dying = true;
    let suppressed = tick_once(&mut periodic, dying);
    assert!(suppressed.is_empty(), "dying suppresses healing and poison");

    let _ = periodic.drain();
    let _ = periodic.add(effect(EffectId::INSTANT_HEALTH, 1, 1));
    let _ = periodic.add(effect(EffectId::INSTANT_DAMAGE, 1, 0));
    let _ = periodic.add(effect(EffectId::ABSORPTION, 100, 1));
    let mut undead = EffectTickInput::new(1, 12.0, 20.0, 0.0);
    undead.polarity = Inverted;
    assert_eq!(
        tick_once(&mut periodic, undead),
        [
            damage(EffectId::INSTANT_HEALTH, Magic, 12.0),
            Removed(EffectId::INSTANT_HEALTH, 1),
            Heal(EffectId::INSTANT_DAMAGE, 4.0),
            Removed(EffectId::INSTANT_DAMAGE, 0),
            Removed(EffectId::ABSORPTION, 1),
        ]
    );
    let half = instant_effect_with_potency(EffectId::INSTANT_HEALTH, Living, 0, 0.5);
    assert_eq!(half, Some(Heal(EffectId::INSTANT_HEALTH, 2.0)));
    let hunger_ok = (hunger_exhaustion_per_tick(0) - 0.005).abs() < 1.0e-6;
    assert!(hunger_ok, "hunger base exhaustion must be 0.005");
}

#[test]
fn effect_id_attribute_modifiers_and_immunity_tags_match_spec() {
    let add = ModifierOperation::AddValue;
    let mul = ModifierOperation::AddMultipliedTotal;
    for &mob_effect in MobEffect::ALL {
        let id = EffectId::from(mob_effect);
        assert_eq!(id.0, mob_effect.id().number());
        assert_eq!(id.to_mob_effect(), Some(mob_effect));
    }
    assert_eq!(EffectId(u16::MAX).to_mob_effect(), None);
    let check = |mob_effect, attribute, operation, base: f32| {
        let entry = effect_attribute_modifier(mob_effect, 0).unwrap();
        let scaled = effect_attribute_modifier(mob_effect, 2).unwrap();
        let expected = f64::from(base);
        assert_eq!(entry.attribute, attribute);
        assert_eq!(entry.modifier.operation, operation);
        assert_eq!(entry.to_instance_modifier().kind(), ModifierKind::Permanent);
        assert_eq!(entry.modifier.amount.to_bits(), expected.to_bits());
        assert_eq!(scaled.modifier.amount.to_bits(), (expected * 3.0).to_bits());
        entry
    };
    let speed = check(MobEffect::Speed, Attribute::MovementSpeed, mul, 0.2);
    let _ = check(MobEffect::Slowness, Attribute::MovementSpeed, mul, -0.15);
    let _ = check(MobEffect::Haste, Attribute::AttackSpeed, mul, 0.1);
    let _ = check(MobEffect::MiningFatigue, Attribute::AttackSpeed, mul, -0.1);
    let _ = check(MobEffect::Strength, Attribute::AttackDamage, add, 3.0);
    let _ = check(MobEffect::Weakness, Attribute::AttackDamage, add, -4.0);
    let _ = check(MobEffect::JumpBoost, Attribute::SafeFallDistance, add, 1.0);
    let _ = check(MobEffect::HealthBoost, Attribute::MaxHealth, add, 4.0);
    let _ = check(MobEffect::Absorption, Attribute::MaxAbsorption, add, 4.0);
    let _ = check(MobEffect::Luck, Attribute::Luck, add, 1.0);
    let _ = check(MobEffect::Unluck, Attribute::Luck, add, -1.0);
    let invis = MobEffect::Invisibility;
    let range = Attribute::WaypointTransmitRange;
    let hide = check(invis, range, mul, -1.0);
    assert_eq!(speed.modifier.id, rl!("minecraft:effect.speed"));
    let hide_id = rl!("minecraft:effect.waypoint_transmit_range_hide");
    assert_eq!(hide.modifier.id, hide_id);
    assert_eq!(effect_attribute_modifier(MobEffect::Regeneration, 0), None);

    let bases = EntityTypeData::PLAYER.attribute_bases().unwrap();
    let mut instances = AttributeInstances::new(bases);
    let applied = apply_effect_attribute_modifier(&mut instances, MobEffect::HealthBoost, 0);
    assert_eq!(applied, InstanceChange::Changed);
    assert_eq!(instances.value(Attribute::MaxHealth), Some(24.0));
    let removed = remove_effect_attribute_modifier(&mut instances, MobEffect::HealthBoost);
    assert_eq!(removed, InstanceChange::Changed);
    assert_eq!(instances.value(Attribute::MaxHealth), Some(20.0));
    let _ = apply_effect_attribute_modifier(&mut instances, MobEffect::Slowness, 6);
    assert_eq!(instances.value(Attribute::MovementSpeed), Some(0.0));
    let poison_tag = effect_immunity_tag(MobEffect::Poison);
    assert_eq!(poison_tag, Some(IMMUNE_TO_POISON));
    assert_eq!(effect_immunity_tag(MobEffect::Speed), None);
}
