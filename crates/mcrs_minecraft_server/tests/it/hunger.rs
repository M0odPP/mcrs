use bevy_ecs::entity::Entity;
use bevy_math::DVec3;
use bevy_time::TimeUpdateStrategy;
use mcrs_minecraft_command_engine::CommandPlugin;
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_command_world::WorldCommandsPlugin;
use mcrs_minecraft_core::Direction;
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::{
    PlayerLivedTicks,
    bus::PacketPayload,
    network_id::player_network_id,
    player_state::{Effect, PlayerFileState},
    server_edit::{ServerEdit, ServerStateHostPlugin},
};
use mcrs_minecraft_engine_damage::DamageTypeIds;
use mcrs_minecraft_protocol::{Bounded, Decode, Packet, VarInt, uuid::Uuid};
use mcrs_minecraft_protocol_game::{
    MoveFlags,
    clientbound::{ClientboundDamageEvent, ClientboundSetHealth},
    player::PlayerAction,
    serverbound::{
        ClientCommandAction, PlayerCommandAction, ServerboundAcceptTeleportation,
        ServerboundAttack, ServerboundChatCommand, ServerboundClientCommand,
        ServerboundMovePlayerPos, ServerboundPlayerAction, ServerboundPlayerCommand,
        ServerboundPlayerLoaded,
    },
};
use mcrs_minecraft_replication::wire_id;
use mcrs_minecraft_save::Difficulty;
use mcrs_minecraft_server::{
    VanillaBlocksPlugin, VanillaEntityAttributesPlugin, VanillaEntityPhysicsPlugin,
    VanillaHungerPlugin, VanillaPlayerAttackPlugin, VanillaRespawnPlugin,
};
use mcrs_voxel::BlockPos;
use serde_json::Value;

use crate::commands::load_stone_column_at;
use crate::host_app::{self, PlayServer, replicated};
use crate::mob_replication::mob_entities;

const ALICE: Uuid = Uuid::from_u128(0xA11CE);
const TRACE: &str = include_str!("../../../../tools/mcrs_minecraft_probe/traces/hunger.json");

fn step<'a>(trace: &'a Value, id: &str) -> &'a Value {
    let steps = trace["steps"].as_array().unwrap();
    steps.iter().find(|s| s["id"].as_str() == Some(id)).unwrap()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-5
}

fn start_server() -> PlayServer {
    let mut app = host_app::make_host_app();
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.add_plugins((
        ServerStateHostPlugin,
        CommandPlugin::<VanillaCommands>::new(),
        VanillaCommandsPlugin,
        WorldCommandsPlugin,
        VanillaBlocksPlugin,
        VanillaEntityPhysicsPlugin,
        VanillaEntityAttributesPlugin,
        VanillaPlayerAttackPlugin,
        VanillaRespawnPlugin,
        VanillaHungerPlugin,
    ));
    let mut s = PlayServer::start(app, &[(ALICE, 4)], 0);
    load_stone_column_at(&mut s, 0, 0);
    s
}

fn join_survival(s: &mut PlayServer) -> (Entity, Entity, i32, Vec<PacketPayload>) {
    let (a, pkts) = s.join(ALICE, "Alice");
    for p in &pkts {
        if let PacketPayload::PlayerPosition(pos) = p {
            let pkt = ServerboundAcceptTeleportation {
                teleport_id: pos.teleport_id,
                position: pos.position.into(),
                look: pos.look,
            };
            s.send(a, &pkt);
        }
    }
    s.queue(a, &ServerboundPlayerLoaded);
    let _ = s.ticks_all(2);
    cmd(s, a, "gamemode survival @s");
    let net_id = player_network_id(s.session_of(a).unwrap());
    (a, s.player(a), net_id, pkts)
}

fn cmd(s: &mut PlayServer, a: Entity, c: &str) {
    s.queue(
        a,
        &ServerboundChatCommand {
            command: Bounded(c),
        },
    );
    let _ = s.ticks_all(2);
}

fn set_diff(s: &mut PlayServer, difficulty: Difficulty) {
    let w = s.app.world_mut();
    w.trigger(ServerEdit::Difficulty { difficulty });
    w.flush();
    let _ = s.ticks_all(1);
}

fn move_step(s: &mut PlayServer, a: Entity, x: f64, y: f64, ground: bool) {
    let flags = MoveFlags::from_bits(u8::from(ground));
    let pos = DVec3::new(x, y, 8.5).into();
    s.queue(
        a,
        &ServerboundMovePlayerPos {
            position: pos,
            flags,
        },
    );
    let _ = s.ticks_all(1);
}

