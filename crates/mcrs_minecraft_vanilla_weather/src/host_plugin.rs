use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::prelude::{Local, Res, ResMut};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_exists;
use mcrs_minecraft_engine::server_edit::Weather;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::tick_pace::tick_runs_normally;
use mcrs_minecraft_engine_world::world_seed::WorldSeed;
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_weather::{WeatherAdvance, WeatherCycleRng, step_cycle};

#[derive(Clone, Copy, Debug, Default)]
pub struct VanillaWeatherHostPlugin;

impl Plugin for VanillaWeatherHostPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            advance_weather_cycle
                .run_if(tick_runs_normally)
                .run_if(resource_exists::<Weather>)
                .run_if(advance_weather_enabled),
        );
    }
}

fn advance_weather_enabled(rules: Option<Res<GameRules>>) -> bool {
    rules.is_some_and(|rules| rules.is_on(GameRule::AdvanceWeather))
}

fn advance_weather_cycle(
    mut weather: ResMut<Weather>,
    seed: Option<Res<WorldSeed>>,
    mut rng: Local<Option<WeatherCycleRng>>,
) {
    let rng = rng.get_or_insert_with(|| WeatherCycleRng::new(seed.map_or(0, |seed| seed.0)));
    let next = step_cycle(weather.as_state(), WeatherAdvance::Normal, rng);
    weather.set_if_neq(Weather {
        clear_ticks: next.clear_ticks,
        rain_ticks: next.rain_ticks,
        thunder_ticks: next.thunder_ticks,
        raining: next.raining,
        thundering: next.thundering,
    });
}
