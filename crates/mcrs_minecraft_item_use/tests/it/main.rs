use core::mem::{align_of, size_of};

use mcrs_minecraft_item_use::{
    AppliedConsumeEffect, ConsumableEffectInstance, ConsumeMode, CooldownMap,
    DEFAULT_CONSUME_SECONDS, DEFAULT_POTION_DURATION_SCALE, DEFAULT_USE_SPEED_MULTIPLIER,
    EffectTiming, INFINITE_EFFECT_DURATION_TICKS, ItemUseState, LIVING_FLAG_OFFHAND,
    LIVING_FLAG_SPIN_ATTACK, LIVING_FLAG_USING_ITEM, OMINOUS_BOTTLE_DURATION_TICKS,
    OMINOUS_BOTTLE_EFFECT_ID, RemainderOutcome, SHIELD_USE_DURATION_TICKS, SprintPolicy,
    StewEffectEntry, UseHand, UseKind, UseOutcome, UseSpec, cancel_on_slot_change,
    collect_potion_effects, collect_stew_effects, compute_consume_remainder, consume_ticks,
    cooldown_ticks, living_flags_bits, ominous_bottle_effect, release_use, resolve_cooldown_group,
    scale_potion_duration, should_emit_consume_tick_effects, start_use, tick_use,
};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn layout_matches_packed_expectations() {
    assert_eq!(size_of::<UseHand>(), 1, "UseHand should be 1 byte");
    assert_eq!(align_of::<UseHand>(), 1, "UseHand should be 1-byte aligned");
    assert_eq!(size_of::<UseSpec>(), 12, "UseSpec should be 12 bytes");
    assert_eq!(align_of::<UseSpec>(), 4, "UseSpec should be 4-byte aligned");
    assert_eq!(
        size_of::<ItemUseState>(),
        16,
        "ItemUseState should be 16 bytes"
    );
    assert_eq!(
        align_of::<ItemUseState>(),
        4,
        "ItemUseState should be 4-byte aligned"
    );
    assert_eq!(size_of::<UseOutcome>(), 20, "UseOutcome should be 20 bytes");
    assert_eq!(
        align_of::<UseOutcome>(),
        4,
        "UseOutcome should be 4-byte aligned"
    );
}

#[test]
fn living_flags_preserves_hand_and_spin_attack_bits() {
    let main = living_flags_bits(0, Some(UseHand::Main));
    assert_eq!(main, LIVING_FLAG_USING_ITEM, "main hand active sets bit 0");
    assert_eq!(
        living_flags_bits(main, None),
        0,
        "stopping main hand clears bit 0 and leaves bit 1 clear"
    );

    let off = living_flags_bits(0, Some(UseHand::Off));
    assert_eq!(
        off,
        LIVING_FLAG_USING_ITEM | LIVING_FLAG_OFFHAND,
        "offhand active sets bits 0 and 1 (3)"
    );
    let stopped_off = living_flags_bits(off, None);
    assert_eq!(
        stopped_off, LIVING_FLAG_OFFHAND,
        "stopping offhand clears bit 0 while keeping bit 1 (3 -> 2)"
    );

    let main_after_off = living_flags_bits(stopped_off, Some(UseHand::Main));
    assert_eq!(
        main_after_off, LIVING_FLAG_USING_ITEM,
        "starting main hand after offhand clears bit 1 and sets bit 0 (2 -> 1)"
    );

    let spin_base = LIVING_FLAG_SPIN_ATTACK;
    let spin_off = living_flags_bits(spin_base, Some(UseHand::Off));
    assert_eq!(spin_off, LIVING_FLAG_SPIN_ATTACK | 3, "spin bit preserved");
    assert_eq!(
        living_flags_bits(spin_off, None),
        LIVING_FLAG_SPIN_ATTACK | 2,
        "spin bit preserved across offhand release"
    );
}

