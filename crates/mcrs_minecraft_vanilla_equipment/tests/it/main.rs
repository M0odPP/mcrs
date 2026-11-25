#![expect(clippy::unwrap_used, reason = "a test reports a failure by panicking")]

use std::borrow::Cow;

use bevy_app::{App, FixedPostUpdate, FixedUpdate};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::schedule::{LogLevel, ScheduleBuildSettings, Schedules, SingleThreadedExecutor};
use bevy_ecs::world::{Mut, World};
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload, PacketTarget};
use mcrs_minecraft_engine::loaded_client::UnloadedClient;
use mcrs_minecraft_engine::persisted_registry::{PersistedRegistry, PersistedSnapshot};
use mcrs_minecraft_engine::swing::SwingPlayer;
use mcrs_minecraft_engine::{
    OwnEquipment, PlayerEquipmentDelta, PublishPlayerTilesSet, UseItemRequest,
};
use mcrs_minecraft_engine_item::{
    HolderCommandsExt, ItemCommandsExt, Request, RequestId, SlotRef, Slots, StackCommandsExt,
};
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_equipment::EquipSlot;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{
    Count, Damage, EntityTypeId, EntityTypeSet, Equippable, EquippableSlot, ItemComponent,
    ItemStack, Kind,
};
use mcrs_minecraft_player_holders::HandAction;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::Hand;
use mcrs_minecraft_protocol_game::clientbound::ClientboundSound;
use mcrs_minecraft_protocol_game::entity::EquipmentSlot;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_vanilla_inventory::{PlayerInventoryCommandsExt, VanillaInventoryRequests};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

type SndList = Vec<(Vec<PlayerSession>, ClientboundSound)>;
type Events = (Vec<PlayerEquipmentDelta>, SndList, Vec<SwingPlayer>);

fn spawn_at(app: &mut App, id: u64, x: f64, pre: Option<(u16, Item)>, loaded: bool) -> Entity {
    let tr = Transform::from_xyz(x, -60.0, 0.5);
    let mode = PlayerGameMode(GameMode::Survival);
    let base = (
        Player,
        Owner(PlayerSession(id)),
        tr,
        mode,
        Vitals::new(20.0),
    );
    let player = app.world_mut().spawn((base, OwnEquipment::default())).id();
    let snap = PersistedSnapshot::new();
    let apply = |world: &mut World, reg: Mut<PersistedRegistry>| reg.apply(world, player, &snap);
    app.world_mut().resource_scope(apply).unwrap();
    if let Some((slot_idx, item)) = pre {
        put(app, player, slot_idx, item, 1);
    }
    if loaded {
        app.world_mut()
            .entity_mut(player)
            .remove::<UnloadedClient>();
    }
    app.update();
    player
}

fn put(app: &mut App, player: Entity, slot_idx: u16, item: Item, count: u8) -> Entity {
    let stk = ItemStack::new(Kind(item.id().number()), Count::new(count).unwrap());
    let ent = app
        .world_mut()
        .commands()
        .spawn_stack(stk)
        .put_at(player, slot_idx)
        .id();
    app.update();
    ent
}

fn set_item<T: ItemComponent + Component>(app: &mut App, ent: Entity, val: T) {
    app.world_mut().commands().entity(ent).set(val);
    app.update();
}

fn use_ev(app: &mut App, entity: Entity, seq: i32) -> Events {
    let _ = take_events(app);
    let req = UseItemRequest {
        entity,
        hand: Hand::Main,
        sequence: VarInt(seq),
        yaw: 0.0,
        pitch: 0.0,
    };
    app.world_mut().write_message(req);
    app.update();
    take_events(app)
}

fn has(app: &App, player: Entity, eq_slot: EquipSlot) -> bool {
    app.world()
        .get::<OwnEquipment>(player)
        .unwrap()
        .0
        .get(eq_slot)
        .is_some()
}

fn is_item(app: &App, player: Entity, eq_slot: EquipSlot, item: Item, count: i32) -> bool {
    let own = &app.world().get::<OwnEquipment>(player).unwrap().0;
    own.get(eq_slot)
        .is_some_and(|stk| stk.id == item.id().into() && stk.count == count)
}

fn take_events(app: &mut App) -> Events {
    let world = app.world_mut();
    let deltas = world
        .resource_mut::<Messages<PlayerEquipmentDelta>>()
        .drain()
        .collect();
    let mut pkts = world.resource_mut::<Messages<OutboundPlayerPacket>>();
    let sounds = pkts
        .drain()
        .filter_map(|pkt| match (&pkt.target, pkt.data) {
            (PacketTarget::PlayerSet(tgts), PacketPayload::Sound(snd)) => {
                Some((tgts.to_vec(), snd))
            }
            _ => None,
        })
        .collect();
    let swings = world
        .resource_mut::<Messages<SwingPlayer>>()
        .drain()
        .collect();
    (deltas, sounds, swings)
}

