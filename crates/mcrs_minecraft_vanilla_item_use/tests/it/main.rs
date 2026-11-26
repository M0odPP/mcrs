#![expect(clippy::unwrap_used, reason = "tests panic on assertion failures")]

use bevy_app::{App, FixedPostUpdate, FixedUpdate};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::schedule::{LogLevel, ScheduleBuildSettings, Schedules, SingleThreadedExecutor};
use bevy_ecs::world::{Mut, World};
use mcrs_minecraft_core::{Direction, Look};
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload, PacketTarget};
use mcrs_minecraft_engine::loaded_client::UnloadedClient;
use mcrs_minecraft_engine::persisted_registry::{PersistedRegistry, PersistedSnapshot};
use mcrs_minecraft_engine::player_sprinting::PlayerSprinting;
use mcrs_minecraft_engine::player_state::{Effect, PlayerFileState};
use mcrs_minecraft_engine::{LivingFlags, UsingItem};
use mcrs_minecraft_engine_damage::DamageSet;
use mcrs_minecraft_engine_item::{
    ItemCommandsExt, ItemPersistencePlugin, ItemPlugin, ItemSet, Selected, Slots, StackCommandsExt,
};
use mcrs_minecraft_engine_registries::test_item_table;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_engine_world::world::dimension::DimensionPlugin;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_registries::registry_load::test_registries;
use mcrs_minecraft_inventory_player::OFF_HAND;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{
    Consumable, ConsumeEffect, Count, ItemComponent, ItemStack, Kind, UseAnimation,
};
use mcrs_minecraft_item_table::ItemTable;
use mcrs_minecraft_network::event::Received;
use mcrs_minecraft_protocol::packets::table::Row;
use mcrs_minecraft_protocol::{Encode, VarInt};
use mcrs_minecraft_protocol_game::Hand;
use mcrs_minecraft_protocol_game::clientbound::{ClientboundEntityEvent, ClientboundSound};
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{
    ServerboundPlayerAction, ServerboundSetCarriedItem, ServerboundUseItem,
};
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_vanilla_inventory::{InventoryPersistencePlugin, VanillaInventoryPlugin};
use mcrs_minecraft_vanilla_item_use::{
    CarriedSlotChanged, ReleaseUseItem, StartUseItem, VanillaItemUsePlugin,
};
use mcrs_test_alloc::{CountingAllocator, allocations};
use mcrs_voxel::BlockPos;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn setup_app() -> App {
    use bevy_ecs::schedule::IntoScheduleConfigs;
    let mut app = App::new();
    let ambiguity_settings = ScheduleBuildSettings {
        ambiguity_detection: LogLevel::Error,
        ..Default::default()
    };
    let registries = test_registries().clone();
    app.init_resource::<PersistedRegistry>()
        .insert_resource(CurrentRegistries::new(registries))
        .insert_resource(test_item_table())
        .insert_resource(bevy_time::TimeUpdateStrategy::FixedTimesteps(1))
        .add_plugins((
            bevy_time::TimePlugin,
            DimensionPlugin,
            ItemPersistencePlugin,
            InventoryPersistencePlugin,
            ItemPlugin::default(),
            VanillaInventoryPlugin,
            mcrs_minecraft_engine::loaded_client::LoadedClientPlugin,
            mcrs_minecraft_engine::swing::PlayerSwingPlugin,
            VanillaItemUsePlugin,
        ));
    app.configure_sets(FixedUpdate, ItemSet.in_set(TickSet::Simulate));
    app.configure_sets(FixedUpdate, DamageSet::Collect.in_set(TickSet::WorldWrite));
    app.configure_schedules(ambiguity_settings);
    app.finish();
    app.cleanup();
    for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
        schedule.set_executor(SingleThreadedExecutor::default());
    }
    app
}