#[test]
fn consume_ticks_handles_valid_and_invalid_durations() {
    assert_eq!(
        consume_ticks(DEFAULT_CONSUME_SECONDS),
        32,
        "1.6s is 32 ticks"
    );
    assert_eq!(consume_ticks(0.8), 16, "0.8s is 16 ticks");
    assert_eq!(consume_ticks(2.5), 50, "2.5s is 50 ticks");
    assert_eq!(consume_ticks(0.0), 0, "0.0s yields 0 ticks");
    assert_eq!(consume_ticks(-0.0), 0, "-0.0s yields 0 ticks");
    assert_eq!(consume_ticks(-1.0), 0, "negative duration yields 0 ticks");
    assert_eq!(consume_ticks(f32::NAN), 0, "NaN yields 0 ticks");
    assert_eq!(consume_ticks(f32::INFINITY), 0, "infinity yields 0 ticks");
}

#[test]
fn consumable_ticks_through_warmup_interval_and_completion() {
    let spec = UseSpec::consumable(
        DEFAULT_CONSUME_SECONDS,
        DEFAULT_USE_SPEED_MULTIPLIER,
        SprintPolicy::Forbid,
    );
    assert!(
        !spec.permits_sprint() && !spec.is_continuous(),
        "default consumable spec should forbid sprinting and mark non-continuous"
    );
    let mut state = start_use(None, UseHand::Main, 0, spec).expect("idle player can start use");
    assert!(
        start_use(Some(state), UseHand::Off, 0, spec).is_none(),
        "second simultaneous start must be rejected while already using"
    );

    let mut tick_effect_remaining = Vec::new();
    let mut completed = None;
    for _ in 0..32 {
        match tick_use(state) {
            UseOutcome::Continuing {
                state: next,
                tick_effects,
            } => {
                if tick_effects {
                    tick_effect_remaining.push(next.remaining_ticks());
                }
                state = next;
            }
            UseOutcome::Completed {
                state: done,
                tick_effects,
            } => {
                if tick_effects {
                    tick_effect_remaining.push(done.remaining_ticks());
                }
                completed = Some(done);
                break;
            }
        }
    }

    assert_eq!(
        tick_effect_remaining,
        [24, 20, 16, 12, 8, 4, 0],
        "consume tick effects should fire 7 times after 7/32 warmup every 4 ticks"
    );
    let done = completed.expect("consumable must finish on tick 32");
    assert_eq!(done.elapsed_ticks, 32, "32 ticks elapsed at completion");
    assert_eq!(done.remaining_ticks(), 0, "0 ticks remaining at completion");
}

#[test]
fn zero_duration_tick_effect_schedule_and_release_and_slot_change() {
    assert!(
        should_emit_consume_tick_effects(1, 0),
        "single-tick consumable emits tick effect on tick 1"
    );

    let shield = UseSpec::continuous(DEFAULT_USE_SPEED_MULTIPLIER, SprintPolicy::Forbid);
    assert_eq!(
        shield.kind,
        UseKind::Continuous,
        "continuous constructor sets UseKind::Continuous"
    );
    let mut main_use =
        start_use(None, UseHand::Main, 2, shield).expect("idle player starts main-hand shield");
    assert_eq!(
        main_use.use_duration_ticks, SHIELD_USE_DURATION_TICKS,
        "shield starts with 72_000 duration window"
    );

    for tick in 1..=72_005 {
        match tick_use(main_use) {
            UseOutcome::Continuing {
                state,
                tick_effects,
            } => {
                assert!(
                    !tick_effects,
                    "continuous shield should never emit consume tick effects on tick {tick}"
                );
                main_use = state;
            }
            UseOutcome::Completed { .. } => {
                panic!(
                    "continuous shield must never complete even past 72_000 ticks (tick {tick})"
                );
            }
        }
    }
    assert_eq!(
        main_use.elapsed_ticks, 72_005,
        "continuous shield keeps incrementing elapsed ticks past 72_000"
    );

    assert_eq!(
        cancel_on_slot_change(main_use, 2),
        Some(main_use),
        "staying on the active main-hand slot keeps item use active"
    );
    assert_eq!(
        cancel_on_slot_change(main_use, 5),
        None,
        "switching hotbar slot while using main hand cancels item use"
    );

    let offhand_use =
        start_use(None, UseHand::Off, 2, shield).expect("idle player starts offhand shield");
    assert_eq!(
        cancel_on_slot_change(offhand_use, 5),
        Some(offhand_use),
        "switching hotbar slot keeps offhand item use active"
    );

    let released = release_use(Some(main_use)).expect("releasing active item returns state");
    assert_eq!(
        released.elapsed_ticks, 72_005,
        "elapsed ticks preserved on release"
    );
    assert!(release_use(None).is_none(), "releasing idle returns None");
}

