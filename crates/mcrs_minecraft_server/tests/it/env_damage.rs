use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::prelude::With;
use bevy_math::DVec3;
use bevy_time::TimeUpdateStrategy;
use mcrs_minecraft_biome_file::BiomeFile;
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_block::definition::schema::PropertyValue;
use mcrs_minecraft_command_engine::CommandPlugin;
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_command_world::WorldCommandsPlugin;
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine::OwnEquipment;
use mcrs_minecraft_engine::bus::PacketPayload;
use mcrs_minecraft_engine::player_state::{Effect, PlayerFileState};
use mcrs_minecraft_engine::server_edit::ServerStateHostPlugin;
use mcrs_minecraft_engine_world::block_update::BlockSetRequest;
use mcrs_minecraft_engine_world::palette::BiomePalette;
use mcrs_minecraft_engine_world::world::dimension::Dimension;
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_engine_world::world_border::DimensionWorldBorder;
use mcrs_minecraft_entity_attribute::ModifierOperation;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_equipment::EquipSlot;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol::{Bounded, Decode, Packet, VarInt};
use mcrs_minecraft_protocol_game::MoveFlags;
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundDamageEvent, ClientboundEntityEvent, ClientboundSetEntityData,
    ClientboundUpdateAttributes,
};
use mcrs_minecraft_protocol_game::entity::MetaDataValue;
use mcrs_minecraft_protocol_game::entity_data_index::{
    AIR_SUPPLY, SHARED_FLAG_ON_FIRE, SHARED_FLAGS, TICKS_FROZEN,
};
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_protocol_game::serverbound::{
    PlayerInputFlags, ServerboundAcceptTeleportation, ServerboundChatCommand,
    ServerboundMovePlayerPos, ServerboundPlayerInput, ServerboundPlayerLoaded,
};
use mcrs_minecraft_registries::{CurrentRegistries, Ref, WireId};
use mcrs_minecraft_resource::ResourceLocationBuf;
use mcrs_minecraft_server::{
    VanillaBlocksPlugin, VanillaDurabilityPlugin, VanillaEntityAttributesPlugin,
    VanillaEntityPhysicsPlugin, VanillaEnvDamagePlugin, VanillaPlayerPosePlugin,
    VanillaRespawnPlugin, VanillaWeatherPlugin,
};
use mcrs_minecraft_simulation::BlockUpdateFlags;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_minecraft_vanilla_env_damage::BakedEnvDamage;
use mcrs_voxel::{BlockPos, BlockStateId, LocalPos, SectionPos};
use serde_json::Value;

use crate::commands::load_stone_column_at;
use crate::host_app::{self, PlayServer, replicated};

const ALICE: Uuid = Uuid::from_u128(0xE11CE);
const TRACE: &str =
    include_str!("../../../../tools/mcrs_minecraft_probe/traces/environment_damage.json");

fn trace_step<'a>(trace: &'a Value, id: &str) -> &'a Value {
    trace["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"].as_str() == Some(id))
        .unwrap()
}

fn trace_console_contains(step: &Value, needle: &str) {
    let output = step["observed"]["sent"]["console_output"]
        .as_array()
        .unwrap();
    assert!(
        output
            .iter()
            .any(|line| line.as_str().is_some_and(|text| text.contains(needle))),
        "probe trace missing '{needle}' in {output:?}"
    );
}

fn start_env_server() -> PlayServer {
    let mut app = host_app::make_host_app();
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.add_plugins((
        ServerStateHostPlugin,
        CommandPlugin::<VanillaCommands>::new(),
        VanillaCommandsPlugin,
        WorldCommandsPlugin,
        VanillaBlocksPlugin,
        VanillaEntityPhysicsPlugin,
        VanillaPlayerPosePlugin,
        VanillaEntityAttributesPlugin,
        VanillaRespawnPlugin,
        VanillaWeatherPlugin,
        VanillaDurabilityPlugin,
        VanillaEnvDamagePlugin,
    ));
    let mut server = PlayServer::start(app, &[(ALICE, 4)], 0);
    load_stone_column_at(&mut server, 0, 0);
    load_stone_column_at(&mut server, 0, -1);
    load_stone_column_at(&mut server, 1, 0);
    server
}

fn confirm_teleports(server: &mut PlayServer, anchor: Entity, pkts: &[PacketPayload]) {
    for pkt in pkts {
        if let PacketPayload::PlayerPosition(pos) = pkt {
            server.send(
                anchor,
                &ServerboundAcceptTeleportation {
                    teleport_id: pos.teleport_id,
                    position: pos.position.into(),
                    look: pos.look,
                },
            );
        }
    }
}

fn join_survival(server: &mut PlayServer) -> (Entity, Entity) {
    let (anchor, pkts) = server.join(ALICE, "Alice");
    confirm_teleports(server, anchor, &pkts);
    server.queue(anchor, &ServerboundPlayerLoaded);
    let _ = server.ticks_all(2);
    let _ = cmd(server, anchor, "gamemode survival @s");
    let _ = cmd(server, anchor, "gamerule natural_health_regeneration false");
    (anchor, server.player(anchor))
}

fn cmd(server: &mut PlayServer, anchor: Entity, line: &str) -> Vec<PacketPayload> {
    let command = Bounded(line);
    server.queue(anchor, &ServerboundChatCommand { command });
    let mut pkts = server.ticks(anchor, 2);
    for pkt in pkts.clone() {
        if let PacketPayload::PlayerPosition(pos) = pkt {
            pkts.extend(server.send(
                anchor,
                &ServerboundAcceptTeleportation {
                    teleport_id: pos.teleport_id,
                    position: pos.position.into(),
                    look: pos.look,
                },
            ));
        }
    }
    pkts
}

fn tp_player(server: &mut PlayServer, anchor: Entity, pos: DVec3) {
    let _ = cmd(
        server,
        anchor,
        &format!("tp @s {} {} {}", pos.x, pos.y, pos.z),
    );
}

fn enter_hazard(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    pos: DVec3,
    extra_ticks: usize,
) -> Vec<PacketPayload> {
    reset_player(server, anchor, player);
    let mut pkts = cmd(
        server,
        anchor,
        &format!("tp @s {} {} {}", pos.x, pos.y, pos.z),
    );
    if extra_ticks > 0 {
        pkts.extend(server.ticks(anchor, extra_ticks));
    }
    pkts
}

