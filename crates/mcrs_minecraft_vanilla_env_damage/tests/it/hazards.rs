use bevy_app::{App, FixedUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use glam::DVec3;
use mcrs_minecraft_biome_file::BiomeFile;
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_damage::{HitRecord, Vitals};
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload};
use mcrs_minecraft_engine::client_on_ground::ClientOnGround;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine::player_state::{Effect, PlayerFileState};
use mcrs_minecraft_engine::{OwnEquipment, PlayerMoved, ShiftKey};
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::block_update::BlockSetRequest;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::palette::BiomePalette;
use mcrs_minecraft_engine_world::world::dimension::{HasSkyLight, HasWeather};
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_entity_attribute::{AttributeInstances, InstanceEdit};
use mcrs_minecraft_equipment::EquipSlot;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_registries::{CurrentRegistries, Ref, WireId};
use mcrs_minecraft_replication::MobSignal;
use mcrs_minecraft_resource::ResourceLocationBuf;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_minecraft_vanilla_env_damage::{BakedEnvDamage, EnvTickState};
use mcrs_voxel::{BlockPos, BlockStateId, SectionPos};

use crate::{RecordedPendingOrder, block_property_state, build_test_app, set_block};

fn default_block(app: &App, name: &str) -> BlockStateId {
    app.world()
        .resource::<Blocks>()
        .default_state(name)
        .expect("vanilla block state should exist in test corpus")
}

fn state_of(app: &App, player: Entity) -> &PlayerFileState {
    app.world()
        .get::<PlayerFileState>(player)
        .expect("player entity should carry PlayerFileState")
}

fn state_mut(app: &mut App, player: Entity) -> bevy_ecs::prelude::Mut<'_, PlayerFileState> {
    app.world_mut()
        .get_mut::<PlayerFileState>(player)
        .expect("player entity should carry mutable PlayerFileState")
}

fn health_of(app: &App, player: Entity) -> f32 {
    app.world()
        .get::<Vitals>(player)
        .expect("player entity should carry Vitals")
        .health
}

fn set_game_mode(app: &mut App, player: Entity, mode: GameMode) {
    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerGameMode>()
        .expect("player entity should carry PlayerGameMode")
        .0 = mode;
}