fn sprint(s: &mut PlayServer, a: Entity, entity_id: i32, on: bool) {
    let action = if on {
        PlayerCommandAction::StartSprinting
    } else {
        PlayerCommandAction::StopSprinting
    };
    let pkt = ServerboundPlayerCommand {
        entity_id: VarInt(entity_id),
        action,
        data: VarInt(0),
    };
    s.queue(a, &pkt);
    let _ = s.ticks_all(1);
}

fn health_pkts(pkts: &[PacketPayload]) -> Vec<ClientboundSetHealth> {
    pkts.iter()
        .filter_map(|p| {
            if let PacketPayload::SetHealth(h) = p {
                Some(*h)
            } else {
                None
            }
        })
        .collect()
}

fn dmg_events(pkts: &[PacketPayload]) -> Vec<ClientboundDamageEvent> {
    replicated(pkts)
        .into_iter()
        .filter(|(id, _)| *id == ClientboundDamageEvent::ID)
        .map(|(_, b)| ClientboundDamageEvent::decode(&mut b.as_slice()).unwrap())
        .collect()
}

fn hunger_of(s: &mut PlayServer, p: Entity) -> (i32, i32, f32, f32) {
    let f = s.world().get::<PlayerFileState>(p).unwrap();
    (
        f.food_level,
        f.food_tick_timer,
        f.food_saturation_level,
        f.food_exhaustion_level,
    )
}

fn set_hunger(s: &mut PlayServer, p: Entity, food: i32, sat: f32, exh: f32, timer: i32) {
    let mut f = s.world().get_mut::<PlayerFileState>(p).unwrap();
    (
        f.food_level,
        f.food_saturation_level,
        f.food_exhaustion_level,
        f.food_tick_timer,
    ) = (food, sat, exh, timer);
}

fn check_exh_and_fast(s: &mut PlayServer, a: Entity, p: Entity, net_id: i32, trace: &Value) {
    sprint(s, a, net_id, true);
    for i in 1..=8 {
        move_step(s, a, 8.5 + f64::from(i) * 0.5, 64.0, true);
    }
    sprint(s, a, net_id, false);
    move_step(s, a, 12.5, 65.2, false);
    move_step(s, a, 12.5, 64.0, true);
    assert!((hunger_of(s, p).3 - 0.45).abs() < 1.0e-4);

    let pos = BlockPos::new(13, 63, 8);
    for action in [
        PlayerAction::StartDestroyBlock,
        PlayerAction::AbortDestroyBlock,
    ] {
        s.queue(
            a,
            &ServerboundPlayerAction {
                action,
                pos,
                direction: Direction::Up,
                sequence: VarInt(1),
            },
        );
    }
    let _ = s.ticks_all(2);
    assert!((hunger_of(s, p).3 - 0.45).abs() < 1.0e-4);

    cmd(s, a, "summon minecraft:pig 14.0 64.0 8.5 {NoAI:1b}");
    let pig = mob_entities(s).pop().unwrap().id;
    let _ = s.ticks_all(10);
    s.queue(
        a,
        &ServerboundAttack {
            entity_id: VarInt(wire_id(pig).0),
        },
    );
    let _ = s.ticks_all(2);
    assert!((hunger_of(s, p).3 - 0.55).abs() < 1.0e-4);

    cmd(s, a, "damage @s 4 minecraft:generic");
    let fast = health_pkts(&s.ticks(a, 40));
    let exp: Vec<f32> = step(trace, "seq_fast_regen")["entity_data"]
        .as_array()
        .unwrap()[1..]
        .iter()
        .map(|e| e["entries"][0]["value"].as_f64().unwrap() as f32)
        .collect();
    assert!(fast.len() == 4 && fast.iter().zip(&exp[..4]).all(|(x, y)| near(x.health, *y)));
    let (_, _, sat, exh) = hunger_of(s, p);
    assert!(sat == 2.0 && near(exh, 2.5500002));
    let rest = health_pkts(&s.ticks(a, 70));
    assert!(
        rest.len() == 6
            && rest[..5]
                .iter()
                .zip(&exp[4..])
                .all(|(x, y)| near(x.health, *y))
    );
    assert!(rest[5].saturation == 0.0 && near(rest[5].health, 19.333332));
}

