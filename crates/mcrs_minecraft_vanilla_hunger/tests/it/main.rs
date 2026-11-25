use bevy_app::{App, FixedUpdate, TaskPoolPlugin};
use bevy_ecs::{
    change_detection::DetectChanges,
    entity::Entity,
    schedule::{LogLevel, ScheduleBuildSettings},
};
use bevy_math::DVec3;
use mcrs_minecraft_damage::{DamageSource, Hit, HitRecord, Vitals};
use mcrs_minecraft_engine::{
    PlayerMoved,
    bus::OutboundPlayerPacket,
    player_signal::PlayerSignal,
    player_sprinting::PlayerSprinting,
    player_state::{Effect, PlayerFileState},
};
use mcrs_minecraft_engine_damage::{DamagePlugin, DamageRequest, DamageTypeIds, PendingHits};
use mcrs_minecraft_engine_player_vitals::PlayerVitalsPlugin;
use mcrs_minecraft_engine_world::{
    birth_clock::BirthClock,
    block_drops::BlockBroken,
    entity::{physics::Transform, player::Player},
    game_rules::GameRules,
    session::{Owner, PlayerSession},
    world::{
        dimension::{DimensionPlugin, DimensionTypeConfig, InDimension},
        lifecycle::stage::SectionStage,
        storage::column::{ColumnIndex, ColumnPos, ColumnSections, ColumnSlot},
    },
};
use mcrs_minecraft_entity::{damage_type_keys, registry_keys::DAMAGE_TYPE};
use mcrs_minecraft_entity_key::{Cause, EntityUuid, WriterKey};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_registries::registry_load::test_registries;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_vanilla_hunger::VanillaHungerPlugin;
use mcrs_voxel::{BlockPos, BlockStateId};

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        DimensionPlugin,
        DamagePlugin,
        PlayerVitalsPlugin,
        VanillaHungerPlugin,
    ));
    app.add_message::<PlayerMoved>();
    app.add_message::<BlockBroken>();
    app.add_message::<OutboundPlayerPacket>();
    app.add_message::<PlayerSignal>();
    app.init_resource::<BirthClock>();
    app.init_resource::<GameRules>();
    app.insert_resource(CurrentRegistries::new(test_registries().clone()));
    app
}

fn spawn_player(app: &mut App, uuid_num: u128, mode: GameMode) -> Entity {
    app.world_mut()
        .spawn((
            Player,
            Owner(PlayerSession(uuid_num as u64)),
            EntityUuid::from_u128(uuid_num),
            Transform::default(),
            Vitals::new(20.0),
            HitRecord::default(),
            PlayerSprinting::default(),
            PlayerGameMode(mode),
            PlayerFileState::default(),
        ))
        .id()
}

fn tick(app: &mut App) {
    let next = app.world().resource::<BirthClock>().0.saturating_add(1);
    app.world_mut().insert_resource(BirthClock(next));
    app.world_mut().run_schedule(FixedUpdate);
}

fn exh(app: &App, entity: Entity) -> f32 {
    app.world()
        .get::<PlayerFileState>(entity)
        .map_or(f32::NAN, |state| state.food_exhaustion_level)
}

fn push_hit(app: &mut App, target: Entity, src: DamageSource, writer: u128, seq: u32) {
    let key = WriterKey::Entity {
        key: EntityUuid::from_u128(writer),
    };
    let req = DamageRequest::new(target, Hit::new(1.0, src), key, 1, seq);
    app.world_mut().resource_mut::<PendingHits>().push(req);
}

#[test]
fn schedule_ambiguity_passes_at_error_level() {
    let mut app = test_app();
    app.world_mut()
        .schedule_scope(FixedUpdate, |world, schedule| {
            let prev = schedule.get_build_settings();
            let settings = ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Error,
                ..prev
            };
            schedule.set_build_settings(settings);
            schedule
                .initialize(world)
                .expect("zero FixedUpdate schedule ambiguities");
        });
}