fn active_effect(id: &str, duration: i32) -> Effect {
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

fn tick(app: &mut App, count: usize) {
    for _ in 0..count {
        let next = app.world().resource::<BirthClock>().0.saturating_add(1);
        app.world_mut().insert_resource(BirthClock(next));
        app.world_mut().run_schedule(FixedUpdate);
    }
}

fn start_hazard_app() -> (App, Entity, Entity) {
    let (mut app, dimension, player) = build_test_app();
    tick(&mut app, 1);
    (app, dimension, player)
}

fn drain_signals(app: &mut App) -> Vec<MobSignal> {
    app.world_mut()
        .resource_mut::<Messages<PlayerSignal>>()
        .drain()
        .map(|entry| entry.signal)
        .collect()
}

fn drain_sound_ids(app: &mut App) -> Vec<WireId<SoundEvent>> {
    app.world_mut()
        .resource_mut::<Messages<OutboundPlayerPacket>>()
        .drain()
        .filter_map(|packet| match packet.data {
            PacketPayload::Sound(sound) => match sound.sound_event {
                Ref::Id(id) => Some(id),
                Ref::Inline(_) => None,
            },
            _ => None,
        })
        .collect()
}

fn drain_block_sets(app: &mut App) -> Vec<(BlockPos, BlockStateId)> {
    app.world_mut()
        .resource_mut::<Messages<BlockSetRequest>>()
        .drain()
        .map(|request| (request.pos, request.new_state))
        .collect()
}

fn fill_column(app: &mut App, dimension: Entity, block_x: i32, block_z: i32, state: BlockStateId) {
    set_block(app, dimension, BlockPos::new(block_x, 64, block_z), state);
    set_block(app, dimension, BlockPos::new(block_x, 65, block_z), state);
}

#[test]
fn drowning_cycle_bubbles_gamerule_and_bubble_column() {
    let (mut app, dimension, player) = start_hazard_app();
    let water = default_block(&app, "minecraft:water");
    let bubble_column = default_block(&app, "minecraft:bubble_column");
    fill_column(&mut app, dimension, 0, 0, water);

    tick(&mut app, 300);
    assert_eq!(
        state_of(&app, player).air,
        0,
        "submerged player air should reach 0 after 300 ticks"
    );

    let _ = drain_signals(&mut app);
    tick(&mut app, 20);
    assert_eq!(
        state_of(&app, player).air,
        0,
        "air should reset to 0 when drowning damage triggers"
    );
    assert!(
        (health_of(&app, player) - 18.0).abs() < 1.0e-4,
        "drowning hit should reduce player health from 20.0 to 18.0"
    );
    assert!(
        drain_signals(&mut app).contains(&MobSignal::DrownBubbles),
        "drowning hit should emit MobSignal::DrownBubbles"
    );

    app.world_mut()
        .resource_mut::<GameRules>()
        .set(GameRule::DrowningDamage, 0);
    tick(&mut app, 20);
    assert!(
        (health_of(&app, player) - 18.0).abs() < 1.0e-4,
        "disabling DrowningDamage gamerule should keep player health at 18.0"
    );
    assert!(
        drain_signals(&mut app).contains(&MobSignal::DrownBubbles),
        "disabling DrowningDamage should still emit DrownBubbles signals"
    );
    app.world_mut()
        .resource_mut::<GameRules>()
        .set(GameRule::DrowningDamage, 1);

    state_mut(&mut app, player).air = 100;
    set_block(&mut app, dimension, BlockPos::new(0, 65, 0), bubble_column);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).air,
        104,
        "bubble_column should regenerate player air by +4 per tick"
    );
}

#[test]
fn drowning_effects_nautilus_spectator_and_oxygen_bonus() {
    let (mut app, dimension, player) = start_hazard_app();
    let water = default_block(&app, "minecraft:water");
    fill_column(&mut app, dimension, 0, 0, water);

    state_mut(&mut app, player).air = 0;
    state_mut(&mut app, player)
        .active_effects
        .push(active_effect("minecraft:water_breathing", 200));
    tick(&mut app, 5);
    assert_eq!(
        state_of(&app, player).air,
        20,
        "WaterBreathing should refill submerged air by +4 per tick"
    );
    state_mut(&mut app, player).air = 295;
    tick(&mut app, 2);
    assert_eq!(
        state_of(&app, player).air,
        300,
        "WaterBreathing air refill should clamp at 300 maximum air"
    );

    state_mut(&mut app, player).active_effects.clear();
    state_mut(&mut app, player).air = 200;
    state_mut(&mut app, player)
        .active_effects
        .push(active_effect("minecraft:conduit_power", 200));
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).air,
        204,
        "ConduitPower should refill submerged air by +4 per tick"
    );

    state_mut(&mut app, player).active_effects.clear();
    state_mut(&mut app, player).air = 120;
    state_mut(&mut app, player)
        .active_effects
        .push(active_effect("minecraft:breath_of_the_nautilus", 200));
    tick(&mut app, 5);
    assert_eq!(
        state_of(&app, player).air,
        120,
        "BreathOfTheNautilus alone should hold submerged air unchanged"
    );

    state_mut(&mut app, player)
        .active_effects
        .push(active_effect("minecraft:water_breathing", 200));
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).air,
        124,
        "WaterBreathing together with BreathOfTheNautilus should refill air by +4 per tick"
    );

    state_mut(&mut app, player).active_effects.clear();
    state_mut(&mut app, player).air = 120;
    set_game_mode(&mut app, player, GameMode::Spectator);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).air,
        124,
        "Spectator mode submerged should refill air by +4 per tick"
    );

    set_game_mode(&mut app, player, GameMode::Survival);
    let _ = app
        .world_mut()
        .get_mut::<AttributeInstances>(player)
        .expect("player should carry AttributeInstances")
        .apply(Attribute::OxygenBonus, &InstanceEdit::SetBase(4.0));
    state_mut(&mut app, player).air = 100;
    tick(&mut app, 20);
    assert!(
        state_of(&app, player).air > 80,
        "OxygenBonus attribute should slow underwater air depletion"
    );
}

