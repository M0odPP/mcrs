use std::path::{Path, PathBuf};

use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::prelude::{ResMut, With};
use bevy_ecs::system::RunSystemOnce;
use bevy_time::TimeUpdateStrategy;
use mcrs_minecraft_command_engine::CommandPlugin;
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_command_world::WorldCommandsPlugin;
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine::bus::PacketPayload;
use mcrs_minecraft_engine::coordinator::birth_clock::BirthClockPlugin;
use mcrs_minecraft_engine::coordinator::clock::WorldClockHostPlugin;
use mcrs_minecraft_engine::server_edit::{ServerEdit, ServerStateHostPlugin, Weather};
use mcrs_minecraft_engine::{WorldStatePlugin, WorldStateSources, WorldStateWriter};
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::ChangeDimensionRequest;
use mcrs_minecraft_engine_world::tick_pace::TickPace;
use mcrs_minecraft_engine_world::world::dimension::{Dimension, HasWeather};
use mcrs_minecraft_engine_world::world::sub_app::AddDimensionPlugins;
use mcrs_minecraft_engine_world::world::world_save::WorldSave;
use mcrs_minecraft_engine_world::world_seed::WorldSeed;
use mcrs_minecraft_environment::{AdvanceTime, WorldClockPlugin};
use mcrs_minecraft_protocol::Bounded;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol_game::GameEventKind;
use mcrs_minecraft_protocol_game::serverbound::ServerboundChatCommand;
use mcrs_minecraft_protocol_game::text::{Color, IntoText, Text};
use mcrs_minecraft_resource::{ResourceKeyBuf, ResourceLocationBuf};
use mcrs_minecraft_server::VanillaWeatherPlugin;
use mcrs_minecraft_server::operator_broadcast::OperatorBroadcastPlugin;
use mcrs_minecraft_weather::{
    CLEAR_TICKS_MAX, CLEAR_TICKS_MIN, RAIN_OFF_MAX, RAIN_OFF_MIN, RAIN_ON_MAX, RAIN_ON_MIN,
    THUNDER_ON_MAX, THUNDER_ON_MIN,
};
use serde_json::Value;

use crate::host_app::{self, PlayServer, label_of, sent_to};
use crate::support::{chats, reply};

const OP: Uuid = Uuid::from_u128(1);
const GUEST: Uuid = Uuid::from_u128(2);
const TRAVELER: Uuid = Uuid::from_u128(3);
const SECOND_OP: Uuid = Uuid::from_u128(4);

struct TempDir(PathBuf);