#[test]
fn exhaustion_sources_respect_rules_and_idle_tracks_stay_clean() {
    let mut app = test_app();
    tick(&mut app);

    let survival = spawn_player(&mut app, 1, GameMode::Survival);
    let victim = spawn_player(&mut app, 2, GameMode::Survival);
    app.world_mut()
        .entity_mut(survival)
        .insert(PlayerSprinting(true));
    app.world_mut().write_message(PlayerMoved {
        entity: survival,
        delta: DVec3::new(1.0, 0.42, 0.0),
        was_on_ground: true,
        now_on_ground: false,
    });
    app.world_mut().write_message(BlockBroken {
        at: BlockPos::new(0, 0, 0),
        state: BlockStateId::checked(1, 2).unwrap(),
        left: BlockStateId::AIR,
        other: None,
        harvests: true,
        breaker: survival,
    });
    let attack_id = app
        .world()
        .resource::<DamageTypeIds>()
        .player_attack
        .unwrap();
    let mut source = DamageSource::of(attack_id);
    source.cause = Cause {
        causing: Some(EntityUuid::from_u128(1)),
        direct: Some(EntityUuid::from_u128(1)),
    };
    push_hit(&mut app, victim, source, 1, 0);
    let fire_id = u32::from(
        test_registries()
            .id_of(DAMAGE_TYPE.location(), damage_type_keys::IN_FIRE.location())
            .unwrap(),
    );
    push_hit(&mut app, victim, DamageSource::of(fire_id), 2, 1);
    tick(&mut app);

    assert!((exh(&app, survival) - 0.305).abs() < 1.0e-5);
    assert!((exh(&app, victim) - 0.1).abs() < 1.0e-5);

    app.world_mut().clear_trackers();
    tick(&mut app);
    let s_ref = app.world().entity(survival);
    assert!(!s_ref.get_ref::<PlayerFileState>().unwrap().is_changed());
    assert!(!s_ref.get_ref::<Vitals>().unwrap().is_changed());
}

#[test]
fn spectator_unloaded_chunk_skips_and_hunger_effect_ticks() {
    let mut app = test_app();
    tick(&mut app);
    let w = app.world_mut();
    let dim = w
        .spawn((ColumnIndex::default(), DimensionTypeConfig::new(0, 16)))
        .id();
    let s0 = w.spawn(SectionStage::Loaded).id();
    let mut sections = ColumnSections::new(0, 1);
    sections.set_loaded(0, s0);
    let col = w.spawn(sections).id();
    w.get_mut::<ColumnIndex>(dim).unwrap().insert(
        ColumnPos::new(0, 0),
        ColumnSlot {
            entity: col,
            section_count: 1,
        },
    );

    let spec = spawn_player(&mut app, 3, GameMode::Spectator);
    let w = app.world_mut();
    w.entity_mut(spec).insert((
        InDimension(dim),
        Transform::default().with_translation(DVec3::new(32.5, 4.0, 0.5)),
    ));
    w.get_mut::<PlayerFileState>(spec)
        .unwrap()
        .food_exhaustion_level = 6.0;
    tick(&mut app);
    assert_eq!(exh(&app, spec), 6.0);

    app.world_mut()
        .get_mut::<Transform>(spec)
        .unwrap()
        .translation = DVec3::new(8.5, 4.0, 8.5);
    tick(&mut app);
    assert_eq!(exh(&app, spec), 2.0);

    let hungry = spawn_player(&mut app, 4, GameMode::Survival);
    let effect = Effect {
        id: "minecraft:hunger".to_owned(),
        amplifier: 79,
        duration: 5,
        ambient: false,
        show_particles: false,
        show_icon: None,
        hidden_effect: None,
    };
    app.world_mut()
        .get_mut::<PlayerFileState>(hungry)
        .unwrap()
        .active_effects
        .push(effect);
    tick(&mut app);
    assert!((exh(&app, hungry) - 0.4).abs() < 1.0e-5);
}