#[test]
fn fire_lava_simultaneous_hits_and_fire_resistance() {
    let (mut app, dimension, player) = start_hazard_app();
    let fire = default_block(&app, "minecraft:fire");
    let lava = default_block(&app, "minecraft:lava");
    let baked = *app.world().resource::<BakedEnvDamage>();
    let burn_id = WireId::from(baked.generic_burn.expect("generic_burn sound should bake"));

    assert_eq!(
        state_of(&app, player).fire,
        -20,
        "idle fire ticks should normalize to the -20 grace value"
    );

    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), fire);
    tick(&mut app, 20);
    assert_eq!(
        state_of(&app, player).fire,
        160,
        "standing in fire should ignite player to 160 ticks"
    );

    set_block(&mut app, dimension, BlockPos::new(1, 64, 0), lava);
    app.world_mut()
        .get_mut::<Transform>(player)
        .expect("player should carry Transform")
        .translation = DVec3::new(0.85, 64.0, 0.5);
    app.world_mut()
        .resource_mut::<RecordedPendingOrder>()
        .0
        .clear();
    tick(&mut app, 1);
    assert_eq!(
        app.world().resource::<RecordedPendingOrder>().0,
        vec![
            baked.in_fire.expect("in_fire damage type should bake"),
            baked.lava.expect("lava damage type should bake"),
        ],
        "touching fire and lava simultaneously should emit both in_fire and lava hits"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(1, 64, 0),
        BlockStateId::AIR,
    );
    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 64, 0),
        BlockStateId::AIR,
    );
    app.world_mut()
        .get_mut::<Transform>(player)
        .expect("player should carry Transform")
        .translation = DVec3::new(0.5, 64.0, 0.5);
    tick(&mut app, 10);
    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), lava);
    let _ = drain_sound_ids(&mut app);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).fire,
        300,
        "standing in lava should ignite player to 300 ticks"
    );
    assert!(
        drain_sound_ids(&mut app).contains(&burn_id),
        "lava damage hit should emit generic burn sound"
    );

    let fire_resistance = active_effect("minecraft:fire_resistance", 200);
    state_mut(&mut app, player)
        .active_effects
        .push(fire_resistance);
    let health_fire_resistance = health_of(&app, player);
    tick(&mut app, 12);
    assert!(
        (health_of(&app, player) - health_fire_resistance).abs() < 1.0e-4,
        "FireResistance should suppress lava and burning damage"
    );
    assert!(
        !drain_sound_ids(&mut app).contains(&burn_id),
        "FireResistance should suppress generic burn sound when damage is cancelled"
    );
}