fn count_type(pkts: &[PacketPayload], type_id: u32) -> usize {
    damage_event_types(pkts)
        .into_iter()
        .filter(|id| *id == type_id)
        .count()
}

fn set_block(server: &mut PlayServer, pos: BlockPos, new_state: BlockStateId) {
    let world = server.world();
    let dimension = world
        .query_filtered::<Entity, With<Dimension>>()
        .single(world)
        .unwrap();
    world
        .resource_mut::<Messages<BlockSetRequest>>()
        .write(BlockSetRequest {
            dimension,
            pos,
            new_state,
            flags: BlockUpdateFlags::all(),
            recursion_left: 512,
        });
    let _ = server.ticks_all(1);
}

fn move_step(
    server: &mut PlayServer,
    anchor: Entity,
    pos: DVec3,
    ground: bool,
) -> Vec<PacketPayload> {
    let flags = MoveFlags::from_bits(u8::from(ground));
    let position = pos.into();
    server.send(anchor, &ServerboundMovePlayerPos { position, flags })
}

fn set_shift(server: &mut PlayServer, anchor: Entity, shift: bool) {
    let input = PlayerInputFlags::new().with_shift(shift);
    let _ = server.send(anchor, &ServerboundPlayerInput { input });
}

fn reset_player(server: &mut PlayServer, anchor: Entity, player: Entity) {
    let world = server.world();
    {
        let mut vitals = world.get_mut::<Vitals>(player).unwrap();
        vitals.health = 20.0;
        vitals.last_damage = 0.0;
        vitals.damage_cooldown = 0;
        vitals.hurt_time = 0;
    }
    world
        .get_mut::<PlayerFileState>(player)
        .unwrap()
        .fall_distance = 0.0;
    let _ = server.ticks(anchor, 1);
}

fn hp(server: &mut PlayServer, player: Entity) -> f32 {
    server.world().get::<Vitals>(player).unwrap().health
}

fn assert_close(got: f32, expected: f32, what: &str) {
    assert!(
        (got - expected).abs() < 1.0e-4,
        "{what}: got {got}, expected {expected}"
    );
}

fn damage_event_types(pkts: &[PacketPayload]) -> Vec<u32> {
    replicated(pkts)
        .into_iter()
        .filter(|(id, _)| *id == ClientboundDamageEvent::ID)
        .map(|(_, bytes)| {
            ClientboundDamageEvent::decode(&mut bytes.as_slice())
                .unwrap()
                .damage_type
                .0 as u32
        })
        .collect()
}

fn has_sound(pkts: &[PacketPayload], expected_id: WireId<SoundEvent>) -> bool {
    pkts.iter().any(|pkt| {
        matches!(
            pkt,
            PacketPayload::Sound(sound)
                if matches!(sound.sound_event, Ref::Id(id) if id == expected_id)
        )
    })
}

fn block_default(server: &mut PlayServer, name: &str) -> BlockStateId {
    server
        .world()
        .resource::<Blocks>()
        .default_state(name)
        .unwrap()
}

#[test]
fn env_damage_fall_sequences_match_probe_trace() {
    let trace: Value = serde_json::from_str(TRACE).unwrap();
    let mut server = start_env_server();
    let (anchor, player) = join_survival(&mut server);
    let baked = *server.world().resource::<BakedEnvDamage>();

    let hay = block_default(&mut server, "minecraft:hay_block");
    let slime = block_default(&mut server, "minecraft:slime_block");
    let water = block_default(&mut server, "minecraft:water");
    set_block(&mut server, BlockPos::new(2, 63, 0), hay);
    set_block(&mut server, BlockPos::new(4, 63, 0), slime);
    set_block(&mut server, BlockPos::new(6, 64, 0), water);

    tp_player(&mut server, anchor, DVec3::new(0.5, 74.0, 0.5));
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 70.5, 0.5), false);
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 67.0, 0.5), false);
    let land = move_step(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5), true);
    assert_close(hp(&mut server, player), 13.0, "seq_fall_norm health");
    trace_console_contains(trace_step(&trace, "seq_fall_norm"), "13.0f");
    assert_eq!(
        damage_event_types(&land),
        vec![baked.fall.unwrap()],
        "seq_fall_norm damage_event"
    );
    assert!(
        has_sound(&land, WireId::from(baked.player_big_fall.unwrap())),
        "seq_fall_norm big fall sound"
    );

    reset_player(&mut server, anchor, player);
    tp_player(&mut server, anchor, DVec3::new(0.5, 74.0, 0.5));
    for y in [70.5, 67.0, 64.0] {
        let _ = move_step(&mut server, anchor, DVec3::new(0.5, y, 0.5), true);
    }
    assert_close(hp(&mut server, player), 20.0, "seq_fall_spoof health");
    trace_console_contains(trace_step(&trace, "seq_fall_spoof"), "20.0f");

    let _ = cmd(&mut server, anchor, "gamerule fall_damage false");
    tp_player(&mut server, anchor, DVec3::new(0.5, 74.0, 0.5));
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 70.5, 0.5), false);
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 67.0, 0.5), false);
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5), true);
    assert_close(hp(&mut server, player), 20.0, "seq_fall_rule_false health");
    trace_console_contains(trace_step(&trace, "seq_fall_rule_false"), "20.0f");
    let _ = cmd(&mut server, anchor, "gamerule fall_damage true");

    tp_player(&mut server, anchor, DVec3::new(4.5, 74.0, 0.5));
    let _ = move_step(&mut server, anchor, DVec3::new(4.5, 70.5, 0.5), false);
    let _ = move_step(&mut server, anchor, DVec3::new(4.5, 67.0, 0.5), false);
    let _ = move_step(&mut server, anchor, DVec3::new(4.5, 64.0, 0.5), true);
    assert_close(hp(&mut server, player), 20.0, "seq_fall_slime health");
    trace_console_contains(trace_step(&trace, "seq_fall_slime"), "20.0f");

    tp_player(&mut server, anchor, DVec3::new(6.5, 74.0, 0.5));
    let _ = move_step(&mut server, anchor, DVec3::new(6.5, 70.5, 0.5), false);
    let _ = move_step(&mut server, anchor, DVec3::new(6.5, 67.0, 0.5), false);
    let _ = move_step(&mut server, anchor, DVec3::new(6.5, 64.0, 0.5), false);
    assert_close(hp(&mut server, player), 20.0, "seq_fall_water health");
    trace_console_contains(trace_step(&trace, "seq_fall_water"), "20.0f");
    assert_eq!(
        server
            .world()
            .get::<PlayerFileState>(player)
            .unwrap()
            .fall_distance,
        0.0,
        "water resets fall_distance"
    );

    tp_player(&mut server, anchor, DVec3::new(2.5, 79.0, 0.5));
    for (y, ground) in [
        (75.5, false),
        (72.0, false),
        (68.5, false),
        (65.5, false),
        (64.0, true),
    ] {
        let _ = move_step(&mut server, anchor, DVec3::new(2.5, y, 0.5), ground);
    }
    assert_close(
        hp(&mut server, player),
        18.0,
        "hay_block 15-block 0.2x fall health",
    );
}

