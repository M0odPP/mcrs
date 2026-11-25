use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine_world::tick_pace::TickPace;
use mcrs_minecraft_environment::AdvanceTime;
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::clientbound::{ClientboundTickingState, ClientboundTickingStep};
use mcrs_minecraft_save::Difficulty;
use rustc_hash::FxHashMap;

use mcrs_minecraft_engine_world::game_rules::GameRules;

use crate::bus::{OutboundPlayerPacket, PacketPayload, PacketTarget, to_target};
use crate::muxer::online_count::send_online_count;
use crate::server_difficulty::ServerDifficulty;
use mcrs_minecraft_weather::{WeatherKind, WeatherState, apply_weather_command};

#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub enum ServerEdit {
    GameRule { rule: GameRule, value: i32 },
    Difficulty { difficulty: Difficulty },
    ClearWeather { ticks: u32 },
    RainWeather { ticks: u32 },
    ThunderWeather { ticks: u32 },
    StartSprint { ticks: u64 },
    StopSprint,
    TickRate { rate: f32 },
    Freeze,
    Unfreeze,
    Step { ticks: u64 },
    StopStep,
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weather {
    pub clear_ticks: u32,
    pub rain_ticks: u32,
    pub thunder_ticks: u32,
    pub raining: bool,
    pub thundering: bool,
}

impl Weather {
    #[must_use]
    pub const fn as_state(self) -> WeatherState {
        WeatherState {
            clear_ticks: self.clear_ticks,
            rain_ticks: self.rain_ticks,
            thunder_ticks: self.thunder_ticks,
            raining: self.raining,
            thundering: self.thundering,
        }
    }
}

#[derive(Resource, Clone, Debug, Default)]
pub struct DimensionWeatherLevels(FxHashMap<Entity, WeatherLevel>);

impl DimensionWeatherLevels {
    #[must_use]
    pub fn get(&self, dim: Entity) -> Option<WeatherLevel> {
        self.0.get(&dim).copied()
    }

    pub fn insert(&mut self, dim: Entity, level: WeatherLevel) {
        self.0.insert(dim, level);
    }

    pub fn remove(&mut self, dim: Entity) {
        self.0.remove(&dim);
    }
}

pub fn apply_edit(world: &mut World, edit: ServerEdit) {
    match edit {
        ServerEdit::GameRule { rule, value } => {
            if let Some(mut rules) = world.get_resource_mut::<GameRules>() {
                rules.set(rule, value);
            }
        }
        ServerEdit::Difficulty { difficulty } => {
            if let Some(mut current) = world.get_resource_mut::<ServerDifficulty>() {
                current.set_if_neq(ServerDifficulty(difficulty));
            }
        }
        ServerEdit::ClearWeather { ticks } => set_weather(world, WeatherKind::Clear, ticks),
        ServerEdit::RainWeather { ticks } => set_weather(world, WeatherKind::Rain, ticks),
        ServerEdit::ThunderWeather { ticks } => set_weather(world, WeatherKind::Thunder, ticks),
        ServerEdit::StartSprint { .. }
        | ServerEdit::StopSprint
        | ServerEdit::TickRate { .. }
        | ServerEdit::Freeze
        | ServerEdit::Unfreeze
        | ServerEdit::Step { .. }
        | ServerEdit::StopStep => {
            if let Some(mut pace) = world.get_resource_mut::<TickPace>() {
                edit_pace(&mut pace, edit);
            }
        }
    }
}

fn set_weather(world: &mut World, kind: WeatherKind, ticks: u32) {
    if let Some(mut w) = world.get_resource_mut::<Weather>() {
        let s = apply_weather_command(kind, ticks);
        w.set_if_neq(Weather {
            clear_ticks: s.clear_ticks,
            rain_ticks: s.rain_ticks,
            thunder_ticks: s.thunder_ticks,
            raining: s.raining,
            thundering: s.thundering,
        });
    }
}

fn edit_pace(pace: &mut TickPace, edit: ServerEdit) {
    match edit {
        ServerEdit::StartSprint { ticks } => pace.start_sprint(ticks),
        ServerEdit::StopSprint => pace.stop_sprint(),
        ServerEdit::TickRate { rate } => pace.set_rate(rate),
        ServerEdit::Freeze => pace.freeze(),
        ServerEdit::Unfreeze => pace.unfreeze(),
        ServerEdit::Step { ticks } => pace.step(ticks),
        ServerEdit::StopStep => pace.stop_step(),
        ServerEdit::GameRule { .. }
        | ServerEdit::Difficulty { .. }
        | ServerEdit::ClearWeather { .. }
        | ServerEdit::RainWeather { .. }
        | ServerEdit::ThunderWeather { .. } => {}
    }
}

pub struct ServerStateHostPlugin;

impl Plugin for ServerStateHostPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameRules>();
        app.init_resource::<ServerDifficulty>();
        app.init_resource::<Weather>();
        app.init_resource::<DimensionWeatherLevels>();
        app.add_observer(apply_host_edit);
        app.add_systems(
            Update,
            announce_tick_state
                .run_if(resource_exists::<TickPace>)
                .after(send_online_count),
        );
    }
}

pub fn announce_tick_state(
    mut pace: ResMut<TickPace>,
    mut packets: MessageWriter<OutboundPlayerPacket>,
) {
    if pace.bypass_change_detection().take_state_change() {
        packets.write(to_target(PacketTarget::AllPlayers, ticking_state(&pace)));
    }
    if let Some(steps) = pace.bypass_change_detection().take_steps_change() {
        packets.write(to_target(PacketTarget::AllPlayers, ticking_step(steps)));
    }
}

pub(crate) fn ticking_state(pace: &TickPace) -> PacketPayload {
    PacketPayload::TickingState(ClientboundTickingState {
        tick_rate: pace.rate(),
        frozen: pace.is_frozen(),
    })
}

pub(crate) fn ticking_step(steps: u64) -> PacketPayload {
    PacketPayload::TickingStep(ClientboundTickingStep {
        steps: VarInt(i32::try_from(steps).unwrap_or(i32::MAX)),
    })
}

fn apply_host_edit(edit: On<ServerEdit>, mut commands: Commands) {
    let edit = *edit.event();
    commands.queue(move |world: &mut World| {
        apply_edit(world, edit);
        if let ServerEdit::GameRule {
            rule: GameRule::AdvanceTime,
            value,
        } = edit
            && let Some(mut advance) = world.get_resource_mut::<AdvanceTime>()
        {
            advance.set_if_neq(AdvanceTime(value != 0));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_plugin_does_not_advance_weather_and_applies_weather_edits() {
        let mut app = App::new();
        app.add_message::<OutboundPlayerPacket>()
            .insert_resource(TickPace::new(20))
            .add_plugins(ServerStateHostPlugin);
        let d = Weather::default();
        let init = Weather {
            clear_ticks: 5,
            raining: true,
            thundering: true,
            ..d
        };
        app.insert_resource(init).update();
        assert_eq!(*app.world().resource::<Weather>(), init);
        let mut chk = |edit, kind, t| {
            app.world_mut().trigger(edit);
            app.update();
            let got = app.world().resource::<Weather>().as_state();
            assert_eq!(got, apply_weather_command(kind, t));
        };
        chk(
            ServerEdit::RainWeather { ticks: 1200 },
            WeatherKind::Rain,
            1200,
        );
        chk(
            ServerEdit::ThunderWeather { ticks: 600 },
            WeatherKind::Thunder,
            600,
        );
        chk(
            ServerEdit::ClearWeather { ticks: 2400 },
            WeatherKind::Clear,
            2400,
        );
    }
}