#[test]
fn rules_perform_zero_heap_allocations() {
    let spec = UseSpec::consumable(
        DEFAULT_CONSUME_SECONDS,
        DEFAULT_USE_SPEED_MULTIPLIER,
        SprintPolicy::Allow,
    );
    assert!(spec.permits_sprint(), "allow sprint policy permits sprint");
    let before = allocations();
    let mut flags = 0_u8;
    let state = start_use(None, UseHand::Off, 1, spec);
    if let Some(mut active) = state {
        flags = living_flags_bits(flags, Some(active.hand));
        for _ in 0..32 {
            match tick_use(active) {
                UseOutcome::Continuing { state, .. } => active = state,
                UseOutcome::Completed { .. } => {
                    flags = living_flags_bits(flags, None);
                    break;
                }
            }
        }
        let _ = cancel_on_slot_change(active, 4);
        let _ = release_use(Some(active));
    }
    let after = allocations();
    assert_eq!(
        flags, LIVING_FLAG_OFFHAND,
        "offhand completion clears bit 0"
    );
    assert_eq!(
        after - before,
        0,
        "mcrs_minecraft_item_use must allocate zero bytes"
    );
}

#[test]
fn cooldown_rules_resolve_groups_expiry_and_removal() {
    let pearl = "minecraft:ender_pearl";
    assert_eq!(resolve_cooldown_group(None, pearl), pearl, "fallback");
    assert_eq!(
        resolve_cooldown_group(Some("mcrs:x"), pearl),
        "mcrs:x",
        "override"
    );
    for (seconds, ticks) in [(1.0, 20), (0.5, 10), (0.0, 0), (-1.0, 0), (f32::NAN, 0)] {
        assert_eq!(cooldown_ticks(seconds), ticks, "cooldown_ticks({seconds})");
    }

    let mut cooldowns = CooldownMap::with_capacity(2);
    cooldowns.start_cooldown(pearl, 100, 20);
    assert!(cooldowns.is_on_cooldown(pearl, 100), "active at start");
    assert_eq!(cooldowns.remaining_cooldown(pearl, 119), 1, "1 left");
    assert!(!cooldowns.is_on_cooldown(pearl, 120), "expired at end_tick");
    assert_eq!(cooldowns.remaining_cooldown(pearl, 120), 0, "0 left");
    let big = (1_u64 << 30) + 100;
    cooldowns.start_cooldown(pearl, big, 20);
    assert!(cooldowns.remove_cooldown(pearl, big + 10), "remove active");
    assert!(!cooldowns.remove_cooldown(pearl, big + 10), "remove absent");
}

#[test]
fn consume_remainder_rules_cover_survival_and_creative() {
    let bottle = Some("glass_bottle");
    let (survival, creative) = (ConsumeMode::Survival, ConsumeMode::Creative);
    let replace = RemainderOutcome::ReplaceSlot {
        remainder: "glass_bottle",
    };
    let give = RemainderOutcome::DecrementAndGive {
        remaining_count: 15,
        remainder: "glass_bottle",
    };
    let decrement = RemainderOutcome::DecrementSource { remaining_count: 1 };
    for (count, mode, remainder, expected) in [
        (1, survival, bottle, replace),
        (16, survival, bottle, give),
        (16, creative, bottle, RemainderOutcome::Unchanged),
        (2, survival, None, decrement),
        (1, survival, None, RemainderOutcome::EmptySlot),
    ] {
        assert_eq!(
            compute_consume_remainder(count, mode, remainder),
            expected,
            "count={count}"
        );
    }
}