fn silent(ev: Events) -> bool {
    ev.0.is_empty() && ev.1.is_empty() && ev.2.is_empty()
}

fn check_slot(app: &mut App, eq_slot: EquipmentSlot, count: i32, patched: bool) {
    let (deltas, sounds, _) = take_events(app);
    assert!(sounds.is_empty(), "{eq_slot:?} zero sounds");
    let ok = matches!(deltas.as_slice(), [PlayerEquipmentDelta::Slots { slots, .. }] if matches!(slots.as_slice(), [(slot, stk)] if *slot == eq_slot && stk.count == count && (!patched || !stk.components.added.is_empty())));
    assert!(ok, "{eq_slot:?} mismatch: {deltas:?}");
}

fn step_fixed(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.world_mut().run_schedule(FixedUpdate);
        app.world_mut().run_schedule(FixedPostUpdate);
    }
}

fn setup_app() -> App {
    use bevy_ecs::schedule::IntoScheduleConfigs;
    let mut app = App::new();
    let err = ScheduleBuildSettings {
        ambiguity_detection: LogLevel::Error,
        ..Default::default()
    };
    let regs = mcrs_minecraft_game_registries::registry_load::test_registries().clone();
    app.configure_schedules(err)
        .init_resource::<PersistedRegistry>()
        .insert_resource(CurrentRegistries::new(regs))
        .insert_resource(mcrs_minecraft_engine_registries::test_item_table())
        .insert_resource(bevy_time::TimeUpdateStrategy::FixedTimesteps(1))
        .add_plugins((
            bevy_time::TimePlugin,
            mcrs_minecraft_engine_world::world::dimension::DimensionPlugin,
            mcrs_minecraft_engine_item::ItemPersistencePlugin,
            mcrs_minecraft_vanilla_inventory::InventoryPersistencePlugin,
            mcrs_minecraft_engine_item::ItemPlugin::default(),
            mcrs_minecraft_vanilla_inventory::VanillaInventoryPlugin,
            mcrs_minecraft_engine::loaded_client::LoadedClientPlugin,
            mcrs_minecraft_engine::swing::PlayerSwingPlugin,
            mcrs_minecraft_vanilla_equipment::VanillaEquipmentPlugin,
        ));
    let sim = mcrs_minecraft_engine_world::tick::TickSet::Simulate;
    let net = mcrs_minecraft_engine_world::voxel_update::VoxelUpdateSet::NetworkSync;
    app.configure_sets(FixedUpdate, mcrs_minecraft_engine_item::ItemSet.in_set(sim));
    app.configure_sets(FixedPostUpdate, net.before(PublishPlayerTilesSet));
    app.finish();
    app.cleanup();
    for (_, sched) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
        sched.set_executor(SingleThreadedExecutor::default());
    }
    app.update();
    app
}

