#![expect(clippy::expect_used, reason = "integration tests")]

mod hazards;

use bevy_app::{App, FixedUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource};
use bevy_ecs::system::SystemState;
use glam::DVec3;
use mcrs_minecraft_block::block_keys::Block;
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_collision_table::collision_table;
use mcrs_minecraft_damage::{Defence, HitRecord, Vitals};
use mcrs_minecraft_engine::PlayerMoved;
use mcrs_minecraft_engine::ShiftKey;
use mcrs_minecraft_engine::ability::PlayerFlying;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload, PacketTarget};
use mcrs_minecraft_engine::client_on_ground::ClientOnGround;
use mcrs_minecraft_engine::network_id;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_damage::{DamagePlugin, DamageSet, PendingHits};
use mcrs_minecraft_engine_physics::PhysicsTables;
use mcrs_minecraft_engine_player_vitals::PlayerVitalsPlugin;
use mcrs_minecraft_engine_registries::test_corpus;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::palette;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::world::dimension::{
    Dimension, DimensionPlugin, DimensionTypeConfig,
};
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_engine_world::world::storage::view::WorldViewMut;
use mcrs_minecraft_engine_world::world_border::{DimensionWorldBorder, WorldBorderBounds};
use mcrs_minecraft_entity::attribute_bases;
use mcrs_minecraft_entity::entity_type_keys::EntityType;
use mcrs_minecraft_entity_attribute::AttributeInstances;
use mcrs_minecraft_entity_key::EntityUuid;
use mcrs_minecraft_env_damage::FallBlockKind;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_registries::registry_load::test_registries;
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_physics::Tables;
use mcrs_minecraft_pose::Pose;
use mcrs_minecraft_protocol_game::clientbound::ClientboundEntityEvent;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_vanilla_env_damage::{
    BakedEnvDamage, EnvBlockTable, EnvContactBlock, VanillaEnvDamagePlugin,
};
use mcrs_minecraft_volume::BlockVolumeMut;
use mcrs_voxel::{BlockPos, BlockStateId, SectionPos};

#[derive(Resource, Default)]
pub(crate) struct RecordedPendingOrder(pub(crate) Vec<u32>);

fn record_pending_order(pending: Res<PendingHits>, mut order: ResMut<RecordedPendingOrder>) {
    order.0.clear();
    let mut requests = pending.requests().to_vec();
    requests.sort_by_key(|request| (request.writer, request.tick, request.sequence));
    order.0.extend(
        requests
            .into_iter()
            .map(|request| request.hit.source.damage_type),
    );
}

fn spawn_player(app: &mut App, session: u64, translation: DVec3) -> Entity {
    let bases = attribute_bases(EntityType::Player).expect("player attribute bases");
    app.world_mut()
        .spawn((
            Player,
            Owner(PlayerSession(session)),
            EntityUuid::from_u128(u128::from(session)),
            Transform::from_translation(translation),
            Pose::Standing,
            PlayerGameMode(GameMode::Survival),
            PlayerFlying(false),
            ClientOnGround(true),
            ShiftKey(false),
            Vitals::new(20.0),
            HitRecord::default(),
            Defence::default(),
            AttributeInstances::new(bases),
            PlayerFileState::default(),
        ))
        .id()
}

pub(crate) fn build_test_app() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        DimensionPlugin,
        DamagePlugin,
        PlayerVitalsPlugin,
        VanillaEnvDamagePlugin,
    ));
    app.add_message::<OutboundPlayerPacket>();
    app.add_message::<PlayerSignal>();
    app.init_resource::<BirthClock>();
    app.init_resource::<GameRules>();
    app.init_resource::<DimensionWorldBorder>();
    app.init_resource::<RecordedPendingOrder>();
    app.add_systems(FixedUpdate, record_pending_order.in_set(DamageSet::Filter));
    let registries = test_registries();
    let (blocks, _) = test_corpus();
    let table = collision_table(blocks, &registries.tags::<Block>().expect("block tags"));
    app.insert_resource(CurrentRegistries::new(registries.clone()));
    app.insert_resource(blocks.clone());
    app.insert_resource(PhysicsTables::new(Tables { collision: table }));

    let dimension = app
        .world_mut()
        .spawn((
            Dimension,
            SectionIndex::new(),
            DimensionTypeConfig::new(-64, 384),
        ))
        .id();
    let player = spawn_player(&mut app, 1, DVec3::new(0.5, 64.0, 0.5));
    (app, dimension, player)
}

