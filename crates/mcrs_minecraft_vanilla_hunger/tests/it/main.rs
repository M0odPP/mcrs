use bevy_app::{App, FixedUpdate, TaskPoolPlugin};
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::entity::Entity;
use bevy_ecs::schedule::{LogLevel, ScheduleBuildSettings, Schedules};
use bevy_math::DVec3;
use mcrs_minecraft_damage::{DamageSource, Hit, HitRecord, Vitals};
use mcrs_minecraft_engine::PlayerMoved;
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine::player_sprinting::PlayerSprinting;
use mcrs_minecraft_engine::player_state::{Effect, PlayerFileState};
use mcrs_minecraft_engine_damage::{DamagePlugin, DamageRequest, DamageTypeIds, PendingHits};
use mcrs_minecraft_engine_player_vitals::PlayerVitalsPlugin;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::block_drops::BlockBroken;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::world::dimension::{
    DimensionPlugin, DimensionTypeConfig, InDimension,
};
use mcrs_minecraft_engine_world::world::lifecycle::stage::SectionStage;
use mcrs_minecraft_engine_world::world::storage::column::{
    ColumnIndex, ColumnPos, ColumnSections, ColumnSlot,
};
use mcrs_minecraft_entity::damage_type_keys;
use mcrs_minecraft_entity::registry_keys::DAMAGE_TYPE;
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

fn assert_close(got: f32, exp: f32, msg: &str) {
    assert!((got - exp).abs() < 1.0e-5, "{msg}: got {got}, exp {exp}");
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
    let world = app.world_mut();
    let labels: Vec<_> = world
        .resource::<Schedules>()
        .iter()
        .map(|(_, schedule)| schedule.label())
        .collect();
    for label in labels {
        world
            .try_schedule_scope(label, |world, schedule| {
                let prev = schedule.get_build_settings();
                schedule.set_build_settings(ScheduleBuildSettings {
                    ambiguity_detection: LogLevel::Error,
                    ..prev.clone()
                });
                let ambiguity = schedule
                    .initialize(world)
                    .err()
                    .map(|err| err.to_string(schedule.graph(), world));
                schedule.set_build_settings(prev);
                schedule.initialize(world).ok();
                assert!(ambiguity.is_none(), "{label:?}: {ambiguity:?}");
            })
            .expect("schedule label exists");
    }
}

#[test]
fn exhaustion_sources_respect_rules_and_idle_tracks_stay_clean() {
    let mut app = test_app();
    tick(&mut app);

    let survival = spawn_player(&mut app, 1, GameMode::Survival);
    let victim = spawn_player(&mut app, 2, GameMode::Survival);
    let mut survival_mut = app.world_mut().entity_mut(survival);
    survival_mut.insert(PlayerSprinting(true));
    survival_mut
        .get_mut::<PlayerFileState>()
        .unwrap()
        .invulnerable = true;
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
    let in_fire = damage_type_keys::IN_FIRE.location();
    let fire_id = u32::from(
        test_registries()
            .id_of(DAMAGE_TYPE.location(), in_fire)
            .unwrap(),
    );
    push_hit(&mut app, victim, DamageSource::of(fire_id), 2, 1);
    tick(&mut app);

    assert_close(exh(&app, survival), 0.305, "survival attacker exh");
    assert_close(exh(&app, victim), 0.1, "fire-damaged victim exh");

    app.world_mut().clear_trackers();
    tick(&mut app);
    let s_ref = app.world().entity(survival);
    let fc = s_ref.get_ref::<PlayerFileState>().unwrap().is_changed();
    let vc = s_ref.get_ref::<Vitals>().unwrap().is_changed();
    assert!(!fc, "idle file state");
    assert!(!vc, "idle vitals");
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
    let slot = ColumnSlot {
        entity: col,
        section_count: 1,
    };
    w.get_mut::<ColumnIndex>(dim)
        .unwrap()
        .insert(ColumnPos::new(0, 0), slot);

    let spec = spawn_player(&mut app, 3, GameMode::Spectator);
    let pos = Transform::default().with_translation(DVec3::new(32.5, 4.0, 0.5));
    let w = app.world_mut();
    w.entity_mut(spec).insert((InDimension(dim), pos));
    w.get_mut::<PlayerFileState>(spec)
        .unwrap()
        .food_exhaustion_level = 6.0;
    tick(&mut app);
    assert_eq!(exh(&app, spec), 6.0, "unloaded spectator skips drain");

    app.world_mut()
        .get_mut::<Transform>(spec)
        .unwrap()
        .translation = DVec3::new(8.5, 4.0, 8.5);
    tick(&mut app);
    assert_eq!(exh(&app, spec), 2.0, "loaded spectator drains threshold");

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
    let mut state = app.world_mut().get_mut::<PlayerFileState>(hungry).unwrap();
    state.active_effects.push(effect);
    tick(&mut app);
    assert_close(exh(&app, hungry), 0.4, "hunger amp 79 exh per tick");
}