fn spawn_player(app: &mut App, id: u64, position_x: f64, mode: GameMode, loaded: bool) -> Entity {
    let player = app
        .world_mut()
        .spawn((
            Player,
            Owner(PlayerSession(id)),
            Transform::from_xyz(position_x, -60.0, 0.5),
            PlayerGameMode(mode),
            Vitals::new(20.0),
            LivingFlags::default(),
            PlayerSprinting(false),
            PlayerFileState::default(),
        ))
        .id();
    let snapshot = PersistedSnapshot::new();
    let apply = |world: &mut World, registry: Mut<PersistedRegistry>| {
        registry.apply(world, player, &snapshot)
    };
    app.world_mut().resource_scope(apply).unwrap();
    if loaded {
        app.world_mut()
            .entity_mut(player)
            .remove::<UnloadedClient>();
    }
    app.update();
    player
}

fn put(app: &mut App, player: Entity, slot_index: u16, item: Item, count: u8) -> Entity {
    let stack = ItemStack::new(Kind(item.id().number()), Count::new(count).unwrap());
    let entity = app
        .world_mut()
        .commands()
        .spawn_stack(stack)
        .put_at(player, slot_index)
        .id();
    app.update();
    entity
}

fn set_item<T: ItemComponent + Component>(app: &mut App, entity: Entity, value: T) {
    app.world_mut().commands().entity(entity).set(value);
    app.update();
}

fn step_fixed(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.world_mut().run_schedule(FixedUpdate);
        app.world_mut().run_schedule(FixedPostUpdate);
    }
}

fn drain_packets(app: &mut App) -> Vec<OutboundPlayerPacket> {
    app.world_mut()
        .resource_mut::<Messages<OutboundPlayerPacket>>()
        .drain()
        .collect()
}

fn sound_events(packets: &[OutboundPlayerPacket]) -> Vec<(Vec<PlayerSession>, ClientboundSound)> {
    packets
        .iter()
        .filter_map(|packet| match (&packet.target, &packet.data) {
            (PacketTarget::PlayerSet(targets), PacketPayload::Sound(sound)) => {
                Some((targets.to_vec(), sound.clone()))
            }
            _ => None,
        })
        .collect()
}

fn entity_events(packets: &[OutboundPlayerPacket]) -> Vec<(PlayerSession, ClientboundEntityEvent)> {
    packets
        .iter()
        .filter_map(|packet| match (packet.target.clone(), &packet.data) {
            (PacketTarget::SinglePlayer(session), PacketPayload::EntityEvent(event)) => {
                Some((session, event.clone()))
            }
            _ => None,
        })
        .collect()
}

fn living_flags(app: &App, player: Entity) -> u8 {
    app.world().get::<LivingFlags>(player).unwrap().0
}

#[test]
fn schedule_sets_have_no_ambiguity() {
    let mut app = setup_app();
    let labels: Vec<_> = app
        .world()
        .resource::<Schedules>()
        .iter()
        .map(|(_, schedule)| schedule.label())
        .collect();
    assert!(
        !labels.is_empty(),
        "test app must register schedules before ambiguity verification"
    );
    for label in labels {
        let outcome = app
            .world_mut()
            .try_schedule_scope(label, |world, schedule| {
                let settings = schedule.get_build_settings();
                assert_eq!(
                    settings.ambiguity_detection,
                    LogLevel::Error,
                    "schedule {label:?} must configure LogLevel::Error ambiguity detection"
                );
                schedule
                    .initialize(world)
                    .map_err(|error| error.to_string(schedule.graph(), world))
            });
        let initialized = outcome.expect("registered schedule must exist in Schedules");
        assert!(
            initialized.is_ok(),
            "schedule {label:?} has conflicting unordered systems: {:?}",
            initialized.err()
        );
    }
    step_fixed(&mut app, 1);
}