#[test]
fn water_powder_snow_cauldron_and_spectator_fire() {
    let (mut app, dimension, player) = start_hazard_app();
    let blocks = app.world().resource::<Blocks>().clone();
    let water = default_block(&app, "minecraft:water");
    let powder_snow = default_block(&app, "minecraft:powder_snow");
    let powder_snow_cauldron =
        block_property_state(&blocks, "minecraft:powder_snow_cauldron", "level", "3");
    let baked = *app.world().resource::<BakedEnvDamage>();
    let extinguish_id = WireId::from(
        baked
            .extinguish_fire
            .expect("extinguish_fire sound should bake"),
    );

    state_mut(&mut app, player).fire = 120;
    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), water);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).fire,
        -20,
        "water contact should extinguish fire ticks back to -20"
    );
    assert!(
        drain_sound_ids(&mut app).contains(&extinguish_id),
        "water extinguishing active fire should emit extinguish sound"
    );

    state_mut(&mut app, player).fire = 60;
    tick(&mut app, 1);
    assert!(
        !drain_sound_ids(&mut app).contains(&extinguish_id),
        "extinguish sound cooldown should prevent immediate re-emission"
    );

    tick(&mut app, 22);
    let _ = drain_sound_ids(&mut app);
    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), powder_snow);
    state_mut(&mut app, player).fire = 80;
    let _ = drain_block_sets(&mut app);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).fire,
        -20,
        "powder_snow should extinguish player fire back to -20"
    );
    assert!(
        drain_block_sets(&mut app).contains(&(BlockPos::new(0, 64, 0), BlockStateId::AIR)),
        "burning player in powder_snow should request melting powder_snow to air"
    );
    assert!(
        drain_sound_ids(&mut app).contains(&extinguish_id),
        "powder_snow extinguishing fire should emit extinguish sound"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 64, 0),
        powder_snow_cauldron,
    );
    state_mut(&mut app, player).fire = -20;
    state_mut(&mut app, player).ticks_frozen = 0;
    tick(&mut app, 5);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        0,
        "unburning player in powder_snow_cauldron should never gain freeze ticks"
    );
    state_mut(&mut app, player).ticks_frozen = 20;
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        18,
        "unburning player in powder_snow_cauldron should thaw positive freeze ticks by 2 per tick"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 64, 0),
        BlockStateId::AIR,
    );
    set_game_mode(&mut app, player, GameMode::Spectator);
    state_mut(&mut app, player).fire = 10;
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).fire,
        9,
        "Spectator mode should decrement positive fire ticks by 1"
    );
    state_mut(&mut app, player).fire = 0;
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).fire,
        0,
        "Spectator mode should leave zero fire ticks unnormalized"
    );
}

#[test]
fn cauldron_extinguish_lowers_cauldron_levels() {
    let (mut app, dimension, player) = start_hazard_app();
    let blocks = app.world().resource::<Blocks>().clone();
    let empty_cauldron = default_block(&app, "minecraft:cauldron");
    let water_level_one = block_property_state(&blocks, "minecraft:water_cauldron", "level", "1");
    let water_level_two = block_property_state(&blocks, "minecraft:water_cauldron", "level", "2");
    let water_level_three = block_property_state(&blocks, "minecraft:water_cauldron", "level", "3");
    let snow_level_one =
        block_property_state(&blocks, "minecraft:powder_snow_cauldron", "level", "1");
    let snow_level_three =
        block_property_state(&blocks, "minecraft:powder_snow_cauldron", "level", "3");
    let cauldron_position = BlockPos::new(0, 64, 0);

    for (initial_state, expected_next, description) in [
        (
            water_level_three,
            water_level_two,
            "burning player touching water_cauldron[level=3] should lower it to water_cauldron[level=2]",
        ),
        (
            water_level_one,
            empty_cauldron,
            "burning player touching water_cauldron[level=1] should empty it to cauldron",
        ),
        (
            snow_level_three,
            water_level_two,
            "burning player touching powder_snow_cauldron[level=3] should melt it to water_cauldron[level=2]",
        ),
        (
            snow_level_one,
            empty_cauldron,
            "burning player touching powder_snow_cauldron[level=1] should melt it to cauldron",
        ),
    ] {
        set_block(&mut app, dimension, cauldron_position, initial_state);
        state_mut(&mut app, player).fire = 80;
        let _ = drain_block_sets(&mut app);
        tick(&mut app, 1);
        assert_eq!(
            state_of(&app, player).fire,
            -20,
            "extinguishing cauldron should reset player fire ticks to -20"
        );
        assert!(
            drain_block_sets(&mut app).contains(&(cauldron_position, expected_next)),
            "{description}"
        );
    }

    set_block(&mut app, dimension, cauldron_position, water_level_three);
    state_mut(&mut app, player).fire = -20;
    let _ = drain_block_sets(&mut app);
    tick(&mut app, 1);
    assert!(
        drain_block_sets(&mut app).is_empty(),
        "unburning player touching water_cauldron should not modify cauldron level"
    );
}

