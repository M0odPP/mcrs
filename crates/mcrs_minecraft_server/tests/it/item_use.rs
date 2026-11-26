use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::system::Res;
use bevy_math::DVec3;
use bevy_time::TimeUpdateStrategy;
use mcrs_minecraft_collision_table::BlockWorld;
use mcrs_minecraft_command_engine::{CommandLimits, CommandPlugin};
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_command_world::WorldCommandsPlugin;
use mcrs_minecraft_core::{Direction, Look};
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::bus::PacketPayload;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::{LivingFlags, UsingItem};
use mcrs_minecraft_engine_item::{ItemCommandsExt, StackCommandsExt};
use mcrs_minecraft_engine_physics::PhysicsTables;
use mcrs_minecraft_engine_registries::{install_item_table, test_item_table};
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::world::dimension::Dimension;
use mcrs_minecraft_engine_world::world::storage::view::WorldView;
use mcrs_minecraft_environment::{AdvanceTime, WorldClockPlugin};
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{Count, ItemStack, Kind};
use mcrs_minecraft_physics::{Keys, NoBodies, PlayerInput, PlayerState, Scratch, View};
use mcrs_minecraft_protocol::packets::table::Decoded;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol::{Bounded, Decode, Packet, VarInt};
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundContainerSetSlot, ClientboundEntityEvent, ClientboundPlayerPosition,
    ClientboundSetEntityData, ClientboundSetEquipment, ClientboundSound,
};
use mcrs_minecraft_protocol_game::entity::MetaDataValue;
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{
    PlayerInputFlags, ServerboundAcceptTeleportation, ServerboundChatCommand,
    ServerboundMovePlayerPos, ServerboundPlayerAction, ServerboundPlayerInput,
    ServerboundPlayerLoaded, ServerboundSetCarriedItem, ServerboundUseItem,
};
use mcrs_minecraft_protocol_game::table::game_clientbound::{ClientboundGamePacket, decode};
use mcrs_minecraft_protocol_game::{Hand, MoveFlags};
use mcrs_minecraft_registries::{CurrentRegistries, Ref, Wire};
use mcrs_minecraft_server::operator_broadcast::OperatorBroadcastPlugin;
use mcrs_minecraft_server::{
    VanillaBlocksPlugin, VanillaEntityAttributesPlugin, VanillaEntityPhysicsPlugin,
    VanillaEquipmentPlugin, VanillaHungerPlugin, VanillaItemEntityPlugin, VanillaItemPlugin,
    VanillaItemUsePlugin, VanillaRespawnPlugin, VanillaSwingPlugin,
};
use mcrs_minecraft_sound::SoundEventValue;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_voxel::BlockPos;
use serde_json::Value;

use crate::commands::load_stone_column_at;
use crate::host_app::{self, PlayServer, replicated, sent_to};

const ALICE: Uuid = Uuid::from_u128(0x0A11_CE01_1700);
const BOB: Uuid = Uuid::from_u128(0x0B0B_0117_0000);
const OFF_HAND_SLOT: u16 = 40;
const TRACE: &str = include_str!("../../../../tools/mcrs_minecraft_probe/traces/item_use.json");

#[derive(Default, Debug)]
struct ObservedPackets {
    rep_tags: Vec<&'static str>,
    own_living_flags: Vec<u8>,
    rep_living_flags: Vec<u8>,
    rep_equipment: Vec<ClientboundSetEquipment>,
    sounds: Vec<ClientboundSound>,
    entity_events: Vec<i8>,
    slot_updates: Vec<ClientboundContainerSetSlot>,
    teleports: Vec<ClientboundPlayerPosition>,
}

fn trace_step<'a>(trace: &'a Value, id: &str) -> &'a Value {
    trace["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("missing trace step {id}"))
}

fn trace_sound_packet_count(step: &Value, client: &str) -> usize {
    step["packets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|packet| {
            packet["client"].as_str() == Some(client) && packet["packet"].as_str() == Some("sound")
        })
        .count()
}