impl TempDir {
    fn create(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start_server_with(
    dims: &[(&str, &str)],
    save_dir: Option<&Path>,
    ops: &[(Uuid, u8)],
    seed: u64,
) -> PlayServer {
    let mut app = host_app::make_host_app();
    if let Some(dir) = save_dir {
        app.world_mut()
            .insert_resource(WorldSave(dir.to_path_buf()));
    }
    app.insert_resource(WorldSeed(seed));
    app.insert_resource(TickPace::new(20));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(AdvanceTime(true));
    app.add_plugins((
        ServerStateHostPlugin,
        WorldClockPlugin,
        WorldClockHostPlugin,
        BirthClockPlugin,
        WorldStatePlugin,
        CommandPlugin::<VanillaCommands>::new(),
        VanillaCommandsPlugin,
        WorldCommandsPlugin,
        VanillaWeatherPlugin,
    ));
    app.add_dimension_plugin(OperatorBroadcastPlugin);
    PlayServer::start_in(app, dims, ops, 0)
}

fn start_server(dims: &[(&str, &str)], save_dir: Option<&Path>) -> PlayServer {
    start_server_with(dims, save_dir, &[(OP, 2)], 0)
}

fn weather_events(packets: &[PacketPayload]) -> Vec<(&'static str, f32)> {
    packets
        .iter()
        .filter_map(|p| match p {
            PacketPayload::GameEvent(ev) => Some(match ev.game_event {
                GameEventKind::BeginRaining => ("begin_raining", 0.0),
                GameEventKind::EndRaining => ("end_raining", 0.0),
                GameEventKind::RainLevelChange(v) => ("rain_level_change", v),
                GameEventKind::ThunderLevelChange(v) => ("thunder_level_change", v),
                GameEventKind::LevelChunksLoadStart => ("level_chunks_load_start", 0.0),
                _ => return None,
            }),
            _ => None,
        })
        .collect()
}

fn trace_events(trace: &Value, step_id: &str, client: &str) -> Vec<(String, f32)> {
    let steps = trace["steps"].as_array().unwrap();
    let evs = steps.iter().find(|s| s["id"] == step_id).unwrap()["game_events"]
        .as_array()
        .unwrap();
    evs.iter()
        .filter(|e| e["client"] == client)
        .map(|e| {
            (
                e["event"].as_str().unwrap().into(),
                e["param"].as_f64().unwrap() as f32,
            )
        })
        .collect()
}

fn eq_trace(packets: &[PacketPayload], trace: &Value, step: &str, client: &str) {
    let got: Vec<(String, f32)> = weather_events(packets)
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
    assert_eq!(got, trace_events(trace, step, client), "{step}/{client}");
}

fn cmd(server: &mut PlayServer, anchor: Entity, line: &str) {
    server.queue(
        anchor,
        &ServerboundChatCommand {
            command: Bounded(line),
        },
    );
}

fn transfer(
    server: &mut PlayServer,
    from: Entity,
    anchor: Entity,
    to: &str,
    tf: Transform,
) -> Vec<PacketPayload> {
    let sess = server.session_of(anchor).unwrap();
    let player = server.player_in(from, anchor).unwrap();
    let key = ResourceKeyBuf::from_location(ResourceLocationBuf::read(to).unwrap());
    server
        .world_in(from)
        .resource_mut::<Messages<ChangeDimensionRequest>>()
        .write(ChangeDimensionRequest {
            player,
            to: key,
            transform: tf,
        });
    sent_to(sess, server.ticks_all(4))
}

#[test]
fn weather_probe_trace_and_save_restart_round_trip() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/mcrs_minecraft_probe/traces/weather.json"
    ))
    .unwrap();
    let trace: Value = serde_json::from_str(&raw).unwrap();
    let save_dir = TempDir::create("mcrs-weather-save");

    let dims = [("minecraft:overworld", "minecraft:overworld")];
    let mut server = start_server(&dims, Some(&save_dir.0));
    server
        .app
        .world_mut()
        .trigger(ServerEdit::ClearWeather { ticks: 600 });
    let (alice, alice_join) = server.join(OP, "Alice");
    let alice_sess = server.session_of(alice).unwrap();
    eq_trace(&alice_join, &trace, "join_alice_clear", "Alice");

    cmd(&mut server, alice, "weather rain 400");
    eq_trace(
        &sent_to(alice_sess, server.ticks_all(105)),
        &trace,
        "weather_rain",
        "Alice",
    );

    let (bob, bob_join) = server.join(GUEST, "Bob");
    let bob_sess = server.session_of(bob).unwrap();
    eq_trace(&bob_join, &trace, "join_bob_during_rain", "Bob");

    cmd(&mut server, alice, "weather thunder 400");
    let thunder_all = server.ticks_all(105);
    eq_trace(
        &sent_to(alice_sess, thunder_all.clone()),
        &trace,
        "weather_thunder",
        "Alice",
    );
    eq_trace(
        &sent_to(bob_sess, thunder_all),
        &trace,
        "weather_thunder",
        "Bob",
    );

    cmd(&mut server, alice, "gamerule advance_weather false");
    cmd(&mut server, alice, "weather clear 200");
    let clear_all = server.ticks_all(105);
    eq_trace(
        &sent_to(alice_sess, clear_all.clone()),
        &trace,
        "weather_clear_and_stop_advance",
        "Alice",
    );
    eq_trace(
        &sent_to(bob_sess, clear_all),
        &trace,
        "weather_clear_and_stop_advance",
        "Bob",
    );
    let frozen = *server.app.world().resource::<Weather>();
    server.ticks_all(10);
    assert_eq!(
        *server.app.world().resource::<Weather>(),
        frozen,
        "frozen weather changed while gamerule advance_weather is false"
    );

    cmd(&mut server, alice, "gamerule advance_weather true");
    cmd(&mut server, alice, "weather thunder 400");
    let save_all = server.ticks_all(105);
    eq_trace(
        &sent_to(alice_sess, save_all.clone()),
        &trace,
        "set_thunder_for_save",
        "Alice",
    );
    eq_trace(
        &sent_to(bob_sess, save_all),
        &trace,
        "set_thunder_for_save",
        "Bob",
    );
    server
        .app
        .world_mut()
        .run_system_once(|src: WorldStateSources, mut w: ResMut<WorldStateWriter>| {
            w.submit(src.snapshot().unwrap())
        })
        .unwrap();
    while !server.app.world().resource::<WorldStateWriter>().is_idle() {
        server.ticks_all(1);
    }
    assert!(
        !server.app.world().resource::<WorldStateWriter>().failed(),
        "world state snapshot write should succeed"
    );
    drop(server);

    let mut restarted = start_server(&dims, Some(&save_dir.0));
    let (alice2, rejoin) = restarted.join(OP, "Alice");
    let alice2_sess = restarted.session_of(alice2).unwrap();
    eq_trace(&rejoin, &trace, "join_alice_after_restart", "Alice");
    assert!(
        weather_events(&sent_to(alice2_sess, restarted.ticks_all(3))).is_empty(),
        "no redundant weather packets immediately after restart join"
    );
}