#[test]
fn rain_extinguishes_fire_unless_roofed() {
    let (mut app, dimension, player) = start_hazard_app();
    let blocks = app.world().resource::<Blocks>().clone();
    let stone = default_block(&app, "minecraft:stone");
    let top_slab = blocks
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
        .expect("dry top stone_slab state should exist");
    let water = default_block(&app, "minecraft:water");
    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 64, 0),
        BlockStateId::AIR,
    );
    let plains =
        ResourceLocationBuf::read("minecraft:plains").expect("plains identifier should parse");
    let plains_biome_number = app
        .world()
        .resource::<CurrentRegistries>()
        .registry::<BiomeFile>()
        .expect("BiomeFile registry should load")
        .by_name(&plains)
        .expect("plains biome should exist")
        .number();
    let plains_id =
        u8::try_from(plains_biome_number).expect("biome palette id should fit inside u8");
    let section = app
        .world()
        .get::<SectionIndex>(dimension)
        .expect("dimension should carry SectionIndex")
        .get(SectionPos::new(0, 4, 0))
        .expect("section (0, 4, 0) should be loaded");
    let palette = BiomePalette::homogeneous(plains_id);
    app.world_mut().entity_mut(section).insert(palette);
    app.world_mut()
        .entity_mut(dimension)
        .insert((HasWeather, HasSkyLight));
    app.insert_resource(WeatherLevel {
        rain: 1.0,
        thunder: 0.0,
    });

    state_mut(&mut app, player).fire = 100;
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).fire,
        -20,
        "open-sky rain in plains biome should extinguish fire"
    );

    set_block(&mut app, dimension, BlockPos::new(0, 65, 0), top_slab);
    state_mut(&mut app, player).fire = 100;
    tick(&mut app, 1);
    assert!(
        state_of(&app, player).fire > 0,
        "top slab at player head block height should shield player from rain"
    );

    set_block(
        &mut app,
        dimension,
        BlockPos::new(0, 65, 0),
        BlockStateId::AIR,
    );
    set_block(&mut app, dimension, BlockPos::new(0, 67, 0), water);
    state_mut(&mut app, player).fire = 100;
    tick(&mut app, 1);
    assert!(
        state_of(&app, player).fire > 0,
        "overhead fluid block should block rain from extinguishing player"
    );

    set_block(&mut app, dimension, BlockPos::new(0, 67, 0), stone);
    state_mut(&mut app, player).fire = 100;
    tick(&mut app, 1);
    assert!(
        state_of(&app, player).fire > 0,
        "overhead solid stone roof should shield player from rain"
    );
}