fn start_item_use_server() -> PlayServer {
    let mut app = host_app::make_host_app();
    install_item_table(app.world_mut(), test_item_table()).unwrap();
    app.insert_resource(CommandLimits(mcrs_command::Limits::default()));
    app.add_plugins((
        WorldClockPlugin,
        mcrs_minecraft_engine::coordinator::clock::WorldClockHostPlugin,
        mcrs_minecraft_engine::coordinator::birth_clock::BirthClockPlugin,
        CommandPlugin::<VanillaCommands>::new(),
        VanillaCommandsPlugin,
        WorldCommandsPlugin,
        VanillaBlocksPlugin,
        VanillaEntityPhysicsPlugin,
        VanillaEntityAttributesPlugin,
    ));
    app.add_plugins((
        VanillaItemEntityPlugin,
        VanillaItemPlugin,
        VanillaSwingPlugin,
        VanillaItemUsePlugin,
        VanillaEquipmentPlugin,
        VanillaHungerPlugin,
        VanillaRespawnPlugin,
    ));
    app.insert_resource(AdvanceTime(false));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    mcrs_minecraft_engine_world::world::sub_app::AddDimensionPlugins::add_dimension_plugin(
        &mut app,
        OperatorBroadcastPlugin,
    );
    let mut server = PlayServer::start(app, &[(ALICE, 4)], 0);
    for cx in -1..=1 {
        for cz in -1..=1 {
            load_stone_column_at(&mut server, cx, cz);
        }
    }
    server
}

fn accept_login_teleports(server: &mut PlayServer, anchor: Entity, packets: &[PacketPayload]) {
    for packet in packets {
        if let PacketPayload::PlayerPosition(position) = packet {
            server.send(
                anchor,
                &ServerboundAcceptTeleportation {
                    teleport_id: position.teleport_id,
                    position: position.position.into(),
                    look: position.look,
                },
            );
        }
    }
    server.queue(anchor, &ServerboundPlayerLoaded);
}

fn collect_packets(server: &mut PlayServer, anchor: Entity, ticks: usize) -> ObservedPackets {
    let raw = server.ticks_all(ticks);
    decode_observed(&sent_to(server.session_of(anchor).unwrap(), raw))
}

fn collect_pair(
    server: &mut PlayServer,
    alice: Entity,
    bob: Entity,
    ticks: usize,
) -> (ObservedPackets, ObservedPackets) {
    let raw = server.ticks_all(ticks);
    let alice_packets = decode_observed(&sent_to(server.session_of(alice).unwrap(), raw.clone()));
    let bob_packets = decode_observed(&sent_to(server.session_of(bob).unwrap(), raw));
    (alice_packets, bob_packets)
}

fn decode_observed(payloads: &[PacketPayload]) -> ObservedPackets {
    let mut observed = ObservedPackets::default();
    for payload in payloads {
        match payload {
            PacketPayload::OwnEntityState(bytes) => {
                let mut rest: &[u8] = bytes;
                let id = VarInt::decode(&mut rest).unwrap();
                if id.0 == ClientboundSetEntityData::ID {
                    let data = ClientboundSetEntityData::decode(&mut rest).unwrap();
                    for entry in data.metadata.0 {
                        if entry.index == 8
                            && let MetaDataValue::Byte(value) = entry.value
                        {
                            observed.own_living_flags.push(value as u8);
                        }
                    }
                }
            }
            PacketPayload::Replication(_) => decode_replication(payload, &mut observed),
            PacketPayload::Sound(sound) => observed.sounds.push(sound.clone()),
            PacketPayload::EntityEvent(event) => observed.entity_events.push(event.entity_status),
            PacketPayload::ContainerSetSlot(slot) => observed.slot_updates.push(slot.clone()),
            PacketPayload::PlayerPosition(position) => observed.teleports.push(position.clone()),
            _ => {}
        }
    }
    observed
}