#[test]
fn weather_commands_permissions_feedback_and_operator_broadcast() {
    let dims = [
        ("minecraft:overworld", "minecraft:overworld"),
        ("minecraft:the_nether", "minecraft:the_nether"),
    ];
    let mut server = start_server_with(&dims, None, &[(OP, 2), (SECOND_OP, 2)], 0);
    server
        .app
        .world_mut()
        .trigger(ServerEdit::ClearWeather { ticks: 600 });
    let nether = label_of(&server.app, "minecraft:the_nether");
    let (op, _) = server.join(OP, "Op");
    let (guest, _) = server.join(GUEST, "Guest");
    let (nether_op, _) = server.join_in(nether, SECOND_OP, "Watcher");
    let (op_sess, guest_sess, nether_sess) = (
        server.session_of(op).unwrap(),
        server.session_of(guest).unwrap(),
        server.session_of(nether_op).unwrap(),
    );

    cmd(&mut server, guest, "weather thunder 100");
    server.ticks_all(1);
    assert!(
        !server.app.world().resource::<Weather>().raining,
        "guest without gamemaster permission must not change weather"
    );
    for (line, key, check) in [
        (
            "weather clear",
            "commands.weather.set.clear",
            (|w: &Weather| {
                !w.raining && (CLEAR_TICKS_MIN..=CLEAR_TICKS_MAX).contains(&w.clear_ticks)
            }) as fn(&Weather) -> bool,
        ),
        (
            "weather rain",
            "commands.weather.set.rain",
            (|w: &Weather| {
                w.raining
                    && !w.thundering
                    && (RAIN_ON_MIN - 2..=RAIN_ON_MAX).contains(&w.rain_ticks)
            }) as fn(&Weather) -> bool,
        ),
        (
            "weather thunder",
            "commands.weather.set.thunder",
            (|w: &Weather| {
                w.raining
                    && w.thundering
                    && (THUNDER_ON_MIN - 2..=THUNDER_ON_MAX).contains(&w.thunder_ticks)
            }) as fn(&Weather) -> bool,
        ),
    ] {
        cmd(&mut server, op, line);
        let pkts = server.ticks_all(2);
        assert!(check(server.app.world().resource::<Weather>()), "{line}");
        assert_eq!(
            chats(&sent_to(op_sess, pkts.clone())),
            [reply(key)],
            "{line} must send translation feedback {key} to source"
        );
        let admin = Text::translate("chat.type.admin", vec!["Op".into_text(), reply(key)])
            .color(Color::GRAY)
            .italic();
        assert_eq!(
            chats(&sent_to(nether_sess, pkts.clone())),
            [admin],
            "{line} must broadcast admin message to second operator"
        );
        assert!(
            chats(&sent_to(guest_sess, pkts)).is_empty(),
            "{line} must not broadcast to non-operator"
        );
    }
}

fn event_names(packets: &[PacketPayload]) -> Vec<&'static str> {
    weather_events(packets)
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

#[test]
fn weather_excludes_nether_and_end_on_join_ramp_and_transfer() {
    let dims = [
        ("minecraft:overworld", "minecraft:overworld"),
        ("minecraft:the_nether", "minecraft:the_nether"),
        ("minecraft:the_end", "minecraft:the_end"),
    ];
    let mut server = start_server(&dims, None);
    server
        .app
        .world_mut()
        .trigger(ServerEdit::ClearWeather { ticks: 600 });
    let (op, _) = server.join(OP, "Op");
    let (overworld, nether, end) = (
        server.dim,
        label_of(&server.app, "minecraft:the_nether"),
        label_of(&server.app, "minecraft:the_end"),
    );
    let (nether_viewer, nether_join) = server.join_in(nether, GUEST, "NetherViewer");
    let (traveler, end_join) = server.join_in(end, TRAVELER, "Traveler");
    let (op_sess, nether_sess, end_sess) = (
        server.session_of(op).unwrap(),
        server.session_of(nether_viewer).unwrap(),
        server.session_of(traveler).unwrap(),
    );
    for (name, dim_ent, join) in [
        ("the_nether", nether, nether_join),
        ("the_end", end, end_join),
    ] {
        assert_eq!(
            event_names(&join),
            ["level_chunks_load_start"],
            "{name} join must omit weather events"
        );
        let world = server.world_in(dim_ent);
        assert!(
            world
                .query_filtered::<(), (With<Dimension>, With<HasWeather>)>()
                .single(world)
                .is_err(),
            "{name} must not have HasWeather"
        );
        assert!(
            world.get_resource::<WeatherLevel>().is_none(),
            "{name} must not initialize WeatherLevel"
        );
    }

    cmd(&mut server, op, "weather rain 500");
    let ramp = server.ticks_all(105);
    assert!(
        !weather_events(&sent_to(op_sess, ramp.clone())).is_empty(),
        "overworld player must receive live rain ramp packets"
    );
    for (name, sess) in [("the_nether", nether_sess), ("the_end", end_sess)] {
        assert!(
            weather_events(&sent_to(sess, ramp.clone())).is_empty(),
            "{name} viewer must receive no live overworld weather ramp packets"
        );
    }

    let p = server.player_in(end, traveler).unwrap();
    let tf = *server.world_in(end).get::<Transform>(p).unwrap();
    let arrived = transfer(&mut server, end, traveler, "minecraft:overworld", tf);
    assert_eq!(
        event_names(&arrived),
        [
            "begin_raining",
            "rain_level_change",
            "thunder_level_change",
            "level_chunks_load_start"
        ],
        "transfer to raining overworld must announce weather before chunk load start"
    );
    for (from, to_name) in [
        (overworld, "minecraft:the_nether"),
        (nether, "minecraft:the_end"),
    ] {
        let pkts = transfer(&mut server, from, traveler, to_name, tf);
        assert_eq!(
            event_names(&pkts),
            ["level_chunks_load_start"],
            "transfer to {to_name} must only emit level_chunks_load_start"
        );
    }
}

