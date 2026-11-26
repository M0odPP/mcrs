use bevy_app::{App, FixedUpdate};
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine_damage::DamagePlugin;
use mcrs_minecraft_engine_player_vitals::PlayerVitalsPlugin;
use mcrs_minecraft_engine_registries::test_corpus;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::world::dimension::DimensionPlugin;
use mcrs_minecraft_engine_world::world_border::{DimensionWorldBorder, WorldBorderBounds};
use mcrs_minecraft_env_damage::FallBlockKind;
use mcrs_minecraft_game_registries::registry_load::test_registries;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_vanilla_env_damage::{
    BakedEnvDamage, EnvBlockTable, EnvContactBlock, VanillaEnvDamagePlugin,
};

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

    let icicle = blocks
        .blocks()
        .iter()
        .find(|entry| entry.identifier.as_str() == "minecraft:icicle")
        .and_then(|entry| {
            (0..entry.state_count)
                .filter_map(|offset| entry.state_at(offset))
                .find(|&id| {
                    entry
                        .value_of(id, "vertical_direction")
                        .is_some_and(|v| v.renders_to("up"))
                        && entry
                            .value_of(id, "thickness")
                            .is_some_and(|v| v.renders_to("tip"))
                })
        })
        .expect("icicle up tip");
    assert_eq!(
        table.get(icicle).expect("icicle entry").fall_kind,
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