fn decode_replication(payload: &PacketPayload, observed: &mut ObservedPackets) {
    for (id, bytes) in replicated(std::slice::from_ref(payload)) {
        let Ok(Decoded::Packet(packet)) = decode(id, &mut bytes.as_slice()) else {
            continue;
        };
        match packet {
            ClientboundGamePacket::ClientboundSetEntityData(data) => {
                observed.rep_tags.push("set_entity_data");
                extract_entity_data(data, observed);
            }
            ClientboundGamePacket::ClientboundSetEquipment(equipment) => {
                observed.rep_tags.push("set_equipment");
                observed.rep_equipment.push(equipment);
            }
            _ => {}
        }
    }
}

fn extract_entity_data(data: ClientboundSetEntityData<'_>, observed: &mut ObservedPackets) {
    for entry in data.metadata.0 {
        if entry.index == 8
            && let MetaDataValue::Byte(value) = entry.value
        {
            observed.rep_living_flags.push(value as u8);
        }
    }
}

fn put(server: &mut PlayServer, player: Entity, slot: u16, kind: Item, count: u8) -> Entity {
    let stack = ItemStack::new(Kind(kind.id().number()), Count::new(count).unwrap());
    let world = server.world();
    let entity = world
        .commands()
        .spawn_stack(stack)
        .put_at(player, slot)
        .id();
    world.flush();
    entity
}

fn clear_slot(server: &mut PlayServer, player: Entity, slot: u16) {
    let world = server.world();
    let current = world
        .get::<mcrs_minecraft_engine_item::Slots>(player)
        .unwrap()
        .get(usize::from(slot));
    if let Some(stack) = current {
        world.commands().entity(stack).consume(64);
        world.flush();
    }
}

fn use_item(server: &mut PlayServer, anchor: Entity, hand: Hand, sequence: i32) {
    server.queue(
        anchor,
        &ServerboundUseItem {
            hand,
            sequence: VarInt(sequence),
            look: Look::default(),
        },
    );
}

fn release_use(server: &mut PlayServer, anchor: Entity, sequence: i32) {
    server.queue(
        anchor,
        &ServerboundPlayerAction {
            action: PlayerAction::ReleaseUseItem,
            pos: BlockPos::new(0, 0, 0),
            direction: Direction::Up,
            sequence: VarInt(sequence),
        },
    );
}

fn run_command(server: &mut PlayServer, anchor: Entity, command: &str) -> ObservedPackets {
    let raw = server.send(
        anchor,
        &ServerboundChatCommand {
            command: Bounded(command),
        },
    );
    decode_observed(&raw)
}

fn sound_wire_id(
    server: &mut PlayServer,
    event: SoundEvent,
) -> Ref<Wire, SoundEvent, SoundEventValue> {
    let id = server
        .world()
        .resource::<CurrentRegistries>()
        .entries::<SoundEvent>()
        .unwrap()
        .by_name(event.location())
        .unwrap();
    Ref::Id(id.into())
}

#[test]
fn trace_parity_shield_apple_gapple_creative_and_adventure() {
    let trace: Value = serde_json::from_str(TRACE).unwrap();
    let mut server = start_item_use_server();
    let (alice, alice_join_packets) = server.join(ALICE, "Alice");
    let (bob, bob_join_packets) = server.join(BOB, "Bob");
    accept_login_teleports(&mut server, alice, &alice_join_packets);
    accept_login_teleports(&mut server, bob, &bob_join_packets);
    let _ = server.ticks_all(3);
    run_command(&mut server, alice, "gamemode survival @s");
    let alice_entity = server.player(alice);

    phase_mainhand_shield(&mut server, &trace, alice, bob, alice_entity);
    phase_offhand_and_coalesce(&mut server, alice, bob, alice_entity);
    phase_apple_and_slot_cancel(&mut server, alice, bob, alice_entity);
    phase_golden_apple_completion(&mut server, &trace, alice, bob, alice_entity);
    phase_creative_and_adventure(&mut server, alice, bob, alice_entity);
}

