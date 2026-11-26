use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::prelude::With;
use bevy_math::DVec3;
use bevy_time::TimeUpdateStrategy;
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_block::definition::schema::PropertyValue;
use mcrs_minecraft_command_engine::CommandPlugin;
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_command_world::WorldCommandsPlugin;
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::bus::PacketPayload;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::server_edit::ServerStateHostPlugin;
use mcrs_minecraft_engine_world::block_update::BlockSetRequest;
use mcrs_minecraft_engine_world::world::dimension::Dimension;
use mcrs_minecraft_engine_world::world_border::DimensionWorldBorder;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol::{Bounded, Decode, Packet};
use mcrs_minecraft_protocol_game::MoveFlags;
use mcrs_minecraft_protocol_game::clientbound::ClientboundDamageEvent;
use mcrs_minecraft_protocol_game::serverbound::{
    PlayerInputFlags, ServerboundAcceptTeleportation, ServerboundChatCommand,
    ServerboundMovePlayerPos, ServerboundPlayerInput, ServerboundPlayerLoaded,
};
use mcrs_minecraft_registries::{Ref, WireId};
use mcrs_minecraft_server::{
    VanillaBlocksPlugin, VanillaDurabilityPlugin, VanillaEntityAttributesPlugin,
    VanillaEntityPhysicsPlugin, VanillaEnvDamagePlugin, VanillaPlayerPosePlugin,
    VanillaRespawnPlugin, VanillaWeatherPlugin,
};
use mcrs_minecraft_simulation::BlockUpdateFlags;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_minecraft_vanilla_env_damage::BakedEnvDamage;
use mcrs_voxel::{BlockPos, BlockStateId};
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