fn check_slow_peaceful_starve(s: &mut PlayServer, a: Entity, p: Entity, trace: &Value) {
    let fx = Effect {
        id: "minecraft:hunger".into(),
        amplifier: 79,
        duration: 20,
        ambient: false,
        show_particles: false,
        show_icon: None,
        hidden_effect: None,
    };
    {
        let mut st = s.world().get_mut::<PlayerFileState>(p).unwrap();
        st.active_effects.push(fx);
    }
    let _ = s.ticks_all(20);
    s.world()
        .get_mut::<PlayerFileState>(p)
        .unwrap()
        .active_effects
        .clear();
    assert_eq!(hunger_of(s, p).0, 18);

    cmd(s, a, "damage @s 4 minecraft:generic");
    let slow = health_pkts(&s.ticks(a, 105));
    let exp_slow = &step(trace, "seq_slow_regen")["health"].as_array().unwrap()[3..];
    assert_eq!(slow.len(), exp_slow.len());
    for (x, y) in slow.iter().zip(exp_slow) {
        assert!(near(x.health, y["health"].as_f64().unwrap() as f32));
        assert_eq!(x.food.0, y["food"].as_i64().unwrap() as i32);
    }
    let (food, _, _, exh) = hunger_of(s, p);
    assert!(food == 17 && near(exh, 2.5500011));
    cmd(s, a, "gamerule natural_health_regeneration false");
    cmd(s, a, "damage @s 2 minecraft:generic");
    let _ = s.ticks_all(100);
    assert!(near(s.world().get::<Vitals>(p).unwrap().health, 14.333332));

    cmd(s, a, "gamerule natural_health_regeneration true");
    set_diff(s, Difficulty::Peaceful);
    s.world().get_mut::<PlayerLivedTicks>(p).unwrap().0 = 19;
    set_hunger(s, p, 19, 1.0, 0.0, 8);
    s.world().get_mut::<Vitals>(p).unwrap().health = 15.5;
    let _ = s.ticks_all(1);
    let m = health_pkts(&s.ticks(a, 1));
    assert!(
        m.len() == 1 && near(m[0].health, 16.833334) && m[0].food.0 == 20 && m[0].saturation == 2.0
    );

    set_diff(s, Difficulty::Easy);
    s.world().get_mut::<Vitals>(p).unwrap().health = 20.0;
    set_hunger(s, p, 0, 0.0, 0.0, 0);
    let starve = s.world().resource::<DamageTypeIds>().starve.unwrap() as i32;
    let trace_starve = step(trace, "seq_starve_easy_normal_hard")["damage_events"][0]["damage_type"]
        .as_i64()
        .unwrap() as i32;
    assert_eq!(starve, trace_starve);
    for (diff, ticks, count, expected_hp) in [
        (Difficulty::Easy, 800, 10, 10.0),
        (Difficulty::Normal, 800, 9, 1.0),
        (Difficulty::Hard, 80, 1, 0.0),
    ] {
        set_diff(s, diff);
        let evs = dmg_events(&s.ticks(a, ticks));
        assert_eq!(evs.len(), count);
        assert!(evs.iter().all(|e| e.damage_type.0 == starve
            && e.cause_entity_id.0 == 0
            && e.direct_entity_id.0 == 0
            && e.source_position.is_none()));
        assert_eq!(s.world().get::<Vitals>(p).unwrap().health, expected_hp);
    }
}

#[test]
fn server_hunger_matches_probe_trace_sequences() {
    let trace: Value = serde_json::from_str(TRACE).unwrap();
    let mut s = start_server();
    let (a, p, net_id, join_pkts) = join_survival(&mut s);
    let join = &health_pkts(&join_pkts)[0];
    let exp = &step(&trace, "join_alice")["health"][0];
    assert_eq!(
        (join.health, join.food.0, join.saturation),
        (
            exp["health"].as_f64().unwrap() as f32,
            exp["food"].as_i64().unwrap() as i32,
            exp["saturation"].as_f64().unwrap() as f32
        )
    );
    check_exh_and_fast(&mut s, a, p, net_id, &trace);
    check_slow_peaceful_starve(&mut s, a, p, &trace);
}

#[test]
fn creative_mode_and_respawn_reset_hunger_state() {
    let mut s = start_server();
    let (a, p, net_id, _) = join_survival(&mut s);
    cmd(&mut s, a, "gamemode creative @s");
    set_hunger(&mut s, p, 20, 5.0, 0.5, 0);
    let _ = s.ticks_all(1);
    sprint(&mut s, a, net_id, true);
    move_step(&mut s, a, 11.5, 64.0, true);
    assert_eq!(hunger_of(&mut s, p).3, 0.5);

    set_hunger(&mut s, p, 4, 0.5, 1.75, 62);
    s.world().get_mut::<PlayerLivedTicks>(p).unwrap().0 = 500;
    cmd(&mut s, a, "kill @s");
    s.queue(
        a,
        &ServerboundClientCommand {
            action: ClientCommandAction::PerformRespawn,
        },
    );
    let _ = s.ticks_all(3);
    assert_eq!(hunger_of(&mut s, p), (20, 0, 5.0, 0.0));
    assert!(s.world().get::<PlayerLivedTicks>(p).unwrap().0 <= 3);
}