pub(crate) fn set_block(app: &mut App, dimension: Entity, position: BlockPos, state: BlockStateId) {
    let section_pos = SectionPos::from(position);
    let already_loaded = app
        .world()
        .entity(dimension)
        .get::<SectionIndex>()
        .and_then(|index| index.get(section_pos))
        .is_some();
    if !already_loaded {
        let chunk_entity = app.world_mut().spawn_empty().id();
        let mut index = app
            .world_mut()
            .entity_mut(dimension)
            .take::<SectionIndex>()
            .expect("dimension has SectionIndex");
        index.insert(section_pos, chunk_entity);
        index.land(
            section_pos,
            chunk_entity,
            palette::air_section(),
            None,
            false,
        );
        app.world_mut().entity_mut(dimension).insert(index);
    }
    let mut state_access: SystemState<WorldViewMut<'_, '_>> = SystemState::new(app.world_mut());
    let mut view_mut = state_access
        .get_mut(app.world_mut())
        .expect("world view mut");
    let mut view = view_mut.get_mut(dimension).expect("dimension view mut");
    view.set_block(position, state).expect("set test block");
}

pub(crate) fn block_property_state(
    blocks: &Blocks,
    identifier: &str,
    key: &str,
    expected: &str,
) -> BlockStateId {
    blocks
        .blocks()
        .iter()
        .find(|entry| entry.identifier.as_str() == identifier)
        .and_then(|entry| {
            (0..entry.state_count)
                .filter_map(|offset| entry.state_at(offset))
                .find(|&id| {
                    entry
                        .value_of(id, key)
                        .is_some_and(|value| value.renders_to(expected))
                })
        })
        .expect("block state matching property")
}

fn icicle_up_tip_state(blocks: &Blocks) -> BlockStateId {
    blocks
        .blocks()
        .iter()
        .find(|entry| entry.identifier.as_str() == "minecraft:icicle")
        .and_then(|entry| {
            (0..entry.state_count)
                .filter_map(|offset| entry.state_at(offset))
                .find(|&id| {
                    entry
                        .value_of(id, "vertical_direction")
                        .is_some_and(|value| value.renders_to("up"))
                        && entry
                            .value_of(id, "thickness")
                            .is_some_and(|value| value.renders_to("tip"))
                })
        })
        .expect("icicle up tip state")
}

fn tick(app: &mut App) {
    let next = app.world().resource::<BirthClock>().0.saturating_add(1);
    app.world_mut().insert_resource(BirthClock(next));
    app.world_mut().run_schedule(FixedUpdate);
}

fn advance_ticks(app: &mut App, count: usize) {
    for _ in 0..count {
        tick(app);
    }
}

fn fall_move(app: &mut App, player: Entity, delta_y: f64, now_on_ground: bool) {
    app.world_mut().write_message(PlayerMoved {
        entity: player,
        delta: DVec3::new(0.0, delta_y, 0.0),
        was_on_ground: false,
        now_on_ground,
    });
    tick(app);
}

