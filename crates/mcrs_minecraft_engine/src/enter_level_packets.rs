use crate::ability::PlayerOpLevel;
use crate::bus::{OutboundPlayerPacket, PacketPayload, to};
use crate::op_level::op_level_packet;
use crate::server_edit::{ticking_state, ticking_step};
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine_world::session::PlayerSession;
use mcrs_minecraft_engine_world::tick_pace::TickPace;
use mcrs_minecraft_protocol_game::GameEventKind;
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundGameEvent, ClientboundPlayerAbilities, ClientboundPlayerPosition,
};
use mcrs_minecraft_weather::{WeatherGameEvent, join_weather_events};

pub(crate) fn enter_level_packets(
    session: PlayerSession,
    op_level: PlayerOpLevel,
    position: ClientboundPlayerPosition,
    abilities: ClientboundPlayerAbilities,
    pace: Option<&TickPace>,
    weather: Option<WeatherLevel>,
    mut push: impl FnMut(OutboundPlayerPacket),
) {
    push(op_level_packet(session, op_level));
    push(to(session, PacketPayload::PlayerPosition(position)));
    push(to(session, PacketPayload::PlayerAbilities(abilities)));
    if let Some(pace) = pace {
        push(to(session, ticking_state(pace)));
        push(to(session, ticking_step(pace.steps())));
    }
    let ge = |game_event| {
        to(
            session,
            PacketPayload::GameEvent(ClientboundGameEvent { game_event }),
        )
    };
    if let Some(weather) = weather {
        for event in join_weather_events(weather.rain, weather.thunder).as_slice() {
            let game_event = match *event {
                WeatherGameEvent::BeginRaining => GameEventKind::BeginRaining,
                WeatherGameEvent::EndRaining => GameEventKind::EndRaining,
                WeatherGameEvent::RainLevelChange(level) => GameEventKind::RainLevelChange(level),
                WeatherGameEvent::ThunderLevelChange(level) => {
                    GameEventKind::ThunderLevelChange(level)
                }
            };
            push(ge(game_event));
        }
    }
    push(ge(GameEventKind::LevelChunksLoadStart));
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcrs_minecraft_engine_world::entity::physics::Transform;
    use mcrs_minecraft_game_mode::GameMode;

    fn ev(w: Option<WeatherLevel>, p: Option<&TickPace>) -> String {
        let pos =
            crate::dimension_change::Arrival::absolute(Transform::default()).position_packet(1);
        let ab =
            crate::game_mode::abilities(GameMode::Survival, crate::ability::PlayerFlying(false));
        let mut got = Vec::new();
        enter_level_packets(PlayerSession(1), PlayerOpLevel(0), pos, ab, p, w, |pkt| {
            if let PacketPayload::GameEvent(ClientboundGameEvent { game_event }) = pkt.data {
                got.push(format!("{game_event:?}"));
            }
        });
        got.join(",")
    }

    #[test]
    fn enter_level_weather_precedes_chunk_load_start_and_skips_dry_or_none() {
        let pace = TickPace::new(20);
        let wl = |rain, thunder| WeatherLevel { rain, thunder };
        let got = ev(Some(wl(0.6, 0.3)), Some(&pace));
        assert_eq!(
            got,
            "BeginRaining,RainLevelChange(0.6),ThunderLevelChange(0.3),LevelChunksLoadStart"
        );
        assert_eq!(ev(None, None), "LevelChunksLoadStart");
        assert_eq!(ev(Some(wl(0.2, 1.0)), None), "LevelChunksLoadStart");
    }
}