#[test]
fn env_damage_suffocation_cactus_campfire_and_magma_match_probe_trace() {
    let trace: Value = serde_json::from_str(TRACE).unwrap();
    let probe = trace_step(&trace, "seq_suffocation_cactus_campfire_magma");
    trace_console_contains(probe, "19.0f");
    trace_console_contains(probe, "17.0f");

    let mut server = start_env_server();
    let (anchor, player) = join_survival(&mut server);
    let baked = *server.world().resource::<BakedEnvDamage>();
    let stone = block_default(&mut server, "minecraft:stone");
    let sand = block_default(&mut server, "minecraft:sand");
    let cactus = block_default(&mut server, "minecraft:cactus");
    let campfire = block_default(&mut server, "minecraft:campfire");
    let soul_campfire = block_default(&mut server, "minecraft:soul_campfire");
    let unlit_campfire = {
        let blocks = server.world().resource::<Blocks>();
        let entry = blocks.block("minecraft:campfire").unwrap();
        entry
            .with(entry.default_state_id, "lit", &PropertyValue::Bool(false))
            .unwrap()
    };
    let magma = block_default(&mut server, "minecraft:magma_block");

    set_block(&mut server, BlockPos::new(0, 64, 6), stone);
    set_block(&mut server, BlockPos::new(0, 65, 6), stone);
    let mut suffocation_packets =
        enter_hazard(&mut server, anchor, player, DVec3::new(0.5, 64.0, 6.5), 0);
    assert_close(hp(&mut server, player), 19.0, "suffocation 1st hit health");
    assert_eq!(
        count_type(&suffocation_packets, baked.in_wall.unwrap()),
        1,
        "in_wall 1st damage event"
    );
    suffocation_packets.extend(server.ticks(anchor, 20));
    assert_close(hp(&mut server, player), 17.0, "suffocation 3 hits health");
    assert_eq!(
        count_type(&suffocation_packets, baked.in_wall.unwrap()),
        3,
        "in_wall 3 damage events"
    );

    set_block(&mut server, BlockPos::new(0, 64, 6), BlockStateId::AIR);
    set_block(&mut server, BlockPos::new(0, 65, 6), BlockStateId::AIR);
    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_block(&mut server, BlockPos::new(0, 63, 8), sand);
    set_block(&mut server, BlockPos::new(0, 64, 8), cactus);
    let cactus_pkts = enter_hazard(&mut server, anchor, player, DVec3::new(0.5, 64.0, 8.5), 19);
    assert_close(hp(&mut server, player), 17.0, "cactus 3 hits health");
    assert_eq!(
        count_type(&cactus_pkts, baked.cactus.unwrap()),
        3,
        "cactus 3 events"
    );

    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_block(&mut server, BlockPos::new(0, 64, 8), campfire);
    let campfire_pkts = enter_hazard(&mut server, anchor, player, DVec3::new(0.5, 64.0, 8.5), 19);
    assert_close(hp(&mut server, player), 17.0, "campfire 3 hits health");
    assert_eq!(
        count_type(&campfire_pkts, baked.campfire.unwrap()),
        3,
        "campfire 3 events"
    );

    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_block(&mut server, BlockPos::new(0, 64, 8), unlit_campfire);
    let _ = enter_hazard(&mut server, anchor, player, DVec3::new(0.5, 64.0, 8.5), 12);
    assert_close(hp(&mut server, player), 20.0, "unlit campfire no damage");

    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_block(&mut server, BlockPos::new(0, 64, 8), soul_campfire);
    let _ = enter_hazard(&mut server, anchor, player, DVec3::new(0.5, 64.0, 8.5), 0);
    assert_close(hp(&mut server, player), 18.0, "soul campfire 2.0 damage");

    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_block(&mut server, BlockPos::new(0, 64, 8), BlockStateId::AIR);
    set_block(&mut server, BlockPos::new(0, 63, 8), magma);
    let mut magma_pkts = enter_hazard(&mut server, anchor, player, DVec3::new(0.5, 64.0, 8.5), 0);
    magma_pkts.extend(move_step(
        &mut server,
        anchor,
        DVec3::new(0.5, 64.0, 8.5),
        true,
    ));
    magma_pkts.extend(server.ticks(anchor, 20));
    assert_close(hp(&mut server, player), 17.0, "magma standing 3 hits");
    assert_eq!(
        count_type(&magma_pkts, baked.hot_floor.unwrap()),
        3,
        "magma 3 events"
    );

    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_shift(&mut server, anchor, true);
    let _ = enter_hazard(&mut server, anchor, player, DVec3::new(0.5, 64.0, 8.5), 0);
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 64.0, 8.5), true);
    let _ = server.ticks(anchor, 20);
    assert_close(hp(&mut server, player), 20.0, "magma crouching suppressed");
    set_shift(&mut server, anchor, false);
}