#[test]
fn env_damage_tables_bake_vanilla_blocks_and_border() {
    let mut app = App::new();
    app.add_plugins((
        DimensionPlugin,
        DamagePlugin,
        PlayerVitalsPlugin,
        VanillaEnvDamagePlugin,
    ));
    app.add_message::<PlayerSignal>();
    app.init_resource::<BirthClock>();
    let (blocks, _) = test_corpus();
    app.insert_resource(CurrentRegistries::new(test_registries().clone()));
    app.insert_resource(blocks.clone());
    app.world_mut().run_schedule(FixedUpdate);

    let baked = app.world().resource::<BakedEnvDamage>();
    assert!(
        baked.fall.is_some() && baked.out_of_world.is_some() && baked.outside_border.is_some(),
        "env damage types"
    );
    assert!(
        baked.player_big_fall.is_some() && baked.honey_slide.is_some(),
        "env sound events"
    );

    let table = app.world().resource::<EnvBlockTable>();
    let cactus = blocks.default_state("minecraft:cactus").expect("cactus");
    let hay = blocks.default_state("minecraft:hay_block").expect("hay");
    let magma = blocks
        .default_state("minecraft:magma_block")
        .expect("magma");
    assert_eq!(
        table.get(cactus).expect("cactus entry").contact,
        EnvContactBlock::Cactus,
        "cactus contact"
    );
    assert_eq!(
        table.get(hay).expect("hay entry").fall_kind,
        FallBlockKind::HayBlock,
        "hay fall kind"
    );
    assert_eq!(
        table.get(magma).expect("magma entry").contact,
        EnvContactBlock::MagmaBlock,
        "magma contact"
    );
    assert_eq!(
        table
            .get(icicle_up_tip_state(blocks))
            .expect("icicle entry")
            .fall_kind,
        FallBlockKind::PointedDripstoneTip,
        "icicle up tip uses stalagmite fall kind"
    );

    let border = DimensionWorldBorder::with_center_and_size(0.5, 0.5, 20.0);
    assert_eq!(
        border.bounds(),
        WorldBorderBounds {
            min_x: -9.5,
            min_z: -9.5,
            max_x: 10.5,
            max_z: 10.5,
        },
        "border bounds"
    );
}

#[test]
fn fall_damage_gamerule_and_icicle_in_plugin() {
    let (mut app, dimension, player) = build_test_app();
    let blocks = app.world().resource::<Blocks>().clone();
    let icicle = icicle_up_tip_state(&blocks);
    tick(&mut app);

    fall_move(&mut app, player, -10.0, true);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        13.0,
        "10-block fall removes 7 HP"
    );

    set_block(&mut app, dimension, BlockPos::new(0, 63, 0), icicle);
    advance_ticks(&mut app, 20);
    fall_move(&mut app, player, -6.0, true);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        2.0,
        "6-block fall on icicle tip deals doubled stalagmite damage (11 HP)"
    );

    app.world_mut()
        .resource_mut::<GameRules>()
        .set(GameRule::FallDamage, 0);
    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 63, 0),
        BlockStateId::AIR,
    );
    fall_move(&mut app, player, -10.0, true);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        2.0,
        "fall_damage=false suppresses damage"
    );
}

#[test]
fn silent_honey_slide_sends_entity_event_beyond_sound_range() {
    let (mut app, dimension, player) = build_test_app();
    let far_viewer = spawn_player(&mut app, 2, DVec3::new(24.5, 64.0, 0.5));
    let honey = app
        .world()
        .resource::<Blocks>()
        .default_state("minecraft:honey_block")
        .expect("honey_block");
    tick(&mut app);

    set_block(&mut app, dimension, BlockPos::new(0, 63, 0), honey);
    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerFileState>()
        .expect("file state")
        .silent = true;
    app.world_mut()
        .resource_mut::<Messages<OutboundPlayerPacket>>()
        .clear();
    fall_move(&mut app, player, -4.0, true);

    let outbound: Vec<OutboundPlayerPacket> = app
        .world_mut()
        .resource_mut::<Messages<OutboundPlayerPacket>>()
        .drain()
        .collect();
    assert!(
        outbound
            .iter()
            .all(|packet| !matches!(packet.data, PacketPayload::Sound(_))),
        "silent honey slide emits no sound packets"
    );
    let actor_network_id = network_id::player_network_id(PlayerSession(1));
    let expected_viewer = app
        .world()
        .entity(far_viewer)
        .get::<Owner>()
        .expect("viewer owner")
        .0;
    let slide_packets: Vec<&OutboundPlayerPacket> = outbound
        .iter()
        .filter(|packet| {
            matches!(
                packet.data,
                PacketPayload::EntityEvent(ClientboundEntityEvent {
                    entity_id,
                    entity_status: ClientboundEntityEvent::HONEY_SLIDE,
                }) if entity_id == actor_network_id
            )
        })
        .collect();
    assert_eq!(
        slide_packets.len(),
        1,
        "silent honey slide emits entity event"
    );
    assert!(
        matches!(
            &slide_packets[0].target,
            PacketTarget::PlayerSet(sessions)
                if sessions.contains(&PlayerSession(1)) && sessions.contains(&expected_viewer)
        ),
        "honey slide event reaches tracking viewers beyond sound range"
    );
}

