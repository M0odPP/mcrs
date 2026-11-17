use mcrs_minecraft_swing::{EffectStep, LoadedClient, SwingKind, SwingModifiers, SwingTracker};

#[test]
fn haste_conduit_and_mining_fatigue_scale_duration() {
    let base = 6;
    assert_eq!(SwingModifiers::default().effective_duration(base), 6);

    let haste0 = SwingModifiers::from_effects([("minecraft:haste", 0)]);
    assert_eq!(haste0.effective_duration(6), 5);

    let haste2 = SwingModifiers::from_effects([("haste", 2)]);
    assert_eq!(haste2.effective_duration(10), 7);
    assert_eq!(haste2.effective_duration(2), 0);

    let conduit3 = SwingModifiers::from_effects([("minecraft:conduit_power", 3)]);
    assert_eq!(conduit3.effective_duration(10), 6);

    let haste_overrides_fatigue =
        SwingModifiers::from_effects([("minecraft:mining_fatigue", 4), ("minecraft:haste", 0)]);
    assert_eq!(haste_overrides_fatigue.effective_duration(6), 5);

    let conduit_max =
        SwingModifiers::from_effects([("minecraft:haste", 1), ("minecraft:conduit_power", 2)]);
    assert_eq!(conduit_max.effective_duration(10), 7);

    let haste_max =
        SwingModifiers::from_effects([("minecraft:haste", 10), ("minecraft:conduit_power", 2)]);
    assert_eq!(haste_max.effective_duration(24), 13);

    let fatigue1 = SwingModifiers::from_effects([("minecraft:mining_fatigue", 1)]);
    assert_eq!(fatigue1.effective_duration(6), 10);

    let fatigue3 = SwingModifiers::from_effects([("minecraft:mining_fatigue", 3)]);
    assert_eq!(fatigue3.effective_duration(6), 14);

    let fatigue4 = SwingModifiers::from_effects([("minecraft:mining_fatigue", 4)]);
    assert_eq!(fatigue4.effective_duration(6), 16);
}

#[test]
fn tracker_enforces_same_tick_and_half_duration_cooldown() {
    let mut tracker = SwingTracker::default();
    let none = SwingModifiers::default();

    assert_eq!(tracker.try_start(100, 6, none), Some(6));
    assert_eq!(
        tracker.try_start(100, 6, none),
        None,
        "SWG-STA-004: at most one swing per tick"
    );
    assert_eq!(tracker.try_start(101, 6, none), None);
    assert_eq!(tracker.try_start(102, 6, none), None);
    assert_eq!(tracker.try_start(103, 6, none), None);
    assert_eq!(tracker.try_start(104, 6, none), Some(6), "unlocks at T + 4");

    assert_eq!(tracker.try_start(200, 0, none), Some(0));
    assert_eq!(
        tracker.try_start(200, 0, none),
        None,
        "SWG-STA-004: D=0 blocks same tick"
    );
    assert_eq!(
        tracker.try_start(201, 0, none),
        Some(0),
        "SWG-STA-003: D=0 unlocks at T + 1"
    );

    let haste2 = SwingModifiers::from_effects([("minecraft:haste", 2)]);
    assert_eq!(tracker.try_start(300, 10, haste2), Some(7));
    assert_eq!(tracker.try_start(303, 10, none), None);
    assert_eq!(tracker.try_start(304, 10, none), Some(10));

    let fatigue4 = SwingModifiers::from_effects([("minecraft:mining_fatigue", 4)]);
    assert_eq!(tracker.try_start(400, 6, fatigue4), Some(16));
    assert_eq!(tracker.try_start(408, 6, none), None);
    assert_eq!(tracker.try_start(409, 6, none), Some(6));

    let fatigue1 = SwingModifiers::from_effects([("minecraft:mining_fatigue", 1)]);
    assert_eq!(tracker.try_start(500, 6, fatigue1), Some(10));
    assert_eq!(tracker.try_start(505, 6, fatigue1), None);
    assert_eq!(tracker.try_start(506, 6, fatigue1), Some(10));
}

#[test]
fn loaded_client_loads_on_packet_or_60_tick_fallback_and_resets() {
    let mut client = LoadedClient::default();
    assert!(!client.is_loaded());

    for _ in 0..59 {
        client.tick();
        assert!(!client.is_loaded());
    }
    client.tick();
    assert!(client.is_loaded());

    client.reset();
    assert!(!client.is_loaded());
    client.mark_loaded();
    assert!(client.is_loaded());
}

#[test]
fn effect_step_and_swing_kind_names_round_trip() {
    let mut infinite = -1;
    assert_eq!(EffectStep::advance(&mut infinite), EffectStep::Active);
    assert_eq!(infinite, -1);

    let mut timed = 2;
    assert_eq!(EffectStep::advance(&mut timed), EffectStep::Active);
    assert_eq!(timed, 1);
    assert_eq!(EffectStep::advance(&mut timed), EffectStep::Expired);
    assert_eq!(timed, 0);
    assert_eq!(EffectStep::advance(&mut timed), EffectStep::Inactive);

    for kind in SwingKind::ALL {
        assert_eq!(SwingKind::from_name(kind.name()), Some(kind));
    }
    assert_eq!(SwingKind::from_name("WHACK"), None);
}