#[test]
fn shield_start_hold_release_and_offhand_bits_match_spec() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Survival, true);
    let _shield_main = put(&mut app, alice, 0, Item::Shield, 1);
    let _shield_off = put(&mut app, alice, OFF_HAND, Item::Shield, 1);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 1);
    assert_eq!(living_flags(&app, alice), 1, "mainhand start sets bit 0");
    assert!(
        app.world().get::<UsingItem>(alice).is_some(),
        "UsingItem should be active immediately after mainhand start"
    );

    step_fixed(&mut app, 4);
    assert_eq!(
        app.world().get::<UsingItem>(alice).unwrap().0.elapsed_ticks,
        5,
        "hold increments elapsed_ticks once per tick"
    );

    app.world_mut()
        .write_message(ReleaseUseItem { player: alice });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "UsingItem should be removed after mainhand release"
    );
    assert_eq!(
        living_flags(&app, alice),
        0,
        "mainhand release clears bit 0"
    );

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Off,
        sequence: VarInt(2),
    });
    step_fixed(&mut app, 1);
    assert_eq!(
        living_flags(&app, alice),
        3,
        "offhand start sets bits 0 and 1"
    );

    app.world_mut().write_message(CarriedSlotChanged {
        player: alice,
        slot: 2,
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_some(),
        "hotbar slot change does not cancel offhand use"
    );

    app.world_mut()
        .write_message(ReleaseUseItem { player: alice });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "UsingItem should be removed after offhand release"
    );
    assert_eq!(
        living_flags(&app, alice),
        2,
        "offhand release clears bit 0 and preserves bit 1"
    );

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(3),
    });
    let _other_shield = put(&mut app, alice, 2, Item::Shield, 1);
    step_fixed(&mut app, 1);
    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(4),
    });
    step_fixed(&mut app, 1);
    assert_eq!(
        living_flags(&app, alice),
        1,
        "mainhand start after offhand clears bit 1"
    );
}

#[test]
fn hotbar_slot_change_cancels_mainhand_without_consuming_or_sounding() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Survival, true);
    let _bob = spawn_player(&mut app, 2, 2.5, GameMode::Survival, true);
    let golden_apple = put(&mut app, alice, 0, Item::GoldenApple, 2);
    let _stick = put(&mut app, alice, 1, Item::Stick, 1);
    let _ = drain_packets(&mut app);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 3);
    assert_eq!(
        living_flags(&app, alice),
        1,
        "mainhand golden apple sets living flags bit 0"
    );

    app.world_mut().write_message(CarriedSlotChanged {
        player: alice,
        slot: 1,
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "hotbar slot change should remove UsingItem on mainhand cancel"
    );
    assert_eq!(
        living_flags(&app, alice),
        0,
        "hotbar slot cancel should clear living flags bit 0"
    );
    assert_eq!(
        app.world().get::<Count>(golden_apple).unwrap().get(),
        2,
        "slot cancel preserves consumable count"
    );
    let packets = drain_packets(&mut app);
    assert!(
        sound_events(&packets).is_empty(),
        "slot cancel emits no sounds"
    );
    assert!(
        entity_events(&packets).is_empty(),
        "slot cancel emits no USE_ITEM_FINISH"
    );
}

#[test]
fn apple_respects_food_gate_and_golden_apple_completes_with_sounds_and_effects() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Survival, true);
    let bob = spawn_player(&mut app, 2, 2.5, GameMode::Survival, true);
    let bob_session = app.world().get::<Owner>(bob).unwrap().0;
    let _apple = put(&mut app, alice, 0, Item::Apple, 1);
    let _golden_apple = put(&mut app, alice, 1, Item::GoldenApple, 1);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "apple cannot start at full food in survival"
    );
    assert_eq!(
        living_flags(&app, alice),
        0,
        "blocked apple use should leave living flags cleared"
    );

    app.world_mut().entity_mut(alice).insert(Selected(1));
    let _ = drain_packets(&mut app);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(2),
    });
    step_fixed(&mut app, 32);

    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "completed golden apple removes UsingItem"
    );
    assert_eq!(
        living_flags(&app, alice),
        0,
        "completed golden apple clears living flags"
    );
    assert!(
        app.world().get::<Slots>(alice).unwrap().get(1).is_none(),
        "single golden apple is consumed at 32 ticks"
    );
    assert_eq!(
        app.world().get::<Vitals>(alice).unwrap().absorption,
        4.0,
        "golden apple absorption effect updates Vitals::absorption"
    );
    assert_eq!(
        app.world()
            .get::<PlayerFileState>(alice)
            .unwrap()
            .active_effects
            .len(),
        2,
        "golden apple applies regeneration and absorption"
    );

    let packets = drain_packets(&mut app);
    let finishes = entity_events(&packets);
    assert_eq!(
        finishes.len(),
        1,
        "actor receives one ClientboundEntityEvent on completion"
    );
    assert_eq!(
        finishes[0].0,
        PlayerSession(1),
        "completion event must target actor session 1"
    );
    assert_eq!(
        finishes[0].1.entity_status,
        ClientboundEntityEvent::USE_ITEM_FINISH,
        "completion entity status must equal USE_ITEM_FINISH"
    );

    let sounds = sound_events(&packets);
    assert_eq!(
        sounds.len(),
        9,
        "32-tick golden apple emits 7 tick eat sounds + 1 finish eat sound + 1 burp = 9 sounds"
    );
    for (recipients, _) in &sounds {
        assert_eq!(
            recipients.as_slice(),
            [bob_session],
            "only nearby viewers receive eat/burp sounds"
        );
    }
}