#[test]
fn suffocation_and_border_suppression_in_plugin() {
    let (mut app, dimension, player) = build_test_app();
    let stone = app
        .world()
        .resource::<Blocks>()
        .default_state("minecraft:stone")
        .expect("stone");
    tick(&mut app);
    let baked = *app.world().resource::<BakedEnvDamage>();

    set_block(&mut app, dimension, BlockPos::new(0, 65, 0), stone);
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        19.0,
        "stone enclosing eye box deals 1 in_wall damage"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 65, 0),
        BlockStateId::AIR,
    );
    advance_ticks(&mut app, 20);
    app.world_mut()
        .insert_resource(DimensionWorldBorder::with_center_and_size(0.5, 0.5, 20.0));
    app.world_mut()
        .get_mut::<Transform>(player)
        .expect("transform")
        .translation
        .x = 20.5;
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        18.0,
        "outside border beyond safe buffer deals 1 damage"
    );

    set_block(&mut app, dimension, BlockPos::new(20, 65, 0), stone);
    advance_ticks(&mut app, 20);
    assert_eq!(
        app.world().resource::<RecordedPendingOrder>().0,
        vec![baked.in_wall.expect("in_wall")],
        "suffocation suppresses outside_border hit in the same tick"
    );
}

#[test]
fn contact_hazards_magma_crouch_and_fire_gamerule_in_plugin() {
    let (mut app, dimension, player) = build_test_app();
    let blocks = app.world().resource::<Blocks>().clone();
    let cactus = blocks.default_state("minecraft:cactus").expect("cactus");
    let lit_campfire = blocks
        .default_state("minecraft:campfire")
        .expect("campfire");
    let unlit_campfire = block_property_state(&blocks, "minecraft:campfire", "lit", "false");
    let soul_campfire = blocks
        .default_state("minecraft:soul_campfire")
        .expect("soul_campfire");
    let magma = blocks
        .default_state("minecraft:magma_block")
        .expect("magma_block");
    tick(&mut app);

    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), cactus);
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        19.0,
        "cactus deals 1 HP"
    );

    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), unlit_campfire);
    advance_ticks(&mut app, 20);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        19.0,
        "unlit campfire deals 0 HP"
    );

    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), lit_campfire);
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        18.0,
        "lit campfire deals 1 HP"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 64, 0),
        BlockStateId::AIR,
    );
    advance_ticks(&mut app, 10);
    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), soul_campfire);
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        16.0,
        "lit soul campfire deals 2 HP"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 64, 0),
        BlockStateId::AIR,
    );
    set_block(&mut app, dimension, BlockPos::new(0, 63, 0), magma);
    app.world_mut()
        .entity_mut(player)
        .get_mut::<ShiftKey>()
        .expect("shift")
        .0 = true;
    advance_ticks(&mut app, 10);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        16.0,
        "crouching spares hot_floor damage on magma_block"
    );

    app.world_mut()
        .entity_mut(player)
        .get_mut::<ShiftKey>()
        .expect("shift")
        .0 = false;
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        15.0,
        "standing on magma_block deals 1 hot_floor HP"
    );

    app.world_mut()
        .resource_mut::<GameRules>()
        .set(GameRule::FireDamage, 0);
    advance_ticks(&mut app, 12);
    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), lit_campfire);
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        15.0,
        "fire_damage=false suppresses magma hot_floor and lit campfire damage"
    );
}