#[test]
fn natural_cycle_countdowns_and_seed_determinism() {
    let dims = [("minecraft:overworld", "minecraft:overworld")];
    let seed = 0xCAFE_BABE_1234_5678;
    let mut a = start_server_with(&dims, None, &[(OP, 2)], seed);
    let mut b = start_server_with(&dims, None, &[(OP, 2)], seed);

    for s in [&mut a, &mut b] {
        s.app
            .world_mut()
            .trigger(ServerEdit::ClearWeather { ticks: 2 });
        s.app.world_mut().flush();
    }
    a.ticks_all(2);
    b.ticks_all(2);
    let w_zero = *a.app.world().resource::<Weather>();
    assert_eq!(
        w_zero,
        *b.app.world().resource::<Weather>(),
        "seed match at clear=0"
    );
    assert_eq!(w_zero.clear_ticks, 0, "clear countdown reaches zero");
    assert!(
        !w_zero.raining && !w_zero.thundering,
        "clear stays dry at zero"
    );

    a.ticks_all(1);
    b.ticks_all(1);
    let w_on = *a.app.world().resource::<Weather>();
    assert_eq!(
        w_on,
        *b.app.world().resource::<Weather>(),
        "seed match on rollover"
    );
    assert!(
        w_on.raining && w_on.thundering,
        "clear rollover starts rain and thunder"
    );
    assert!(
        (RAIN_ON_MIN..=RAIN_ON_MAX).contains(&w_on.rain_ticks),
        "rain on bounds"
    );
    assert!(
        (THUNDER_ON_MIN..=THUNDER_ON_MAX).contains(&w_on.thunder_ticks),
        "thunder on bounds"
    );

    for s in [&mut a, &mut b] {
        s.app
            .world_mut()
            .trigger(ServerEdit::RainWeather { ticks: 2 });
        s.app.world_mut().flush();
        s.app.world_mut().resource_mut::<Weather>().thunder_ticks = 1;
    }
    a.ticks_all(1);
    b.ticks_all(1);
    let w_step = *a.app.world().resource::<Weather>();
    assert!(
        !w_step.thundering && w_step.raining,
        "t1 stays clear/rain before zero"
    );
    assert_eq!(
        (w_step.rain_ticks, w_step.thunder_ticks),
        (1, 0),
        "t1 timers"
    );

    a.ticks_all(1);
    b.ticks_all(1);
    let w_thunder = *a.app.world().resource::<Weather>();
    assert!(
        w_thunder.raining && w_thunder.thundering,
        "thunder flips on at zero"
    );
    assert_eq!(
        w_thunder.rain_ticks, 0,
        "rain reaches zero while thunder flips"
    );
    assert!(
        (THUNDER_ON_MIN..=THUNDER_ON_MAX).contains(&w_thunder.thunder_ticks),
        "thunder on"
    );
    assert_eq!(
        w_thunder,
        *b.app.world().resource::<Weather>(),
        "seed match at thunder flip"
    );

    a.ticks_all(1);
    b.ticks_all(1);
    let w_clear = *a.app.world().resource::<Weather>();
    assert!(!w_clear.raining, "rain flips off at zero");
    assert!(
        (RAIN_OFF_MIN..=RAIN_OFF_MAX).contains(&w_clear.rain_ticks),
        "rain off bounds"
    );
    assert_eq!(
        w_clear,
        *b.app.world().resource::<Weather>(),
        "seed match at rain off"
    );
}