fn phase_mainhand_shield(
    server: &mut PlayServer,
    trace: &Value,
    alice: Entity,
    bob: Entity,
    alice_entity: Entity,
) {
    put(server, alice_entity, 0, Item::Shield, 1);
    put(server, alice_entity, 1, Item::Stick, 1);
    let _ = collect_pair(server, alice, bob, 1);

    let start_shield_trace = trace_step(trace, "start_use_main_shield");
    assert_eq!(
        start_shield_trace["entity_data"][0]["entries"][0]["value"].as_u64(),
        Some(1),
        "reference trace should record living_flags=1 on mainhand shield start"
    );
    use_item(server, alice, Hand::Main, 1);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [1],
        "actor gets living_flags = 1"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [1],
        "viewer gets living_flags = 1"
    );
    assert!(
        viewer_packets.sounds.is_empty(),
        "shield start emits no sound"
    );

    let release_shield_trace = trace_step(trace, "release_use_shield");
    assert_eq!(
        release_shield_trace["entity_data"][0]["entries"][0]["value"].as_u64(),
        Some(0),
        "reference trace should record living_flags=0 on shield release"
    );
    release_use(server, alice, 2);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [0],
        "actor clears living_flags -> 0"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [0],
        "viewer clears living_flags -> 0"
    );

    use_item(server, alice, Hand::Main, 3);
    let _ = collect_pair(server, alice, bob, 1);
    server.queue(alice, &ServerboundSetCarriedItem { slot: 1 });
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [0],
        "slot switch stops actor use"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [0],
        "slot switch stops viewer use"
    );
    assert_eq!(
        viewer_packets.rep_tags,
        ["set_entity_data", "set_equipment"],
        "viewer receives set_entity_data before set_equipment on hotbar swap"
    );
    assert_eq!(
        viewer_packets.rep_equipment.len(),
        1,
        "viewer should receive one ClientboundSetEquipment packet after hotbar swap"
    );

    use_item(server, alice, Hand::Main, 4);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert!(
        actor_packets.own_living_flags.is_empty() && viewer_packets.rep_living_flags.is_empty(),
        "non-usable stick must emit no living_flags metadata"
    );
    assert!(
        server.world().get::<UsingItem>(alice_entity).is_none(),
        "non-usable stick must not activate UsingItem"
    );
}

fn phase_offhand_and_coalesce(
    server: &mut PlayServer,
    alice: Entity,
    bob: Entity,
    alice_entity: Entity,
) {
    put(server, alice_entity, OFF_HAND_SLOT, Item::Shield, 1);
    let _ = collect_pair(server, alice, bob, 1);
    use_item(server, alice, Hand::Off, 5);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [3],
        "offhand shield sets bits 0 | 1"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [3],
        "viewer sees offhand shield 3"
    );

    release_use(server, alice, 6);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [2],
        "offhand release clears bit 0 -> 2"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [2],
        "viewer sees offhand release 2"
    );

    use_item(server, alice, Hand::Off, 13);
    release_use(server, alice, 14);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert!(
        actor_packets.own_living_flags.is_empty() && viewer_packets.rep_living_flags.is_empty(),
        "same-tick use and release must emit no living_flags deltas"
    );
    assert!(
        server.world().get::<UsingItem>(alice_entity).is_none(),
        "same-tick use and release leaves item use inactive"
    );
}

fn phase_apple_and_slot_cancel(
    server: &mut PlayServer,
    alice: Entity,
    bob: Entity,
    alice_entity: Entity,
) {
    clear_slot(server, alice_entity, OFF_HAND_SLOT);
    put(server, alice_entity, 0, Item::Apple, 1);
    put(server, alice_entity, 2, Item::GoldenApple, 1);
    server.queue(alice, &ServerboundSetCarriedItem { slot: 0 });
    let _ = collect_pair(server, alice, bob, 1);

    use_item(server, alice, Hand::Main, 7);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert!(
        actor_packets.own_living_flags.is_empty() && viewer_packets.rep_living_flags.is_empty(),
        "apple at full food must not start use"
    );

    server.queue(alice, &ServerboundSetCarriedItem { slot: 2 });
    let _ = collect_pair(server, alice, bob, 1);
    use_item(server, alice, Hand::Main, 8);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [1],
        "golden apple start sets actor living_flags = 1"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [1],
        "golden apple start sets viewer living_flags = 1"
    );
    server.queue(alice, &ServerboundSetCarriedItem { slot: 1 });
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [0],
        "slot swap cancels golden apple and clears actor living_flags -> 0"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [0],
        "slot swap cancels golden apple and clears viewer living_flags -> 0"
    );
    assert!(
        actor_packets.entity_events.is_empty(),
        "cancelled golden apple never finishes"
    );
}

