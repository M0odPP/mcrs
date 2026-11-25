use bevy_app::{App, FixedUpdate, Plugin, Startup};
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::{Commands, Query, Res, ResMut, With, not};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists;
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine::PoseSystems;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload, PacketTarget, to_target};
use mcrs_minecraft_engine::server_edit::Weather;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_engine_world::tick_pace::tick_runs_normally;
use mcrs_minecraft_engine_world::world::dimension::{Dimension, HasWeather};
use mcrs_minecraft_protocol_game::GameEventKind;
use mcrs_minecraft_protocol_game::clientbound::ClientboundGameEvent;
use mcrs_minecraft_weather::{WeatherGameEvent, initial_levels, step_weather_levels};
use smallvec::SmallVec;

#[derive(Clone, Copy, Debug, Default)]
pub struct VanillaWeatherDimensionPlugin;

impl Plugin for VanillaWeatherDimensionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            init_weather_level
                .run_if(not(resource_exists::<WeatherLevel>))
                .run_if(is_weather_dimension),
        );
        app.add_systems(
            FixedUpdate,
            (
                init_weather_level
                    .in_set(TickSet::DomainInput)
                    .run_if(not(resource_exists::<WeatherLevel>))
                    .run_if(is_weather_dimension),
                step_weather_dimension
                    .in_set(TickSet::Project)
                    .before(PoseSystems::Publish)
                    .run_if(tick_runs_normally)
                    .run_if(resource_exists::<WeatherLevel>)
                    .run_if(resource_exists::<Weather>),
            ),
        );
    }
}

fn is_weather_dimension(dim: Query<(), (With<Dimension>, With<HasWeather>)>) -> bool {
    !dim.is_empty()
}

fn init_weather_level(mut commands: Commands, weather: Option<Res<Weather>>) {
    let (rain, thunder) = weather
        .as_deref()
        .map_or((0.0, 0.0), |w| initial_levels(&w.as_state()));
    commands.insert_resource(WeatherLevel { rain, thunder });
}

fn step_weather_dimension(
    dims: Query<(), (With<Dimension>, With<HasWeather>)>,
    players: Query<&Owner, With<Player>>,
    weather: Res<Weather>,
    mut level: ResMut<WeatherLevel>,
    mut packets: MessageWriter<OutboundPlayerPacket>,
) {
    if dims.is_empty() {
        return;
    }
    let step = step_weather_levels(level.rain, level.thunder, &weather.as_state());
    level.set_if_neq(WeatherLevel {
        rain: step.rain,
        thunder: step.thunder,
    });
    if step.events.is_empty() {
        return;
    }
    let recipients: SmallVec<[PlayerSession; 8]> = players.iter().map(|o| o.0).collect();
    if recipients.is_empty() {
        return;
    }
    for event in step.events.as_slice() {
        let game_event = match *event {
            WeatherGameEvent::BeginRaining => GameEventKind::BeginRaining,
            WeatherGameEvent::EndRaining => GameEventKind::EndRaining,
            WeatherGameEvent::RainLevelChange(v) => GameEventKind::RainLevelChange(v),
            WeatherGameEvent::ThunderLevelChange(v) => GameEventKind::ThunderLevelChange(v),
        };
        packets.write(to_target(
            PacketTarget::PlayerSet(recipients.clone()),
            PacketPayload::GameEvent(ClientboundGameEvent { game_event }),
        ));
    }
}