#[test]
fn freezing_powder_snow_boots_and_spectator_thaw() {
    let (mut app, dimension, player) = start_hazard_app();
    let powder_snow = default_block(&app, "minecraft:powder_snow");

    fill_column(&mut app, dimension, 0, 0, powder_snow);
    tick(&mut app, 160);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        140,
        "powder_snow accumulation should cap ticks_frozen at 140"
    );
    let speed = app
        .world()
        .get::<AttributeInstances>(player)
        .expect("player should carry AttributeInstances")
        .value(Attribute::MovementSpeed)
        .expect("MovementSpeed attribute should compute");
    let expected_speed = f64::from(0.1_f32) + f64::from(-0.05_f32);
    assert!(
        (speed - expected_speed).abs() < 1.0e-7,
        "MovementSpeed should include full minecraft:powder_snow slow modifier at 140 ticks"
    );
    assert!(
        (health_of(&app, player) - 19.0).abs() < 1.0e-4,
        "reaching 140 ticks_frozen on 40-tick interval should deal 1.0 freeze damage"
    );

    let boots_id = {
        let registries = app.world().resource::<CurrentRegistries>();
        let boots_location = ResourceLocationBuf::read("minecraft:leather_boots")
            .expect("boots location should parse");
        registries
            .entries::<Item>()
            .expect("Item registry should load")
            .by_name(&boots_location)
            .expect("leather_boots item should exist")
    };
    let mut equipment = OwnEquipment::default();
    equipment.0.set(
        EquipSlot::Feet,
        Some(ProtoStack {
            count: 1,
            id: boots_id.into(),
            ..Default::default()
        }),
    );
    app.world_mut().entity_mut(player).insert(equipment);
    tick(&mut app, 20);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        100,
        "wearing leather_boots inside powder_snow should thaw 2 ticks per tick"
    );

    app.world_mut()
        .entity_mut(player)
        .insert(OwnEquipment::default());
    set_game_mode(&mut app, player, GameMode::Spectator);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        98,
        "Spectator mode should thaw ticks_frozen by 2 per tick"
    );
}

#[test]
fn freezing_effect_and_fire_clear_extinguish_sound() {
    let (mut app, dimension, player) = start_hazard_app();
    let powder_snow = default_block(&app, "minecraft:powder_snow");
    let fire = default_block(&app, "minecraft:fire");
    let baked = *app.world().resource::<BakedEnvDamage>();
    let extinguish_id = WireId::from(
        baked
            .extinguish_fire
            .expect("extinguish_fire sound should bake"),
    );

    state_mut(&mut app, player).ticks_frozen = 140;
    state_mut(&mut app, player)
        .active_effects
        .push(active_effect("minecraft:freezing", 200));
    app.world_mut()
        .get_mut::<EnvTickState>(player)
        .expect("player should carry EnvTickState")
        .freeze_age_ticks = 40;
    let health_before_effect = health_of(&app, player);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        140,
        "MobEffect::Freezing should preserve ticks_frozen outside powder_snow"
    );
    assert!(
        (health_before_effect - health_of(&app, player) - 1.5).abs() < 1.0e-4,
        "MobEffect::Freezing at maximum frozen ticks should deal 1.5 damage"
    );

    fill_column(&mut app, dimension, 0, 0, powder_snow);
    state_mut(&mut app, player).ticks_frozen = 138;
    *app.world_mut()
        .get_mut::<Vitals>(player)
        .expect("player should carry Vitals") = Vitals::new(20.0);
    app.world_mut()
        .get_mut::<EnvTickState>(player)
        .expect("player should carry EnvTickState")
        .freeze_age_ticks = 39;
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        139,
        "MobEffect::Freezing inside powder_snow should still accumulate +1 ticks_frozen per tick"
    );
    app.world_mut()
        .get_mut::<EnvTickState>(player)
        .expect("player should carry EnvTickState")
        .freeze_age_ticks = 40;
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        140,
        "MobEffect::Freezing inside powder_snow should reach 140 ticks_frozen"
    );
    assert!(
        (20.0 - health_of(&app, player) - 1.5).abs() < 1.0e-4,
        "MobEffect::Freezing inside powder_snow at 140 ticks should deal 1.5 effect damage"
    );

    fill_column(&mut app, dimension, 0, 0, BlockStateId::AIR);
    state_mut(&mut app, player).active_effects.clear();
    set_block(&mut app, dimension, BlockPos::new(0, 64, 0), fire);
    let _ = drain_sound_ids(&mut app);
    tick(&mut app, 1);
    assert_eq!(
        state_of(&app, player).ticks_frozen,
        0,
        "standing in fire should clear ticks_frozen immediately to 0"
    );
    assert!(
        drain_sound_ids(&mut app).contains(&extinguish_id),
        "fire contact clearing freeze ticks should emit extinguish sound"
    );
}