fn phase_golden_apple_completion(
    server: &mut PlayServer,
    trace: &Value,
    alice: Entity,
    bob: Entity,
    alice_entity: Entity,
) {
    server.queue(alice, &ServerboundSetCarriedItem { slot: 2 });
    let _ = collect_pair(server, alice, bob, 1);
    let golden_apple_trace = trace_step(trace, "use_golden_apple_always_eat");
    let expected_bob_sounds = trace_sound_packet_count(golden_apple_trace, "Bob");
    let eat_id = sound_wire_id(server, SoundEvent::EntityGenericEat);
    let burp_id = sound_wire_id(server, SoundEvent::EntityPlayerBurp);

    use_item(server, alice, Hand::Main, 9);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 33);
    assert_eq!(
        actor_packets.own_living_flags,
        [1, 0],
        "actor living_flags transitions 1 on start then 0 on completion"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [1, 0],
        "viewer living_flags transitions 1 on start then 0 on completion"
    );
    assert_eq!(
        actor_packets.entity_events,
        [ClientboundEntityEvent::USE_ITEM_FINISH],
        "actor gets USE_ITEM_FINISH = 9 on completion"
    );
    assert!(
        actor_packets.sounds.is_empty(),
        "actor receives zero consume sounds"
    );
    assert_eq!(
        viewer_packets.sounds.len(),
        expected_bob_sounds,
        "viewer receives exact trace sound count ({expected_bob_sounds})"
    );
    assert_eq!(
        viewer_packets
            .sounds
            .iter()
            .filter(|sound| sound.sound_event == eat_id)
            .count(),
        8,
        "8 eat sounds (7 tick + 1 finish)"
    );
    assert_eq!(
        viewer_packets
            .sounds
            .iter()
            .filter(|sound| sound.sound_event == burp_id)
            .count(),
        1,
        "1 burp sound on food completion"
    );
    assert!(
        actor_packets
            .slot_updates
            .iter()
            .any(|slot| slot.slot == 38 && slot.item.is_empty()),
        "survival golden apple slot 2 is emptied on consume"
    );
    let world = server.world();
    assert!(
        (world.get::<Vitals>(alice_entity).unwrap().absorption - 4.0).abs() < 1e-6,
        "golden apple grants absorption = 4.0"
    );
    assert_eq!(
        world
            .get::<PlayerFileState>(alice_entity)
            .unwrap()
            .active_effects
            .len(),
        2,
        "golden apple grants two active status effects"
    );
}

fn phase_creative_and_adventure(
    server: &mut PlayServer,
    alice: Entity,
    bob: Entity,
    alice_entity: Entity,
) {
    run_command(server, alice, "gamemode creative @s");
    server.queue(alice, &ServerboundSetCarriedItem { slot: 0 });
    let _ = collect_pair(server, alice, bob, 1);
    use_item(server, alice, Hand::Main, 10);
    let (actor_packets, _) = collect_pair(server, alice, bob, 33);
    assert_eq!(
        actor_packets.entity_events,
        [ClientboundEntityEvent::USE_ITEM_FINISH],
        "creative eats apple even at full food"
    );
    let apple_still_there = server
        .world()
        .get::<mcrs_minecraft_engine_item::Slots>(alice_entity)
        .unwrap()
        .get(0)
        .and_then(|stack| server.world().get::<Count>(stack).copied())
        .map(|count| count.get());
    assert_eq!(apple_still_there, Some(1), "creative eat keeps apple");

    run_command(server, alice, "gamemode adventure @s");
    put(server, alice_entity, 0, Item::Shield, 1);
    let _ = collect_pair(server, alice, bob, 1);
    use_item(server, alice, Hand::Main, 11);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [1],
        "adventure shield start sets 1"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [1],
        "viewer sees adventure shield start 1"
    );
    release_use(server, alice, 12);
    let (actor_packets, viewer_packets) = collect_pair(server, alice, bob, 1);
    assert_eq!(
        actor_packets.own_living_flags,
        [0],
        "adventure shield release sets 0"
    );
    assert_eq!(
        viewer_packets.rep_living_flags,
        [0],
        "viewer sees adventure shield release 0"
    );
}