#[test]
fn creative_mode_eats_at_full_food_and_preserves_item_and_zero_tick_consumable_finishes_now() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Creative, true);
    let apple = put(&mut app, alice, 0, Item::Apple, 1);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 32);
    assert_eq!(
        app.world().get::<Count>(apple).unwrap().get(),
        1,
        "creative mode preserves apple count after completion"
    );
    assert_eq!(
        entity_events(&drain_packets(&mut app)).len(),
        1,
        "creative consumable still emits USE_ITEM_FINISH"
    );

    let instant_apple = put(&mut app, alice, 1, Item::Apple, 1);
    set_item(
        &mut app,
        instant_apple,
        Consumable {
            consume_seconds: 0.0,
            animation: UseAnimation::Eat,
            ..Consumable::new()
        },
    );
    app.world_mut().entity_mut(alice).insert(Selected(1));
    let _ = drain_packets(&mut app);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(2),
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "0-tick consumable finishes immediately without active use"
    );
    assert_eq!(
        living_flags(&app, alice),
        0,
        "0-tick consumable completion keeps living flags cleared"
    );
    assert_eq!(
        entity_events(&drain_packets(&mut app)).len(),
        1,
        "0-tick consumable emits USE_ITEM_FINISH"
    );
}

#[test]
fn inert_unloaded_dead_spectator_and_stack_swap_rules_enforced() {
    let mut app = setup_app();
    let unloaded = spawn_player(&mut app, 1, 0.5, GameMode::Survival, false);
    let _unloaded_shield = put(&mut app, unloaded, 0, Item::Shield, 1);
    app.world_mut().write_message(StartUseItem {
        player: unloaded,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(unloaded).is_none(),
        "unloaded player cannot start item use"
    );

    let spectator = spawn_player(&mut app, 2, 0.5, GameMode::Spectator, true);
    let _spectator_shield = put(&mut app, spectator, 0, Item::Shield, 1);
    app.world_mut().write_message(StartUseItem {
        player: spectator,
        hand: Hand::Main,
        sequence: VarInt(2),
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(spectator).is_none(),
        "spectator player cannot start item use"
    );

    let dead = spawn_player(&mut app, 3, 0.5, GameMode::Survival, true);
    app.world_mut().get_mut::<Vitals>(dead).unwrap().health = 0.0;
    let _dead_shield = put(&mut app, dead, 0, Item::Shield, 1);
    app.world_mut().write_message(StartUseItem {
        player: dead,
        hand: Hand::Main,
        sequence: VarInt(3),
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(dead).is_none(),
        "dead player cannot start item use"
    );

    let alice = spawn_player(&mut app, 4, 0.5, GameMode::Survival, true);
    let _stick = put(&mut app, alice, 0, Item::Stick, 1);
    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(4),
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "non-usable stick cannot start item use"
    );

    let _shield = put(&mut app, alice, 0, Item::Shield, 1);
    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(5),
    });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_some(),
        "usable shield should activate UsingItem"
    );

    let _replaced = put(&mut app, alice, 0, Item::Stick, 1);
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "replacing active slot item kind cancels active use"
    );
    assert_eq!(
        living_flags(&app, alice),
        0,
        "replacing active slot clears living flags"
    );
}