#[test]
fn consume_effect_rules_scale_potions_stew_and_ominous_bottle() {
    for (duration, timing, scale, expected) in [
        (
            INFINITE_EFFECT_DURATION_TICKS,
            EffectTiming::Timed,
            0.25,
            -1,
        ),
        (1, EffectTiming::Instant, 2.0, 1),
        (3600, EffectTiming::Timed, 0.25, 900),
        (2, EffectTiming::Timed, 0.1, 1),
    ] {
        assert_eq!(
            scale_potion_duration(duration, timing, scale),
            expected,
            "scale {duration}"
        );
    }

    let instance = |effect, duration, timing, amplifier, show_particles| {
        let mut built = ConsumableEffectInstance::new(effect, duration, timing);
        built.amplifier = amplifier;
        built.show_particles = show_particles;
        built
    };
    let mut collected = [None; 2];
    let mut index = 0;
    collect_potion_effects(
        [instance(1_u16, 1800, EffectTiming::Timed, 0, true)],
        [instance(2_u16, 1, EffectTiming::Instant, 1, false)],
        0.25,
        |effect| {
            if let Some(slot) = collected.get_mut(index) {
                *slot = Some(effect);
            }
            index += 1;
        },
    );
    assert_eq!(
        (
            index,
            collected[0].map(|item| item.duration),
            collected[1].map(|item| item.duration)
        ),
        (2, Some(450), Some(1)),
        "potion scaling"
    );

    let mut stew_output = None;
    collect_stew_effects([StewEffectEntry::new(7_u16, 160)], |effect| {
        stew_output = Some(effect);
    });
    assert_eq!(
        stew_output,
        Some(AppliedConsumeEffect::new(7_u16, 160)),
        "stew effect"
    );
    let mut expected_omen =
        AppliedConsumeEffect::new(OMINOUS_BOTTLE_EFFECT_ID, OMINOUS_BOTTLE_DURATION_TICKS);
    expected_omen.amplifier = 4;
    expected_omen.show_particles = false;
    assert_eq!(
        ominous_bottle_effect(9),
        expected_omen,
        "ominous bottle bad_omen"
    );
}

#[test]
fn hot_cooldown_remainder_and_effect_paths_allocate_zero_bytes() {
    let (pearl, wind, reused) = (
        "minecraft:ender_pearl",
        "minecraft:wind_charge",
        "mcrs:reused",
    );
    let mut cooldowns = CooldownMap::with_capacity(2);
    cooldowns.start_cooldown(pearl, 10, 20);
    cooldowns.start_cooldown(wind, 10, 10);
    let potion = [ConsumableEffectInstance::new(
        5_u16,
        1200,
        EffectTiming::Timed,
    )];
    let stew = [StewEffectEntry::new(9_u16, 160)];

    let before = allocations();
    let _group = resolve_cooldown_group(None, pearl);
    let _ticks = cooldown_ticks(1.5);
    assert!(cooldowns.is_on_cooldown(pearl, 15), "pearl active");
    assert_eq!(cooldowns.remaining_cooldown(wind, 15), 5, "wind 5 ticks");
    cooldowns.start_cooldown(pearl, 16, 40);
    cooldowns.start_cooldown(reused, 25, 15);
    assert!(cooldowns.is_on_cooldown(reused, 25), "reused active");
    assert!(!cooldowns.is_on_cooldown(wind, 25), "expired evicted");
    cooldowns.start_cooldown(reused, 26, 0);
    let _ = cooldowns.iter_active(26).count();
    let _ = compute_consume_remainder(4, ConsumeMode::Survival, Some(1_u8));
    collect_potion_effects(potion, [], DEFAULT_POTION_DURATION_SCALE, |_| {});
    collect_stew_effects(stew, |_| {});
    let _ = ominous_bottle_effect(2);
    cooldowns.clear();
    assert_eq!(
        allocations() - before,
        0,
        "hot cooldown, slot reuse after expiry, remainder, and effect helpers allocate zero bytes"
    );
}