fn expected_using_step(
    server: &mut PlayServer,
    from: PlayerState,
    using_spec: mcrs_minecraft_physics::ItemUse,
) -> PlayerState {
    let mut expected_state = from;
    expected_state.item_use = using_spec;
    let client_input = PlayerInput {
        keys: Keys {
            forward: true,
            ..Keys::default()
        },
        ..PlayerInput::default()
    };
    let mut next_state = expected_state;
    let world = server.world();
    let dimension = world
        .query_filtered::<Entity, With<Dimension>>()
        .single(world)
        .unwrap();
    let mut query_state =
        bevy_ecs::system::SystemState::<(WorldView, Res<PhysicsTables>)>::new(world);
    let (world_view, tables) = query_state.get(world).unwrap();
    let blocks = world_view.get(dimension).unwrap();
    let block_world = BlockWorld {
        blocks: &blocks,
        table: tables.collision(),
    };
    let view = View::new(&block_world, &NoBodies, tables.tables());
    let mut scratch = Scratch::new();
    let mut actions = Vec::new();
    mcrs_minecraft_physics::tick_player(
        view,
        mcrs_minecraft_physics::Side::OwnClient,
        &expected_state,
        &client_input,
        &mut next_state,
        &mut actions,
        &mut scratch,
    );
    next_state
}

fn confirm_latest_teleport(server: &mut PlayServer, anchor: Entity, packets: &ObservedPackets) {
    if let Some(position) = packets.teleports.last() {
        server.send(
            anchor,
            &ServerboundAcceptTeleportation {
                teleport_id: position.teleport_id,
                position: position.position.into(),
                look: position.look,
            },
        );
    }
}

fn send_pos(server: &mut PlayServer, anchor: Entity, position: DVec3, on_ground: bool) {
    server.queue(
        anchor,
        &ServerboundMovePlayerPos {
            position: position.into(),
            flags: MoveFlags::new().with_on_ground(on_ground),
        },
    );
}

fn join_at_prediction_floor(server: &mut PlayServer) -> (Entity, Entity, DVec3) {
    let (alice, alice_join_packets) = server.join(ALICE, "Alice");
    accept_login_teleports(server, alice, &alice_join_packets);
    let _ = server.ticks_all(3);
    run_command(server, alice, "gamemode survival @s");
    let teleport_packets = run_command(server, alice, "tp @s 8.5 64.0 8.5");
    assert!(
        !teleport_packets.teleports.is_empty(),
        "tp command must emit pending teleport"
    );
    confirm_latest_teleport(server, alice, &teleport_packets);

    let alice_entity = server.player(alice);
    put(server, alice_entity, 0, Item::Shield, 1);
    let start_position = server
        .world()
        .get::<Transform>(alice_entity)
        .unwrap()
        .translation;
    send_pos(server, alice, start_position, true);
    let _ = collect_packets(server, alice, 1);
    (alice, alice_entity, start_position)
}

fn assert_predicted_move(
    server: &mut PlayServer,
    alice: Entity,
    alice_entity: Entity,
    target_state: &PlayerState,
    message: &str,
) {
    send_pos(
        server,
        alice,
        target_state.body.position,
        target_state.body.flags.on_ground,
    );
    let step_packets = collect_packets(server, alice, 1);
    assert!(
        step_packets.teleports.is_empty(),
        "{message}: unexpected setbacks {:?}",
        step_packets.teleports
    );
    assert!(
        server
            .world()
            .get::<mcrs_minecraft_engine::MovementPrediction>(alice_entity)
            .and_then(|prediction| prediction.last_outcome())
            .is_some_and(|outcome| outcome.setback.is_none()),
        "{message}: MovementPrediction must accept move"
    );
}