#[test]
fn equipment_plugin_sync_sounds_creative_and_suppression() {
    let mut app = setup_app();
    let alice = spawn_at(&mut app, 10, 0.5, None, true);
    let _far = spawn_at(&mut app, 12, 200.5, None, true);
    let bob = spawn_at(&mut app, 11, 2.5, Some((39, Item::IronHelmet)), true);
    let join_ok = take_events(&mut app).1.is_empty() && has(&app, bob, EquipSlot::Head);
    assert!(join_ok, "pre-equipped join must seed Head with zero sounds");
    let head = app.world().get::<Slots>(bob).unwrap().get(39).unwrap();
    step_fixed(&mut app, 2);
    let before = allocations();
    step_fixed(&mut app, 10);
    assert_eq!(allocations() - before, 0, "idle ticks zero allocs");
    let _ = take_events(&mut app);
    set_item(&mut app, head, Damage(5));
    check_slot(&mut app, EquipmentSlot::Head, 1, true);
    let source = app.world().resource::<VanillaInventoryRequests>().source;
    let clr = Request::new(RequestId { source, serial: 1 }).set(
        SlotRef {
            holder: bob,
            index: 39,
        },
        None,
        0,
    );
    app.world_mut().commands().entity(bob).request(clr);
    app.update();
    check_slot(&mut app, EquipmentSlot::Head, 0, false);
    assert!(!has(&app, bob, EquipSlot::Head), "Head cleared");
    put(&mut app, alice, 0, Item::GoldenHelmet, 1);
    let (deltas, sounds, swings) = use_ev(&mut app, alice, 1);
    let eq_ok = !has(&app, alice, EquipSlot::MainHand) && has(&app, alice, EquipSlot::Head);
    assert!(eq_ok, "use GoldenHelmet moves MainHand to Head");
    let [(tgts, snd)] = sounds.as_slice() else {
        panic!("expected 1 sound: {sounds:?}")
    };
    let tgt = tgts.contains(&PlayerSession(10))
        && tgts.contains(&PlayerSession(11))
        && !tgts.contains(&PlayerSession(12));
    let pos_ok = (snd.x, snd.y, snd.z) == (4, -480, 4);
    assert!(tgt && pos_ok, "sound range/pos");
    let use_ok = matches!(deltas.as_slice(), [PlayerEquipmentDelta::Slots { slots, .. }] if slots.iter().map(|(eq_s, stk)| (*eq_s, stk.count)).eq([(EquipmentSlot::MainHand, 0), (EquipmentSlot::Head, 1)]));
    assert!(use_ok && swings.len() == 1, "MainHand=0 + Head=1 + swing");
    put(&mut app, alice, 0, Item::GoldenHelmet, 1);
    assert!(silent(use_ev(&mut app, alice, 2)), "identical worn no-op");
    let no_snd = put(&mut app, bob, 0, Item::Stone, 1);
    set_item(&mut app, no_snd, Equippable::new(EquippableSlot::Head));
    let (_, snds, swgs) = use_ev(&mut app, bob, 3);
    let silent_eq = has(&app, bob, EquipSlot::Head) && snds.is_empty() && swgs.len() == 1;
    assert!(silent_eq, "equip_sound=None equips silently and swings");
    let sword = put(&mut app, alice, 0, Item::IronSword, 2);
    check_slot(&mut app, EquipmentSlot::MainHand, 2, false);
    app.world_mut().commands().entity(sword).consume(1);
    app.update();
    check_slot(&mut app, EquipmentSlot::MainHand, 1, false);
    set_item(&mut app, sword, Damage(7));
    check_slot(&mut app, EquipmentSlot::MainHand, 1, true);
    put(&mut app, alice, 1, Item::GoldenApple, 1);
    put(&mut app, alice, 9, Item::Stick, 3);
    assert!(silent(take_events(&mut app)), "unselected hotbar/backpack");
    app.world_mut().commands().entity(alice).select_slot(1);
    app.update();
    check_slot(&mut app, EquipmentSlot::MainHand, 1, false);
    app.world_mut().commands().entity(alice).select_slot(1);
    app.update();
    assert!(silent(take_events(&mut app)), "no-op hotbar select");
    put(&mut app, alice, 40, Item::Shield, 1);
    check_slot(&mut app, EquipmentSlot::OffHand, 1, false);
    let mut cmds = app.world_mut().commands();
    cmds.entity(alice).hand_action(HandAction::SwapHands);
    app.update();
    let (swap_d, swap_s, _) = take_events(&mut app);
    let swap_ok = swap_s.is_empty()
        && matches!(swap_d.as_slice(), [PlayerEquipmentDelta::SwapHands { player }] if *player == alice);
    assert!(swap_ok, "HandSwap delta with zero sounds: {swap_d:?}");
    let hands_ok = is_item(&app, alice, EquipSlot::MainHand, Item::Shield, 1)
        && is_item(&app, alice, EquipSlot::OffHand, Item::GoldenApple, 1);
    assert!(hands_ok, "HandSwap exchanges MainHand and OffHand");
    verify_creative_and_suppressed(&mut app, bob);
}

fn verify_creative_and_suppressed(app: &mut App, bob: Entity) {
    app.world_mut().get_mut::<PlayerGameMode>(bob).unwrap().0 = GameMode::Creative;
    let blocked = put(app, bob, 0, Item::Stone, 1);
    let ids = &[EntityTypeId(0)];
    let mut pig = Equippable::new(EquippableSlot::Chest);
    pig.allowed_entities = Some(EntityTypeSet::EntityTypes(Cow::Borrowed(ids)));
    set_item(app, blocked, pig);
    let rej_ok = silent(use_ev(app, bob, 4)) && !has(app, bob, EquipSlot::Chest);
    assert!(rej_ok, "Creative enforces allowed_entities");
    put(app, bob, 0, Item::DiamondHelmet, 2);
    let (_, c_snd, c_swg) = use_ev(app, bob, 5);
    let cr_ok = is_item(app, bob, EquipSlot::Head, Item::DiamondHelmet, 1)
        && is_item(app, bob, EquipSlot::MainHand, Item::DiamondHelmet, 2);
    let all_ok = cr_ok && c_snd.len() == 1 && c_swg.len() == 1;
    assert!(all_ok, "Creative replace");
    let spec = spawn_at(app, 41, 0.5, None, true);
    let dead = spawn_at(app, 42, 0.5, None, true);
    let unloaded = spawn_at(app, 43, 0.5, None, false);
    app.world_mut().get_mut::<PlayerGameMode>(spec).unwrap().0 = GameMode::Spectator;
    let mut vitals = app.world_mut().get_mut::<Vitals>(dead).unwrap();
    (vitals.health, vitals.death_time) = (0.0, Some(1));
    for (seq, actor) in [spec, dead, unloaded].into_iter().enumerate() {
        put(app, actor, 0, Item::IronChestplate, 1);
        let ev = use_ev(app, actor, seq as i32 + 10);
        assert!(
            silent(ev) && !has(app, actor, EquipSlot::Chest),
            "suppressed"
        );
    }
}