#[test]
fn env_damage_sweet_berry_void_and_world_border_match_probe_trace() {
    let trace: Value = serde_json::from_str(TRACE).unwrap();
    let probe = trace_step(&trace, "seq_berry_void_border");
    trace_console_contains(probe, "17.0f");
    trace_console_contains(probe, "8.0f");
    trace_console_contains(probe, "16.0f");

    let mut server = start_env_server();
    let (anchor, player) = join_survival(&mut server);
    let baked = *server.world().resource::<BakedEnvDamage>();

    let dirt = block_default(&mut server, "minecraft:dirt");
    let (bush_age0, bush_age2) = {
        let blocks = server.world().resource::<Blocks>();
        let entry = blocks.block("minecraft:sweet_berry_bush").unwrap();
        let age0 = entry.default_state_id;
        let age2 = entry
            .with(entry.default_state_id, "age", &PropertyValue::Int(2))
            .unwrap();
        (age0, age2)
    };

    set_block(&mut server, BlockPos::new(0, 63, -4), dirt);
    set_block(&mut server, BlockPos::new(0, 64, -4), bush_age0);
    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, -3.2));
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 64.0, -3.5), true);
    assert_close(hp(&mut server, player), 20.0, "sweet berry age 0 no damage");

    set_block(&mut server, BlockPos::new(0, 64, -4), bush_age2);
    let _ = server.ticks(anchor, 10);
    assert_close(
        hp(&mut server, player),
        20.0,
        "sweet berry stationary no damage",
    );

    let berry_one = move_step(&mut server, anchor, DVec3::new(0.5, 64.0, -3.2), true);
    assert_close(hp(&mut server, player), 19.0, "sweet berry moving 1st hit");
    assert_eq!(
        damage_event_types(&berry_one),
        vec![baked.sweet_berry_bush.unwrap()],
        "sweet berry damage event"
    );
    let _ = server.ticks(anchor, 11);
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 64.0, -3.5), true);
    let _ = server.ticks(anchor, 11);
    let _ = move_step(&mut server, anchor, DVec3::new(0.5, 64.0, -3.8), true);
    assert_close(hp(&mut server, player), 17.0, "sweet berry 3 moving hits");

    reset_player(&mut server, anchor, player);
    let void_tp = cmd(&mut server, anchor, "tp @s 0.5 -129 0.5");
    let void_pkts = server.ticks(anchor, 20);
    assert_close(hp(&mut server, player), 8.0, "void 3 hits health");
    let mut void_events = damage_event_types(&void_tp);
    void_events.extend(damage_event_types(&void_pkts));
    assert_eq!(
        void_events
            .into_iter()
            .filter(|id| *id == baked.out_of_world.unwrap())
            .count(),
        3,
        "void 3 damage events"
    );

    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    reset_player(&mut server, anchor, player);
    let _ = cmd(&mut server, anchor, "gamemode creative @s");
    let creative_void = cmd(&mut server, anchor, "tp @s 0.5 -129 0.5");
    assert_close(
        hp(&mut server, player),
        16.0,
        "void hurts creative players by 4.0 HP",
    );
    assert!(
        count_type(&creative_void, baked.out_of_world.unwrap()) >= 1,
        "void emits out_of_world event for creative players"
    );

    let _ = cmd(&mut server, anchor, "gamemode survival @s");
    tp_player(&mut server, anchor, DVec3::new(20.5, 64.0, 0.5));
    reset_player(&mut server, anchor, player);
    server
        .app
        .world_mut()
        .insert_resource(DimensionWorldBorder::with_center_and_size(0.5, 0.5, 20.0));
    let border_pkts = server.ticks(anchor, 32);
    assert_close(hp(&mut server, player), 16.0, "world border 4 hits health");
    assert_eq!(
        damage_event_types(&border_pkts)
            .into_iter()
            .filter(|id| *id == baked.outside_border.unwrap())
            .count(),
        4,
        "world border 4 damage events"
    );
}

fn own_entity_packets(packets: &[PacketPayload], expected_id: i32) -> impl Iterator<Item = &[u8]> {
    packets.iter().filter_map(move |packet| {
        let PacketPayload::OwnEntityState(bytes) = packet else {
            return None;
        };
        let mut rest: &[u8] = bytes;
        let packet_id = VarInt::decode(&mut rest).ok()?;
        (packet_id.0 == expected_id).then_some(rest)
    })
}

fn own_metadata_values(packets: &[PacketPayload], target_index: u8) -> Vec<i32> {
    let mut values = Vec::new();
    for mut rest in own_entity_packets(packets, ClientboundSetEntityData::ID) {
        let decoded = ClientboundSetEntityData::decode(&mut rest)
            .expect("SetEntityData packet should decode cleanly");
        for entry in decoded.metadata.0 {
            if entry.index == target_index {
                match entry.value {
                    MetaDataValue::Byte(flag) => values.push(i32::from(flag)),
                    MetaDataValue::VarInt(value) => values.push(value.0),
                    _ => {}
                }
            }
        }
    }
    values
}

fn own_movement_modifiers(packets: &[PacketPayload]) -> Vec<(String, f64, ModifierOperation)> {
    let mut modifiers = Vec::new();
    for mut rest in own_entity_packets(packets, ClientboundUpdateAttributes::ID) {
        let decoded = ClientboundUpdateAttributes::decode(&mut rest)
            .expect("UpdateAttributes packet should decode cleanly");
        for attribute in decoded.attributes {
            if attribute.attribute.resolve() == Some(Attribute::MovementSpeed) {
                for modifier in attribute.modifiers {
                    modifiers.push((modifier.id.to_string(), modifier.amount, modifier.operation));
                }
            }
        }
    }
    modifiers
}

fn entity_event_statuses(packets: &[PacketPayload]) -> Vec<i8> {
    replicated(packets)
        .into_iter()
        .filter(|(id, _)| *id == ClientboundEntityEvent::ID)
        .map(|(_, bytes)| {
            ClientboundEntityEvent::decode(&mut bytes.as_slice())
                .expect("EntityEvent packet should decode cleanly")
                .entity_status
        })
        .collect()
}

fn sound_count(packets: &[PacketPayload], expected_id: WireId<SoundEvent>) -> usize {
    packets
        .iter()
        .filter(|packet| {
            matches!(
                packet,
                PacketPayload::Sound(sound)
                    if matches!(sound.sound_event, Ref::Id(id) if id == expected_id)
            )
        })
        .count()
}

fn effect_of(id: &str, duration: i32) -> Effect {
    Effect {
        id: id.to_owned(),
        amplifier: 0,
        duration,
        ambient: false,
        show_particles: true,
        show_icon: None,
        hidden_effect: None,
    }
}

fn dry_top_stone_slab(server: &mut PlayServer) -> BlockStateId {
    let blocks = server.world().resource::<Blocks>();
    blocks
        .blocks()
        .iter()
        .find(|entry| entry.identifier.as_str() == "minecraft:stone_slab")
        .and_then(|entry| {
            (0..entry.state_count)
                .filter_map(|offset| entry.state_at(offset))
                .find(|&id| {
                    entry
                        .value_of(id, "type")
                        .is_some_and(|value| value.renders_to("top"))
                        && entry
                            .value_of(id, "waterlogged")
                            .is_some_and(|value| value.renders_to("false"))
                })
        })
        .expect("dry top stone_slab block state should exist in registry")
}