#[test]
fn movement_prediction_enforces_slow_speed_while_using_item() {
    let mut server = start_item_use_server();
    let (alice, alice_entity, start_position) = join_at_prediction_floor(&mut server);

    server.queue(
        alice,
        &ServerboundPlayerInput {
            input: PlayerInputFlags::new().with_forward(true),
        },
    );
    use_item(&mut server, alice, Hand::Main, 1);
    let _ = collect_packets(&mut server, alice, 2);
    assert!(
        server.world().get::<UsingItem>(alice_entity).is_some(),
        "shield use must insert UsingItem before prediction step"
    );

    let using_spec = server
        .world()
        .get::<UsingItem>(alice_entity)
        .unwrap()
        .item_use();
    let mut initial = PlayerState::new(start_position);
    initial.body.flags.on_ground = true;
    let next_state = expected_using_step(&mut server, initial, using_spec);
    assert_predicted_move(
        &mut server,
        alice,
        alice_entity,
        &next_state,
        "valid 0.2x item-use move",
    );

    send_pos(
        &mut server,
        alice,
        next_state.body.position + DVec3::new(0.0, 0.0, 0.25),
        true,
    );
    let rejected_step = collect_packets(&mut server, alice, 1);
    assert!(
        !rejected_step.teleports.is_empty(),
        "normal walk delta while holding shield must trigger setback teleport"
    );
    confirm_latest_teleport(&mut server, alice, &rejected_step);
    release_use(&mut server, alice, 2);
    let _ = collect_packets(&mut server, alice, 2);
    assert!(
        server.world().get::<UsingItem>(alice_entity).is_none(),
        "releasing shield must clear UsingItem before normal speed resumes"
    );

    let mut resumed = next_state;
    resumed.item_use = mcrs_minecraft_physics::ItemUse::default();
    let walk_step = expected_using_step(
        &mut server,
        resumed,
        mcrs_minecraft_physics::ItemUse::default(),
    );
    assert_predicted_move(
        &mut server,
        alice,
        alice_entity,
        &walk_step,
        "normal walk speed after shield release",
    );
}

#[test]
fn gates_block_unloaded_spectator_and_dead_players() {
    let mut server = start_item_use_server();
    let (alice, alice_join_packets) = server.join(ALICE, "Alice");
    let alice_entity = server.player(alice);
    put(&mut server, alice_entity, 0, Item::Shield, 1);

    use_item(&mut server, alice, Hand::Main, 1);
    let observed = collect_packets(&mut server, alice, 1);
    assert!(
        observed.own_living_flags.is_empty()
            && server.world().get::<UsingItem>(alice_entity).is_none(),
        "unloaded client cannot start item use"
    );

    accept_login_teleports(&mut server, alice, &alice_join_packets);
    let _ = collect_packets(&mut server, alice, 3);
    run_command(&mut server, alice, "gamemode spectator @s");
    use_item(&mut server, alice, Hand::Main, 2);
    let observed = collect_packets(&mut server, alice, 1);
    assert!(
        observed.own_living_flags.is_empty()
            && server.world().get::<UsingItem>(alice_entity).is_none(),
        "spectator cannot start item use"
    );

    run_command(&mut server, alice, "gamemode survival @s");
    server
        .world()
        .get_mut::<Vitals>(alice_entity)
        .unwrap()
        .health = 0.0;
    use_item(&mut server, alice, Hand::Main, 3);
    let observed = collect_packets(&mut server, alice, 1);
    assert!(
        observed.own_living_flags.is_empty()
            && server.world().get::<UsingItem>(alice_entity).is_none(),
        "dead player cannot start item use"
    );
    assert_eq!(
        server
            .world()
            .get::<LivingFlags>(alice_entity)
            .map(|flags| flags.0),
        Some(0),
        "dead player living_flags must remain 0"
    );
}
