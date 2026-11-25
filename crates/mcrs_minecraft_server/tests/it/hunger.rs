use bevy_ecs::entity::Entity;
use bevy_math::DVec3;
use bevy_time::TimeUpdateStrategy;
use mcrs_minecraft_command_engine::CommandPlugin;
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_command_world::WorldCommandsPlugin;
use mcrs_minecraft_core::Direction;
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::PlayerLivedTicks;
use mcrs_minecraft_engine::bus::PacketPayload;
use mcrs_minecraft_engine::network_id::player_network_id;
use mcrs_minecraft_engine::player_state::{Effect, PlayerFileState};
use mcrs_minecraft_engine::server_edit::{ServerEdit, ServerStateHostPlugin};
use mcrs_minecraft_engine_damage::DamageTypeIds;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol::{Bounded, Decode, Packet, VarInt};
use mcrs_minecraft_protocol_game::MoveFlags;
use mcrs_minecraft_protocol_game::clientbound::{ClientboundDamageEvent, ClientboundSetHealth};
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{
    ClientCommandAction, PlayerCommandAction, ServerboundAcceptTeleportation, ServerboundAttack,
    ServerboundChatCommand, ServerboundClientCommand, ServerboundMovePlayerPos,
    ServerboundPlayerAction, ServerboundPlayerCommand, ServerboundPlayerLoaded,
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

fn assert_close(got: f32, exp: f32, tol: f32, what: &str) {
    assert!((got - exp).abs() < tol, "{what}: got {got}, exp {exp}");
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
            let (teleport_id, position, look) = (pos.teleport_id, pos.position.into(), pos.look);
            s.send(
                a,
                &ServerboundAcceptTeleportation {
                    teleport_id,
                    position,
                    look,
                },
            );
        }
    }
    s.queue(a, &ServerboundPlayerLoaded);
    let _ = s.ticks_all(2);
    cmd(s, a, "gamemode survival @s");
    let net_id = player_network_id(s.session_of(a).unwrap());
    (a, s.player(a), net_id, pkts)
}

fn cmd(s: &mut PlayServer, a: Entity, c: &str) {
    let command = Bounded(c);
    s.queue(a, &ServerboundChatCommand { command });
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
    let position = DVec3::new(x, y, 8.5).into();
    s.queue(a, &ServerboundMovePlayerPos { position, flags });
    let _ = s.ticks_all(1);
}

fn sprint(s: &mut PlayServer, a: Entity, entity_id: i32, on: bool) {
    let action = if on {
        PlayerCommandAction::StartSprinting
    } else {
        PlayerCommandAction::StopSprinting
    };
    let (entity_id, data) = (VarInt(entity_id), VarInt(0));
    s.queue(
        a,
        &ServerboundPlayerCommand {
            entity_id,
            action,
            data,
        },
    );
    let _ = s.ticks_all(1);
}