fn set_column_plains_biome(server: &mut PlayServer, column_x: i32, column_z: i32) {
    let registries = server.world().resource::<CurrentRegistries>().clone();
    let biome_location = ResourceLocationBuf::read("minecraft:plains")
        .expect("plains biome resource location should parse");
    let raw_id = registries
        .registry::<BiomeFile>()
        .expect("BiomeFile registry should load")
        .by_name(&biome_location)
        .expect("plains biome entry should exist")
        .number();
    let biome_id = u8::try_from(raw_id).expect("plains biome id should fit in u8 palette");
    let world = server.world();
    let sections: Vec<Entity> = (-4..20)
        .filter_map(|section_y| {
            world
                .query::<&SectionIndex>()
                .single(world)
                .ok()?
                .get(SectionPos::new(column_x, section_y, column_z))
        })
        .collect();
    for entity in sections {
        world
            .entity_mut(entity)
            .insert(BiomePalette::homogeneous(biome_id));
    }
}

fn block_at(server: &mut PlayServer, position: BlockPos) -> BlockStateId {
    let world = server.world();
    let dimension = world
        .query_filtered::<Entity, With<Dimension>>()
        .single(world)
        .expect("server world should contain a single dimension");
    world
        .get::<SectionIndex>(dimension)
        .expect("dimension should carry SectionIndex")
        .blocks(SectionPos::from(position))
        .map_or(BlockStateId::AIR, |section| {
            section.0.get(LocalPos::from(position))
        })
}

fn state_of(server: &mut PlayServer, player: Entity) -> &PlayerFileState {
    server
        .world()
        .get::<PlayerFileState>(player)
        .expect("player should carry PlayerFileState")
}

fn state_mut(server: &mut PlayServer, player: Entity, edit: impl FnOnce(&mut PlayerFileState)) {
    let mut state = server
        .world()
        .get_mut::<PlayerFileState>(player)
        .expect("player should carry mutable PlayerFileState");
    edit(&mut state);
}

#[test]
fn env_damage_drowning_matches_probe_trace() {
    let trace: Value =
        serde_json::from_str(TRACE).expect("environment_damage.json probe trace should parse");
    let probe = trace_step(&trace, "seq_drowning");
    assert_eq!(
        probe["entity_events"]
            .as_array()
            .expect("seq_drowning should contain entity_events")
            .len(),
        6,
        "probe drowning trace should record 6 bubble entity events"
    );
    assert_eq!(
        probe["damage_events"]
            .as_array()
            .expect("seq_drowning should contain damage_events")
            .len(),
        4,
        "probe drowning trace should record 4 drowning damage events"
    );
    trace_console_contains(probe, "12.0f");
    trace_console_contains(probe, "300s");

    let mut server = start_env_server();
    let (anchor, player) = join_survival(&mut server);
    let baked = *server.world().resource::<BakedEnvDamage>();
    let drown_id = baked.drown.expect("drown damage type should bake");
    let water = block_default(&mut server, "minecraft:water");
    let bubble_column = block_default(&mut server, "minecraft:bubble_column");

    set_block(&mut server, BlockPos::new(5, 64, 5), water);
    set_block(&mut server, BlockPos::new(5, 65, 5), water);
    let mut drain_packets = cmd(&mut server, anchor, "tp @s 5.5 64 5.5");
    let current_air = state_of(&mut server, player).air;
    assert!(
        current_air < 300,
        "submerged eyes should begin depleting air supply below 300"
    );
    let remaining_ticks =
        usize::try_from(current_air + 20).expect("remaining ticks to drown should be positive");
    drain_packets.extend(server.ticks(anchor, remaining_ticks));

    assert_close(
        hp(&mut server, player),
        18.0,
        "first drowning hit should reduce health from 20.0 to 18.0",
    );
    assert_eq!(
        state_of(&mut server, player).air,
        0,
        "air supply should reset to 0 after drowning hit"
    );
    assert!(
        own_metadata_values(&drain_packets, AIR_SUPPLY).contains(&0),
        "AIR_SUPPLY metadata update to 0 should be replicated to owner"
    );
    assert!(
        entity_event_statuses(&drain_packets).contains(&ClientboundEntityEvent::DROWN_BUBBLES),
        "drowning hit should emit DROWN_BUBBLES entity event 67"
    );
    let order: Vec<i32> = replicated(&drain_packets)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let bubble_position = order
        .iter()
        .position(|id| *id == ClientboundEntityEvent::ID)
        .expect("replicated stream should include bubble ClientboundEntityEvent");
    let damage_position = order
        .iter()
        .position(|id| *id == ClientboundDamageEvent::ID)
        .expect("replicated stream should include ClientboundDamageEvent");
    assert!(
        bubble_position < damage_position,
        "entity_event 67 should be sent before damage_event on drowning hit"
    );

    let second_hit_packets = server.ticks(anchor, 20);
    assert_close(
        hp(&mut server, player),
        16.0,
        "second drowning hit should reduce health to 16.0",
    );
    assert_eq!(
        count_type(&second_hit_packets, drown_id),
        1,
        "second drowning interval should emit one drown damage event"
    );
    let _ = server.ticks(anchor, 20);
    assert_close(
        hp(&mut server, player),
        14.0,
        "third drowning hit should reduce health to 14.0",
    );
    let _ = server.ticks(anchor, 20);
    assert_close(
        hp(&mut server, player),
        12.0,
        "fourth drowning hit should reduce health to 12.0",
    );

    assert_drowning_damage_gamerule_disabled(&mut server, anchor, player, drown_id);
    assert_drown_protections_and_regeneration(&mut server, anchor, player, bubble_column);
}

fn assert_drowning_damage_gamerule_disabled(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    drown_id: u32,
) {
    let _ = cmd(server, anchor, "gamerule drowning_damage false");
    let rule_off_packets = server.ticks(anchor, 20);
    assert_close(
        hp(server, player),
        12.0,
        "disabling drowning_damage gamerule should hold player health at 12.0",
    );
    assert!(
        entity_event_statuses(&rule_off_packets).contains(&ClientboundEntityEvent::DROWN_BUBBLES),
        "disabling drowning_damage should still emit bubble entity event 67"
    );
    assert_eq!(
        count_type(&rule_off_packets, drown_id),
        0,
        "disabling drowning_damage should suppress drown damage events"
    );
    let _ = cmd(server, anchor, "gamerule drowning_damage true");
}