#[test]
fn continuous_shield_holds_past_72000_ticks_without_sounds_or_completion() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Survival, true);
    let _bob = spawn_player(&mut app, 2, 2.5, GameMode::Survival, true);
    let shield = put(&mut app, alice, 0, Item::Shield, 1);
    let _ = drain_packets(&mut app);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 5);
    app.world_mut()
        .get_mut::<UsingItem>(alice)
        .expect("shield must be active")
        .0
        .elapsed_ticks = 71_998;

    step_fixed(&mut app, 7);
    let active = app
        .world()
        .get::<UsingItem>(alice)
        .expect("continuous shield must remain active past 72_000 ticks");
    assert_eq!(
        active.0.elapsed_ticks, 72_005,
        "continuous shield continues incrementing elapsed ticks past 72_000"
    );
    assert_eq!(
        living_flags(&app, alice),
        1,
        "continuous shield keeps living flags bit 0 active past 72_000 ticks"
    );
    assert_eq!(
        app.world().get::<Count>(shield).unwrap().get(),
        1,
        "continuous shield is never consumed past 72_000 ticks"
    );
    let packets = drain_packets(&mut app);
    assert!(
        sound_events(&packets).is_empty(),
        "continuous shield emits zero consume tick or completion sounds"
    );
    assert!(
        entity_events(&packets).is_empty(),
        "continuous shield emits zero USE_ITEM_FINISH events"
    );
}

#[test]
fn same_tick_start_release_and_stop_before_start_coalesce_cleanly() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Survival, true);
    let _shield_slot_zero = put(&mut app, alice, 0, Item::Shield, 1);
    let _shield_slot_one = put(&mut app, alice, 1, Item::Shield, 1);
    app.world_mut()
        .entity_mut(alice)
        .insert(PlayerSprinting(true));

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    app.world_mut()
        .write_message(ReleaseUseItem { player: alice });
    step_fixed(&mut app, 1);
    assert!(
        app.world().get::<UsingItem>(alice).is_none(),
        "same-tick start + release must leave UsingItem absent"
    );
    assert_eq!(
        living_flags(&app, alice),
        0,
        "same-tick start + release must leave LivingFlags unchanged at 0"
    );
    assert!(
        app.world().get::<PlayerSprinting>(alice).unwrap().0,
        "same-tick start + release must not clear PlayerSprinting"
    );

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(2),
    });
    step_fixed(&mut app, 1);
    assert_eq!(
        app.world().get::<UsingItem>(alice).unwrap().0.slot,
        0,
        "initial start activates slot 0"
    );

    app.world_mut().entity_mut(alice).insert(Selected(1));
    app.world_mut().write_message(CarriedSlotChanged {
        player: alice,
        slot: 1,
    });
    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(3),
    });
    step_fixed(&mut app, 1);
    assert_eq!(
        app.world().get::<UsingItem>(alice).unwrap().0.slot,
        1,
        "stop before start replaces slot 0 use with slot 1 on the same tick"
    );
}

#[test]
fn steady_state_shield_hold_ticks_allocate_zero_bytes() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Survival, true);
    let _shield = put(&mut app, alice, 0, Item::Shield, 1);

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 2);

    let before = allocations();
    for _ in 0..10 {
        app.world_mut().run_schedule(FixedUpdate);
        app.world_mut().run_schedule(FixedPostUpdate);
    }
    let delta = allocations() - before;
    assert_eq!(
        delta, 0,
        "steady-state FixedUpdate item-use hold ticks must allocate zero bytes, got {delta}"
    );
}

fn send_packet<P: Row + Encode>(world: &mut World, entity: Entity, packet: &P) {
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    world.trigger(Received::<P>::new(entity, bytes.into()));
}

