use core::mem::{align_of, size_of};

use mcrs_minecraft_item_use::{
    DEFAULT_CONSUME_SECONDS, DEFAULT_USE_SPEED_MULTIPLIER, ItemUseState, LIVING_FLAG_OFFHAND,
    LIVING_FLAG_SPIN_ATTACK, LIVING_FLAG_USING_ITEM, SHIELD_USE_DURATION_TICKS, SprintPolicy,
    UseHand, UseKind, UseOutcome, UseSpec, cancel_on_slot_change, consume_ticks, living_flags_bits,
    release_use, should_emit_consume_tick_effects, start_use, tick_use,
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