fn assert_drown_protections_and_regeneration(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    bubble_column: BlockStateId,
) {
    state_mut(server, player, |state| {
        state.air = 200;
        state
            .active_effects
            .push(effect_of("minecraft:water_breathing", 200));
    });
    let _ = server.ticks(anchor, 5);
    assert_eq!(
        state_of(server, player).air,
        220,
        "WaterBreathing underwater should refill air supply by +4 per tick"
    );
    state_mut(server, player, |state| state.air = 295);
    let _ = server.ticks(anchor, 2);
    assert_eq!(
        state_of(server, player).air,
        300,
        "WaterBreathing underwater refill should clamp at 300"
    );

    state_mut(server, player, |state| {
        state.active_effects.clear();
        state.air = 120;
        state
            .active_effects
            .push(effect_of("minecraft:breath_of_the_nautilus", 200));
    });
    let _ = server.ticks(anchor, 5);
    assert_eq!(
        state_of(server, player).air,
        120,
        "BreathOfTheNautilus alone should hold underwater air unchanged"
    );

    state_mut(server, player, |state| state.active_effects.clear());
    let _ = cmd(server, anchor, "gamemode spectator @s");
    state_mut(server, player, |state| state.air = 120);
    let _ = server.ticks(anchor, 1);
    assert_eq!(
        state_of(server, player).air,
        124,
        "Spectator mode submerged should refill air by +4 per tick"
    );
    let _ = cmd(server, anchor, "gamemode survival @s");

    state_mut(server, player, |state| state.air = 200);
    set_block(server, BlockPos::new(5, 65, 5), bubble_column);
    assert_eq!(
        state_of(server, player).air,
        204,
        "bubble_column should refill air by +4 on placement tick"
    );
    let _ = server.ticks(anchor, 1);
    assert_eq!(
        state_of(server, player).air,
        208,
        "bubble_column should continue refilling air by +4 per tick"
    );

    tp_player(server, anchor, DVec3::new(0.5, 64.0, 0.5));
    let regeneration_packets = server.ticks(anchor, 25);
    assert_eq!(
        state_of(server, player).air,
        300,
        "stepping back onto dry land should refill air to 300"
    );
    assert!(
        own_metadata_values(&regeneration_packets, AIR_SUPPLY).contains(&300),
        "returning to 300 air should replicate AIR_SUPPLY = 300 to owner"
    );
}

fn set_named_block(server: &mut PlayServer, position: BlockPos, name: &str) {
    let state = block_default(server, name);
    set_block(server, position, state);
}