fn place_named(app: &mut App, dimension: Entity, block_position: BlockPos, name: &str) {
    let state = default_block(app, name);
    set_block(app, dimension, block_position, state);
}

fn run_order_tick(app: &mut App, player: Entity, air: i16, fire: i16, frozen: i32) -> Vec<u32> {
    {
        let mut entity = app.world_mut().entity_mut(player);
        *entity
            .get_mut::<HitRecord>()
            .expect("player should carry HitRecord") = HitRecord::default();
        entity
            .get_mut::<Vitals>()
            .expect("player should carry Vitals")
            .health = 20.0;
        entity
            .get_mut::<ClientOnGround>()
            .expect("player should carry ClientOnGround")
            .0 = true;
        let mut state = entity
            .get_mut::<PlayerFileState>()
            .expect("player should carry PlayerFileState");
        state.air = air;
        state.fire = fire;
        state.ticks_frozen = frozen;
        state.fall_distance = 6.0;
    }
    app.world_mut()
        .get_mut::<EnvTickState>(player)
        .expect("player should carry EnvTickState")
        .freeze_age_ticks = 40;
    app.world_mut().write_message(PlayerMoved {
        entity: player,
        delta: DVec3::new(0.0, -0.5, 0.0),
        was_on_ground: false,
        now_on_ground: true,
    });
    app.world_mut()
        .resource_mut::<RecordedPendingOrder>()
        .0
        .clear();
    tick(app, 1);
    app.world().resource::<RecordedPendingOrder>().0.clone()
}

#[test]
fn env_damage_full_hazard_order_matches_env_ord_001() {
    let (mut app, dimension, player) = start_hazard_app();
    let baked = *app.world().resource::<BakedEnvDamage>();
    for (block_x, block_y, name) in [
        (0, 63, "minecraft:magma_block"),
        (0, 64, "minecraft:fire"),
        (1, 64, "minecraft:cactus"),
        (1, 65, "minecraft:stone"),
    ] {
        place_named(
            &mut app,
            dimension,
            BlockPos::new(block_x, block_y, 0),
            name,
        );
    }
    app.world_mut()
        .get_mut::<ShiftKey>(player)
        .expect("player should carry ShiftKey")
        .0 = false;
    app.world_mut()
        .get_mut::<Transform>(player)
        .expect("player should carry Transform")
        .translation = DVec3::new(0.85, 64.0, 0.5);

    let first_order = run_order_tick(&mut app, player, 300, 20, 0);
    assert_eq!(
        first_order,
        vec![
            baked.on_fire.expect("on_fire damage type should bake"),
            baked.in_wall.expect("in_wall damage type should bake"),
            baked.hot_floor.expect("hot_floor damage type should bake"),
            baked.in_fire.expect("in_fire damage type should bake"),
            baked.cactus.expect("cactus damage type should bake"),
            baked.fall.expect("fall damage type should bake"),
        ],
        "first hazard pass should follow on_fire -> in_wall -> hot_floor -> in_fire -> cactus -> fall"
    );

    for (block_y, name) in [(64, "minecraft:powder_snow"), (65, "minecraft:water")] {
        place_named(&mut app, dimension, BlockPos::new(0, block_y, 0), name);
    }
    let second_order = run_order_tick(&mut app, player, -19, -20, 140);
    assert_eq!(
        second_order,
        vec![
            baked.in_wall.expect("in_wall damage type should bake"),
            baked.drown.expect("drown damage type should bake"),
            baked.hot_floor.expect("hot_floor damage type should bake"),
            baked.cactus.expect("cactus damage type should bake"),
            baked.freeze.expect("freeze damage type should bake"),
        ],
        "second hazard pass should follow in_wall -> drown -> hot_floor -> cactus -> freeze"
    );
}