fn health_pkts(pkts: &[PacketPayload]) -> Vec<ClientboundSetHealth> {
    pkts.iter()
        .filter_map(|p| match p {
            PacketPayload::SetHealth(h) => Some(*h),
            _ => None,
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

fn hp(s: &mut PlayServer, p: Entity) -> f32 {
    s.world().get::<Vitals>(p).unwrap().health
}

fn check_exh_and_fast(s: &mut PlayServer, a: Entity, p: Entity, net_id: i32, trace: &Value) {
    sprint(s, a, net_id, true);
    for i in 1..=8 {
        move_step(s, a, 8.5 + f64::from(i) * 0.5, 64.0, true);
    }
    sprint(s, a, net_id, false);
    move_step(s, a, 12.5, 65.2, false);
    move_step(s, a, 12.5, 64.0, true);
    assert_close(hunger_of(s, p).3, 0.45, 1.0e-4, "sprint+jump exh");

    let (pos, direction, sequence) = (BlockPos::new(13, 63, 8), Direction::Up, VarInt(1));
    for action in [
        PlayerAction::StartDestroyBlock,
        PlayerAction::AbortDestroyBlock,
    ] {
        let pkt = ServerboundPlayerAction {
            action,
            pos,
            direction,
            sequence,
        };
        s.queue(a, &pkt);
    }
    let _ = s.ticks_all(2);
    assert_close(hunger_of(s, p).3, 0.45, 1.0e-4, "aborted mine exh");

    cmd(s, a, "summon minecraft:pig 14.0 64.0 8.5 {NoAI:1b}");
    let pig = mob_entities(s).pop().unwrap().id;
    let _ = s.ticks_all(10);
    let entity_id = VarInt(wire_id(pig).0);
    s.queue(a, &ServerboundAttack { entity_id });
    let _ = s.ticks_all(2);
    assert_close(hunger_of(s, p).3, 0.55, 1.0e-4, "pig attack exh");

    cmd(s, a, "damage @s 4 minecraft:generic");
    let fast = health_pkts(&s.ticks(a, 40));
    let exp: Vec<f32> = step(trace, "seq_fast_regen")["entity_data"]
        .as_array()
        .unwrap()[1..]
        .iter()
        .map(|e| e["entries"][0]["value"].as_f64().unwrap() as f32)
        .collect();
    assert_eq!(fast.len(), 4, "fast regen 40t count");
    for (x, y) in fast.iter().zip(&exp[..4]) {
        assert_close(x.health, *y, 1.0e-5, "fast regen 40t hp");
    }
    let (_, _, sat, exh) = hunger_of(s, p);
    assert_eq!(sat, 2.0, "fast regen saturation");
    assert_close(exh, 2.5500002, 1.0e-5, "fast regen exh");
    let rest = health_pkts(&s.ticks(a, 70));
    assert_eq!(rest.len(), 6, "fast regen tail count");
    for (x, y) in rest[..5].iter().zip(&exp[4..]) {
        assert_close(x.health, *y, 1.0e-5, "fast regen tail hp");
    }
    assert_eq!(rest[5].saturation, 0.0, "sat zero drain");
    assert_close(rest[5].health, 19.333332, 1.0e-5, "sat zero drain hp");
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
    let mut st = s.world().get_mut::<PlayerFileState>(p).unwrap();
    st.active_effects.push(fx);
    let _ = s.ticks_all(20);
    let mut st = s.world().get_mut::<PlayerFileState>(p).unwrap();
    st.active_effects.clear();
    assert_eq!(hunger_of(s, p).0, 18, "hunger fx drains food");

    cmd(s, a, "damage @s 4 minecraft:generic");
    let slow = health_pkts(&s.ticks(a, 105));
    let exp_slow = &step(trace, "seq_slow_regen")["health"].as_array().unwrap()[3..];
    assert_eq!(slow.len(), exp_slow.len(), "slow regen count");
    for (x, y) in slow.iter().zip(exp_slow) {
        let hp_exp = y["health"].as_f64().unwrap() as f32;
        assert_close(x.health, hp_exp, 1.0e-5, "slow regen hp");
        assert_eq!(x.food.0, y["food"].as_i64().unwrap() as i32, "slow food");
    }
    let (food, _, _, exh) = hunger_of(s, p);
    assert_eq!(food, 17, "slow regen food");
    assert_close(exh, 2.5500011, 1.0e-5, "slow regen exh");
    cmd(s, a, "gamerule natural_health_regeneration false");
    cmd(s, a, "damage @s 2 minecraft:generic");
    let _ = s.ticks_all(100);
    assert_close(hp(s, p), 14.333332, 1.0e-5, "regen off hp");

    cmd(s, a, "gamerule natural_health_regeneration true");
    set_diff(s, Difficulty::Peaceful);
    s.world().get_mut::<PlayerLivedTicks>(p).unwrap().0 = 19;
    set_hunger(s, p, 19, 1.0, 0.0, 8);
    s.world().get_mut::<Vitals>(p).unwrap().health = 15.5;
    let _ = s.ticks_all(1);
    let m = health_pkts(&s.ticks(a, 1));
    assert_eq!(m.len(), 1, "peaceful merged packet count");
    assert_close(m[0].health, 16.833334, 1.0e-5, "peaceful hp");
    assert_eq!((m[0].food.0, m[0].saturation), (20, 2.0), "peaceful food");

    set_diff(s, Difficulty::Easy);
    s.world().get_mut::<Vitals>(p).unwrap().health = 20.0;
    set_hunger(s, p, 0, 0.0, 0.0, 0);
    let starve = s.world().resource::<DamageTypeIds>().starve.unwrap() as i32;
    let ev = &step(trace, "seq_starve_easy_normal_hard")["damage_events"][0];
    let exp_starve = ev["damage_type"].as_i64().unwrap() as i32;
    assert_eq!(starve, exp_starve, "starve wire id");
    for (diff, ticks, count, expected_hp) in [
        (Difficulty::Easy, 800, 10, 10.0),
        (Difficulty::Normal, 800, 9, 1.0),
        (Difficulty::Hard, 80, 1, 0.0),
    ] {
        set_diff(s, diff);
        let evs = dmg_events(&s.ticks(a, ticks));
        assert_eq!(evs.len(), count, "{diff:?} starve count");
        let ok = evs.iter().all(|e| {
            e.damage_type.0 == starve
                && e.cause_entity_id.0 == 0
                && e.direct_entity_id.0 == 0
                && e.source_position.is_none()
        });
        assert!(ok, "{diff:?} starve fields");
        assert_eq!(hp(s, p), expected_hp, "{diff:?} starve floor");
    }
}

#[test]
fn server_hunger_matches_probe_trace_sequences() {
    let trace: Value = serde_json::from_str(TRACE).unwrap();
    let mut s = start_server();
    let (a, p, net_id, join_pkts) = join_survival(&mut s);
    let join = &health_pkts(&join_pkts)[0];
    let exp = &step(&trace, "join_alice")["health"][0];
    let e_hp = exp["health"].as_f64().unwrap() as f32;
    let e_food = exp["food"].as_i64().unwrap() as i32;
    let e_sat = exp["saturation"].as_f64().unwrap() as f32;
    let got = (join.health, join.food.0, join.saturation);
    assert_eq!(got, (e_hp, e_food, e_sat), "join health");
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
    assert_eq!(hunger_of(&mut s, p).3, 0.5, "creative exh unchanged");

    set_hunger(&mut s, p, 4, 0.5, 1.75, 62);
    s.world().get_mut::<PlayerLivedTicks>(p).unwrap().0 = 500;
    cmd(&mut s, a, "kill @s");
    let action = ClientCommandAction::PerformRespawn;
    s.queue(a, &ServerboundClientCommand { action });
    let _ = s.ticks_all(3);
    assert_eq!(hunger_of(&mut s, p), (20, 0, 5.0, 0.0), "respawn hunger");
    let lived = s.world().get::<PlayerLivedTicks>(p).unwrap().0;
    assert!(lived <= 3, "respawn ticks");
}