#[test]
fn env_damage_fire_and_lava_match_probe_trace() {
    let trace: Value =
        serde_json::from_str(TRACE).expect("environment_damage.json probe trace should parse");
    let probe = trace_step(&trace, "seq_fire_lava");
    trace_console_contains(probe, "-20s");
    trace_console_contains(probe, "300s");
    trace_console_contains(probe, "8.0f");

    let mut server = start_env_server();
    load_stone_column_at(&mut server, -1, 0);
    let (anchor, player) = join_survival(&mut server);
    let baked = *server.world().resource::<BakedEnvDamage>();
    let burn_sound = WireId::from(baked.generic_burn.expect("generic_burn sound should bake"));
    let extinguish_sound = WireId::from(
        baked
            .extinguish_fire
            .expect("extinguish_fire sound should bake"),
    );

    tp_player(&mut server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_named_block(&mut server, BlockPos::new(0, 64, 3), "minecraft:fire");
    set_named_block(&mut server, BlockPos::new(-8, 64, 8), "minecraft:water");
    set_named_block(&mut server, BlockPos::new(8, 64, 8), "minecraft:lava");

    let mut fire_packets = cmd(&mut server, anchor, "tp @s 0.5 64 3.5");
    fire_packets.extend(server.ticks(anchor, 20));
    assert!(
        state_of(&mut server, player).fire > 0,
        "standing inside fire should keep fire ticks positive"
    );
    assert!(
        own_metadata_values(&fire_packets, SHARED_FLAGS)
            .iter()
            .any(|flag| (*flag as u8) & SHARED_FLAG_ON_FIRE != 0),
        "igniting player should replicate SHARED_FLAG_ON_FIRE bit set"
    );
    assert!(
        count_type(&fire_packets, baked.in_fire.expect("in_fire should bake")) >= 1,
        "standing in fire should emit at least one in_fire damage event"
    );

    let water_packets = cmd(&mut server, anchor, "tp @s -7.5 64 8.5");
    assert_eq!(
        state_of(&mut server, player).fire,
        -20,
        "entering water should extinguish fire back to -20 grace ticks"
    );
    assert!(
        own_metadata_values(&water_packets, SHARED_FLAGS)
            .iter()
            .any(|flag| (*flag as u8) & SHARED_FLAG_ON_FIRE == 0),
        "extinguishing fire in water should clear SHARED_FLAG_ON_FIRE metadata bit"
    );
    assert!(
        has_sound(&water_packets, extinguish_sound),
        "water extinguishing burning player should play extinguish sound"
    );

    reset_player(&mut server, anchor, player);
    let fire_resistance = effect_of("minecraft:fire_resistance", 400);
    state_mut(&mut server, player, |state| {
        state.active_effects.push(fire_resistance);
    });
    let fire_resistance_packets = cmd(&mut server, anchor, "tp @s 8.5 64 8.5");
    assert_eq!(
        state_of(&mut server, player).fire,
        300,
        "lava contact should ignite fire to 300 ticks even with FireResistance"
    );
    assert_close(
        hp(&mut server, player),
        20.0,
        "FireResistance should preserve health at 20.0 inside lava",
    );
    assert_eq!(
        sound_count(&fire_resistance_packets, burn_sound),
        0,
        "FireResistance cancelling lava damage should emit zero burn sounds"
    );

    assert_lava_hits_and_gamerule(&mut server, anchor, player, &baked, burn_sound);
    assert_simultaneous_fire_and_lava(&mut server, anchor, player, &baked, burn_sound);
    assert_rain_extinguish_and_roof(&mut server, anchor, player, extinguish_sound);
}

fn assert_lava_hits_and_gamerule(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    baked: &BakedEnvDamage,
    burn_sound: WireId<SoundEvent>,
) {
    state_mut(server, player, |state| state.active_effects.clear());
    let lava_packets = server.ticks(anchor, 22);
    assert!(
        hp(server, player) <= 12.0,
        "lava exposure without FireResistance should deal repeated 4.0 HP hits"
    );
    assert!(
        count_type(&lava_packets, baked.lava.expect("lava should bake")) >= 2,
        "22 ticks in lava should emit at least two lava damage events"
    );
    assert_eq!(
        count_type(&lava_packets, baked.on_fire.expect("on_fire should bake")),
        0,
        "player submerged in lava should emit zero on_fire hits"
    );
    assert!(
        sound_count(&lava_packets, burn_sound) >= 2,
        "repeated lava hits should emit generic burn sounds"
    );

    let _ = cmd(server, anchor, "gamerule fire_damage false");
    let health_before_rule = hp(server, player);
    let rule_off_packets = server.ticks(anchor, 22);
    assert_close(
        hp(server, player),
        health_before_rule,
        "disabling fire_damage gamerule should hold health steady in lava",
    );
    assert_eq!(
        sound_count(&rule_off_packets, burn_sound),
        0,
        "disabling fire_damage gamerule should suppress lava burn sounds"
    );
}

fn assert_simultaneous_fire_and_lava(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    baked: &BakedEnvDamage,
    burn_sound: WireId<SoundEvent>,
) {
    tp_player(server, anchor, DVec3::new(0.5, 64.0, 0.5));
    let _ = cmd(server, anchor, "gamerule fire_damage true");
    set_named_block(server, BlockPos::new(0, 64, 3), "minecraft:fire");
    set_named_block(server, BlockPos::new(1, 64, 3), "minecraft:lava");
    reset_player(server, anchor, player);
    let simultaneous_packets = cmd(server, anchor, "tp @s 0.85 64 3.5");
    assert!(
        count_type(
            &simultaneous_packets,
            baked.in_fire.expect("in_fire should bake"),
        ) >= 1,
        "overlapping both fire and lava in one tick should still emit in_fire"
    );
    assert!(
        hp(server, player) <= 16.0,
        "overlapping both fire and lava in one tick should also apply partial lava damage to reach <= 16.0 health"
    );
    assert_eq!(
        state_of(server, player).fire,
        300,
        "overlapping both fire and lava should ignite player to lava 300 ticks"
    );
    assert!(
        sound_count(&simultaneous_packets, burn_sound) >= 1,
        "overlapping both fire and lava should emit lava burn sound"
    );
    tp_player(server, anchor, DVec3::new(0.5, 64.0, 0.5));
    set_block(server, BlockPos::new(1, 64, 3), BlockStateId::AIR);
}

fn assert_rain_extinguish_and_roof(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    extinguish_sound: WireId<SoundEvent>,
) {
    reset_player(server, anchor, player);
    let _ = cmd(server, anchor, "gamerule fire_damage true");
    set_named_block(server, BlockPos::new(0, 64, 3), "minecraft:fire");
    let _ = cmd(server, anchor, "tp @s 0.5 64 3.5");
    let _ = server.ticks(anchor, 20);
    tp_player(server, anchor, DVec3::new(0.5, 64.0, 0.5));
    let _ = server.ticks(anchor, 5);
    set_column_plains_biome(server, 0, 0);
    let rain = WeatherLevel {
        rain: 1.0,
        thunder: 0.0,
    };
    *server.world().resource_mut::<WeatherLevel>() = rain;
    let rain_packets = server.ticks(anchor, 2);
    assert_eq!(
        state_of(server, player).fire,
        -20,
        "open-sky rain in plains biome should extinguish burning player to -20"
    );
    assert!(
        has_sound(&rain_packets, extinguish_sound),
        "rain extinguishing fire should emit extinguish sound"
    );

    let top_slab = dry_top_stone_slab(server);
    set_block(server, BlockPos::new(0, 65, 0), top_slab);
    state_mut(server, player, |state| state.fire = 100);
    let _ = server.ticks(anchor, 2);
    assert!(
        state_of(server, player).fire > 0,
        "top slab at player head block height should shield player from rain"
    );

    set_named_block(server, BlockPos::new(0, 65, 0), "minecraft:air");
    set_named_block(server, BlockPos::new(0, 67, 0), "minecraft:water");
    state_mut(server, player, |state| state.fire = 100);
    let _ = server.ticks(anchor, 2);
    assert!(
        state_of(server, player).fire > 0,
        "overhead fluid block should shield player from rain"
    );

    set_named_block(server, BlockPos::new(0, 67, 0), "minecraft:stone");
    state_mut(server, player, |state| state.fire = 100);
    let _ = server.ticks(anchor, 2);
    assert!(
        state_of(server, player).fire > 0,
        "overhead solid roof block should shield player from rain"
    );
}

#[test]
fn env_damage_freezing_matches_probe_trace() {
    let trace: Value =
        serde_json::from_str(TRACE).expect("environment_damage.json probe trace should parse");
    let probe = trace_step(&trace, "seq_freezing");
    let probe_sounds = probe["sounds"]
        .as_array()
        .expect("seq_freezing should contain sounds");
    assert_eq!(
        probe_sounds.len(),
        3,
        "probe freezing trace should record 3 total sound events"
    );
    let extinguish_sound_count = probe_sounds
        .iter()
        .filter(|entry| entry["sound"] == "minecraft:entity.generic.extinguish_fire")
        .count();
    assert_eq!(
        extinguish_sound_count, 2,
        "probe freezing trace should record 2 extinguish_fire sound events"
    );
    let equip_sound_count = probe_sounds
        .iter()
        .filter(|entry| entry["sound"] == "minecraft:item.armor.equip_leather")
        .count();
    assert_eq!(
        equip_sound_count, 1,
        "probe freezing trace should record 1 equip_leather sound event"
    );
    trace_console_contains(probe, "140");
    trace_console_contains(probe, "Test passed");

    let mut server = start_env_server();
    load_stone_column_at(&mut server, -1, 0);
    let (anchor, player) = join_survival(&mut server);
    let baked = *server.world().resource::<BakedEnvDamage>();
    let extinguish_sound = WireId::from(
        baked
            .extinguish_fire
            .expect("extinguish_fire sound should bake"),
    );
    let powder_snow = block_default(&mut server, "minecraft:powder_snow");

    set_block(&mut server, BlockPos::new(-4, 64, 0), powder_snow);
    set_block(&mut server, BlockPos::new(-4, 65, 0), powder_snow);
    let _ = cmd(&mut server, anchor, "tp @s -3.5 64 0.5");
    let freeze_packets = server.ticks(anchor, 165);
    assert_eq!(
        state_of(&mut server, player).ticks_frozen,
        140,
        "standing in powder_snow should cap ticks_frozen at 140"
    );
    assert!(
        own_metadata_values(&freeze_packets, TICKS_FROZEN).contains(&140),
        "TICKS_FROZEN = 140 metadata should replicate to owner"
    );
    assert!(
        own_movement_modifiers(&freeze_packets)
            .iter()
            .any(|(identifier, amount, operation)| {
                identifier == "minecraft:powder_snow"
                    && (*amount - (-0.05_f32 as f64)).abs() < 1.0e-7
                    && *operation == ModifierOperation::AddValue
            }),
        "UpdateAttributes should replicate minecraft:powder_snow slow modifier"
    );
    assert!(
        hp(&mut server, player) <= 19.0,
        "reaching maximum frozen ticks should deal at least 1.0 freeze damage"
    );
    assert!(
        count_type(
            &freeze_packets,
            baked.freeze.expect("freeze damage type should bake"),
        ) >= 1,
        "maximum freeze exposure should emit at least one freeze damage event"
    );

    let _ = cmd(&mut server, anchor, "gamerule freeze_damage false");
    let health_before_freeze = hp(&mut server, player);
    let _ = server.ticks(anchor, 45);
    assert_close(
        hp(&mut server, player),
        health_before_freeze,
        "disabling freeze_damage gamerule should hold player health steady",
    );
    let _ = cmd(&mut server, anchor, "gamerule freeze_damage true");

    assert_freeze_clears_and_cauldron(&mut server, anchor, player, extinguish_sound);
}

fn assert_freeze_clears_and_cauldron(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    extinguish_sound: WireId<SoundEvent>,
) {
    let boots_location = ResourceLocationBuf::read("minecraft:leather_boots")
        .expect("leather_boots resource location should parse");
    let boots_id = {
        let registries = server.world().resource::<CurrentRegistries>();
        registries
            .entries::<Item>()
            .expect("Item registry should load")
            .by_name(&boots_location)
            .expect("leather_boots item should exist")
    };
    let mut boots = OwnEquipment::default();
    let stack = ProtoStack {
        count: 1,
        id: boots_id.into(),
        ..Default::default()
    };
    boots.0.set(EquipSlot::Feet, Some(stack));
    *server
        .world()
        .get_mut::<OwnEquipment>(player)
        .expect("player should carry OwnEquipment") = boots;
    let _ = server.ticks(anchor, 40);
    assert_eq!(
        state_of(server, player).ticks_frozen,
        60,
        "wearing leather_boots in powder_snow should thaw 2 ticks per tick to 60"
    );

    *server
        .world()
        .get_mut::<OwnEquipment>(player)
        .expect("player should carry OwnEquipment") = OwnEquipment::default();
    let _ = cmd(server, anchor, "gamemode spectator @s");
    state_mut(server, player, |state| state.ticks_frozen = 60);
    let _ = server.ticks(anchor, 1);
    assert_eq!(
        state_of(server, player).ticks_frozen,
        58,
        "Spectator mode should thaw ticks_frozen by 2 per tick"
    );
    let _ = cmd(server, anchor, "gamemode survival @s");

    set_named_block(server, BlockPos::new(0, 64, 3), "minecraft:fire");
    let fire_clear_packets = cmd(server, anchor, "tp @s 0.5 64 3.5");
    assert_eq!(
        state_of(server, player).ticks_frozen,
        0,
        "fire contact should clear ticks_frozen immediately to 0"
    );
    assert!(
        has_sound(&fire_clear_packets, extinguish_sound),
        "fire clearing freeze ticks should play extinguish sound"
    );

    assert_burning_melts_powder_snow(server, anchor, player, extinguish_sound);

    let powder_snow_cauldron = {
        let blocks = server.world().resource::<Blocks>();
        blocks
            .blocks()
            .iter()
            .find(|entry| entry.identifier.as_str() == "minecraft:powder_snow_cauldron")
            .and_then(|entry| {
                (0..entry.state_count)
                    .filter_map(|offset| entry.state_at(offset))
                    .find(|&id| {
                        entry
                            .value_of(id, "level")
                            .is_some_and(|value| value.renders_to("3"))
                    })
            })
            .expect("powder_snow_cauldron[level=3] state should exist")
    };
    set_block(server, BlockPos::new(0, 64, 3), powder_snow_cauldron);
    let _ = cmd(server, anchor, "tp @s 0.5 64 3.5");
    state_mut(server, player, |state| {
        state.fire = 60;
        state.ticks_frozen = 0;
    });
    let _ = server.ticks(anchor, 1);
    assert_eq!(
        state_of(server, player).fire,
        -20,
        "powder_snow_cauldron should extinguish fire ticks back to -20"
    );
    assert_eq!(
        state_of(server, player).ticks_frozen,
        0,
        "powder_snow_cauldron should never freeze the player"
    );
}

fn assert_burning_melts_powder_snow(
    server: &mut PlayServer,
    anchor: Entity,
    player: Entity,
    extinguish_sound: WireId<SoundEvent>,
) {
    let _ = server.ticks(anchor, 22);
    assert!(
        state_of(server, player).fire > 0,
        "standing in fire should keep fire ticks positive before melting snow"
    );
    set_block(server, BlockPos::new(0, 64, 3), BlockStateId::AIR);
    let melt_packets = cmd(server, anchor, "tp @s -3.5 64 0.5");
    let _ = server.ticks(anchor, 2);
    assert_eq!(
        block_at(server, BlockPos::new(-4, 64, 0)),
        BlockStateId::AIR,
        "burning player touching powder_snow at y=64 should melt it to air"
    );
    assert_eq!(
        block_at(server, BlockPos::new(-4, 65, 0)),
        BlockStateId::AIR,
        "burning player touching powder_snow at y=65 should melt it to air"
    );
    assert_eq!(
        state_of(server, player).fire,
        -20,
        "melting powder_snow should extinguish burning player fire to -20"
    );
    assert!(
        has_sound(&melt_packets, extinguish_sound),
        "melting powder_snow should play extinguish sound"
    );
}