#[test]
fn sweet_berry_bush_and_water_reset_in_plugin() {
    let (mut app, dimension, player) = build_test_app();
    let blocks = app.world().resource::<Blocks>().clone();
    let age_0 = block_property_state(&blocks, "minecraft:sweet_berry_bush", "age", "0");
    let age_1 = block_property_state(&blocks, "minecraft:sweet_berry_bush", "age", "1");
    let water = blocks.default_state("minecraft:water").expect("water");
    tick(&mut app);

    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), age_0);
    app.world_mut().write_message(PlayerMoved {
        entity: player,
        delta: DVec3::new(0.05, 0.0, 0.0),
        was_on_ground: true,
        now_on_ground: true,
    });
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        20.0,
        "sweet berry bush age 0 never damages"
    );

    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), age_1);
    app.world_mut().write_message(PlayerMoved {
        entity: player,
        delta: DVec3::new(0.0029, 0.0, 0.0),
        was_on_ground: true,
        now_on_ground: true,
    });
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        20.0,
        "movement below 0.003 inside sweet berry bush deals no damage"
    );

    app.world_mut().write_message(PlayerMoved {
        entity: player,
        delta: DVec3::new(0.003, 0.0, 0.0),
        was_on_ground: true,
        now_on_ground: true,
    });
    tick(&mut app);
    assert_eq!(
        app.world().get::<Vitals>(player).expect("vitals").health,
        19.0,
        "movement >= 0.003 in mature sweet berry bush deals 1 HP"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 64, 0),
        BlockStateId::AIR,
    );
    fall_move(&mut app, player, -8.0, false);
    assert_eq!(
        app.world()
            .entity(player)
            .get::<PlayerFileState>()
            .expect("file state")
            .fall_distance,
        8.0,
        "airborne downward step accumulates fall_distance"
    );
    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), water);
    tick(&mut app);
    assert_eq!(
        app.world()
            .entity(player)
            .get::<PlayerFileState>()
            .expect("file state")
            .fall_distance,
        0.0,
        "standing in water resets accumulated fall_distance"
    );
}

#[test]
fn env_damage_order_void_wall_or_border_hot_floor_contact_then_fall() {
    let (mut app, dimension, player) = build_test_app();
    let blocks = app.world().resource::<Blocks>().clone();
    let stone = blocks.default_state("minecraft:stone").expect("stone");
    let magma = blocks
        .default_state("minecraft:magma_block")
        .expect("magma_block");
    let cactus = blocks.default_state("minecraft:cactus").expect("cactus");
    tick(&mut app);
    let baked = *app.world().resource::<BakedEnvDamage>();

    app.world_mut()
        .insert_resource(DimensionWorldBorder::with_center_and_size(0.5, 0.5, 20.0));
    set_block(&mut app, dimension, BlockPos::new(0, 65, 0), stone);
    set_block(&mut app, dimension, BlockPos::new(0, 63, 0), magma);
    set_block(&mut app, dimension, BlockPos::new(1, 64, 0), cactus);
    app.world_mut()
        .get_mut::<Transform>(player)
        .expect("transform")
        .translation = DVec3::new(0.8, 64.0, 0.5);
    fall_move(&mut app, player, -10.0, true);
    assert_eq!(
        app.world().resource::<RecordedPendingOrder>().0,
        vec![
            baked.in_wall.expect("in_wall"),
            baked.hot_floor.expect("hot_floor"),
            baked.cactus.expect("cactus"),
            baked.fall.expect("fall"),
        ],
        "intra-tick hits order in_wall -> hot_floor -> cactus -> fall"
    );

    advance_ticks(&mut app, 20);
    app.world_mut()
        .entity_mut(player)
        .get_mut::<Vitals>()
        .expect("vitals")
        .health = 20.0;
    app.world_mut()
        .get_mut::<Transform>(player)
        .expect("transform")
        .translation = DVec3::new(20.5, -129.0, 0.5);
    fall_move(&mut app, player, -10.0, true);
    assert_eq!(
        app.world().resource::<RecordedPendingOrder>().0,
        vec![
            baked.out_of_world.expect("out_of_world"),
            baked.outside_border.expect("outside_border"),
            baked.fall.expect("fall"),
        ],
        "void precedes outside_border which precedes fall"
    );
}
