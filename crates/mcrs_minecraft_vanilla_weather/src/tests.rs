use bevy_app::{App, FixedUpdate};
use bevy_ecs::schedule::{IntoScheduleConfigs, LogLevel, ScheduleBuildSettings, Schedules};
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::server_edit::Weather;
use mcrs_minecraft_engine_world::tick::TickSet;

use crate::{VanillaWeatherDimensionPlugin, VanillaWeatherHostPlugin};

fn assert_no_schedule_ambiguities(app: &mut App, kind: &str) {
    let world = app.world_mut();
    let labels: Vec<_> = world
        .resource::<Schedules>()
        .iter()
        .map(|(_, schedule)| schedule.label())
        .collect();
    assert!(!labels.is_empty(), "{kind} must register schedules");
    for label in labels {
        let scoped = world.try_schedule_scope(label, |world, schedule| {
            let prev = schedule.get_build_settings();
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Error,
                ..prev.clone()
            });
            let err = schedule
                .initialize(world)
                .err()
                .map(|e| e.to_string(schedule.graph(), world));
            schedule.set_build_settings(prev);
            schedule.initialize(world).ok();
            assert!(err.is_none(), "{kind} {label:?}: {err:?}");
        });
        assert!(scoped.is_ok(), "{kind} missing schedule {label:?}");
    }
}

#[test]
fn weather_schedules_build_without_ambiguities() {
    let mut host = App::new();
    host.add_plugins(VanillaWeatherHostPlugin);
    host.init_resource::<Weather>();
    assert_no_schedule_ambiguities(&mut host, "host");

    let mut dim = App::new();
    dim.configure_sets(
        FixedUpdate,
        (
            TickSet::DomainInput,
            TickSet::ClientInput,
            TickSet::Simulate,
            TickSet::WorldWrite,
            TickSet::Project,
            TickSet::Output,
        )
            .chain(),
    );
    dim.add_plugins(VanillaWeatherDimensionPlugin);
    dim.add_message::<OutboundPlayerPacket>();
    dim.init_resource::<Weather>();
    assert_no_schedule_ambiguities(&mut dim, "dimension");
}

#[test]
fn unbaked_weather_tables_reject_rain_until_baked() {
    let unbaked = crate::weather_baked::WeatherBaked::default();
    assert!(unbaked.is_unbaked(), "default tables must be unbaked");
    assert!(
        !unbaked.can_rain(0),
        "unbaked biome table must not permit rain before baking"
    );
}