#[test]
fn observers_emit_start_release_and_valid_slot_change_messages() {
    let mut app = App::new();
    app.add_plugins(VanillaItemUsePlugin);
    let player = app.world_mut().spawn_empty().id();

    send_packet(
        app.world_mut(),
        player,
        &ServerboundUseItem {
            hand: Hand::Off,
            sequence: VarInt(4),
            look: Look::default(),
        },
    );
    send_packet(
        app.world_mut(),
        player,
        &ServerboundPlayerAction {
            action: PlayerAction::Stab,
            pos: BlockPos::new(0, 0, 0),
            direction: Direction::Up,
            sequence: VarInt(1),
        },
    );
    send_packet(
        app.world_mut(),
        player,
        &ServerboundPlayerAction {
            action: PlayerAction::ReleaseUseItem,
            pos: BlockPos::new(0, 0, 0),
            direction: Direction::Down,
            sequence: VarInt(2),
        },
    );
    send_packet(
        app.world_mut(),
        player,
        &ServerboundSetCarriedItem { slot: 9 },
    );
    send_packet(
        app.world_mut(),
        player,
        &ServerboundSetCarriedItem { slot: 3 },
    );

    let starts: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<StartUseItem>>()
        .drain()
        .collect();
    let releases: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<ReleaseUseItem>>()
        .drain()
        .collect();
    let slots: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<CarriedSlotChanged>>()
        .drain()
        .collect();
    assert_eq!(
        starts,
        [StartUseItem {
            player,
            hand: Hand::Off,
            sequence: VarInt(4),
        }],
        "offhand ServerboundUseItem observer must emit StartUseItem"
    );
    assert_eq!(
        releases,
        [ReleaseUseItem { player }],
        "ReleaseUseItem action observer must emit ReleaseUseItem"
    );
    assert_eq!(
        slots,
        [CarriedSlotChanged { player, slot: 3 }],
        "valid hotbar slot observer must emit CarriedSlotChanged"
    );
}

#[test]
fn infinite_effect_duration_and_normalized_id_rules_apply() {
    let mut app = setup_app();
    let alice = spawn_player(&mut app, 1, 0.5, GameMode::Creative, true);
    let _golden_apple = put(&mut app, alice, 0, Item::GoldenApple, 2);

    app.world_mut()
        .get_mut::<PlayerFileState>(alice)
        .unwrap()
        .active_effects
        .push(Effect {
            id: "regeneration".to_owned(),
            amplifier: 1,
            duration: 50,
            ambient: false,
            show_particles: true,
            show_icon: Some(true),
            hidden_effect: None,
        });

    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(1),
    });
    step_fixed(&mut app, 32);
    let after_finite = &app
        .world()
        .get::<PlayerFileState>(alice)
        .unwrap()
        .active_effects;
    assert_eq!(
        after_finite.len(),
        2,
        "un-namespaced regeneration must match minecraft:regeneration without duplicating entries"
    );
    assert_eq!(
        after_finite[0].duration, 100,
        "normalized regeneration effect must upgrade finite duration 50 to 100"
    );

    let mut infinite_consumable = app
        .world()
        .resource::<ItemTable>()
        .prototype(Kind(Item::GoldenApple.id().number()))
        .unwrap()
        .get::<Consumable>()
        .unwrap()
        .clone();
    infinite_consumable.consume_seconds = 0.0;
    for effect in &mut infinite_consumable.on_consume_effects {
        if let ConsumeEffect::ApplyEffects { effects, .. } = effect {
            for instance in effects {
                instance.duration = -1;
            }
        }
    }
    let infinite_apple = put(&mut app, alice, 1, Item::GoldenApple, 1);
    set_item(&mut app, infinite_apple, infinite_consumable);
    app.world_mut().entity_mut(alice).insert(Selected(1));
    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(2),
    });
    step_fixed(&mut app, 1);
    assert_eq!(
        app.world()
            .get::<PlayerFileState>(alice)
            .unwrap()
            .active_effects[0]
            .duration,
        -1,
        "infinite (-1) duration must replace shorter finite duration 100"
    );

    app.world_mut().entity_mut(alice).insert(Selected(0));
    app.world_mut().write_message(StartUseItem {
        player: alice,
        hand: Hand::Main,
        sequence: VarInt(3),
    });
    step_fixed(&mut app, 32);
    assert_eq!(
        app.world()
            .get::<PlayerFileState>(alice)
            .unwrap()
            .active_effects[0]
            .duration,
        -1,
        "finite duration (100) must preserve existing infinite (-1) duration"
    );
}
