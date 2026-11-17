use bevy_ecs::entity::Entity;
use bevy_math::DVec3;
use bevy_time::TimeUpdateStrategy;
use bytes::Bytes;
use mcrs_command::Limits;
use mcrs_minecraft_command_engine::{CommandLimits, CommandPlugin};
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_core::Look;
use mcrs_minecraft_engine::bus::PacketPayload;
use mcrs_minecraft_engine::column_version::ColumnVersion;
use mcrs_minecraft_engine::coordinator::birth_clock::BirthClockPlugin;
use mcrs_minecraft_engine::coordinator::clock::WorldClockHostPlugin;
use mcrs_minecraft_engine::player_state::{Effect, PlayerFileState};
use mcrs_minecraft_engine::teleport::TeleportState;
use mcrs_minecraft_engine_item::{ItemCommandsExt, StackCommandsExt};
use mcrs_minecraft_engine_physics::Mobs;
use mcrs_minecraft_engine_registries::{install_item_table, test_item_table};
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::world::storage::column::ColumnPos;
use mcrs_minecraft_engine_world::world::sub_app::AddDimensionPlugins;
use mcrs_minecraft_environment::{AdvanceTime, WorldClockPlugin};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{
    AttackAnimation, Count, InteractAnimation, ItemStack, Kind, PiercingWeapon, SwingAnimation,
    SwingKind,
};
use mcrs_minecraft_protocol::packets::common::serverbound::ClientInformation;
use mcrs_minecraft_protocol::setting::{ChatMode, DisplayedSkinParts, MainArm, ParticleStatus};
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol::{Bounded, Decode, Packet, VarInt};
use mcrs_minecraft_protocol_game::clientbound::ClientboundSwingAnimation;
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{
    ServerboundAcceptTeleportation, ServerboundChatCommand, ServerboundChunkBatchReceived,
    ServerboundClientInformation, ServerboundCommandSuggestion, ServerboundPlayerAction,
    ServerboundPlayerLoaded, ServerboundPunch, ServerboundSetCarriedItem, ServerboundUseItemOn,
};
use mcrs_minecraft_protocol_game::{Direction, Hand, SwingAnimationKind};
use mcrs_minecraft_server::operator_broadcast::OperatorBroadcastPlugin;
use mcrs_minecraft_server::{
    VanillaBlockPlacePlugin, VanillaBlocksPlugin, VanillaEntityPhysicsPlugin,
    VanillaItemEntityPlugin, VanillaItemPlugin, VanillaSwingPlugin,
};
use mcrs_minecraft_text::TextContent;
use mcrs_voxel::BlockPos;

use crate::commands::{chats, load_stone_column};
use crate::host_app::{self, PlayServer, label_of, replicated, sent_to};

const ALICE: Uuid = Uuid::from_u128(101);
const BOB: Uuid = Uuid::from_u128(202);
const CHARLIE: Uuid = Uuid::from_u128(303);

fn start_swing_server(dimensions: &[(&str, &str)]) -> PlayServer {
    let mut app = host_app::make_host_app();
    install_item_table(app.world_mut(), test_item_table()).unwrap();
    app.insert_resource(CommandLimits(Limits::default()));
    app.add_plugins(WorldClockPlugin);
    app.insert_resource(AdvanceTime(false));
    app.add_plugins(WorldClockHostPlugin);
    app.add_plugins(BirthClockPlugin);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.add_plugins(CommandPlugin::<VanillaCommands>::new());
    app.add_dimension_plugin(OperatorBroadcastPlugin);
    app.add_plugins((
        VanillaEntityPhysicsPlugin,
        VanillaBlocksPlugin,
        VanillaItemEntityPlugin,
        VanillaItemPlugin,
        VanillaBlockPlacePlugin,
        VanillaSwingPlugin,
        VanillaCommandsPlugin,
    ));
    let mut server = PlayServer::start_in(app, dimensions, &[(ALICE, 2)], 0);
    load_stone_column(&mut server);
    server
}

fn advance_ticks(
    server: &mut PlayServer,
    ticks: usize,
) -> Vec<mcrs_minecraft_engine::bus::OutboundPlayerPacket> {
    server.ticks_all(ticks)
}

fn step_and_collect(
    server: &mut PlayServer,
    anchors: &[Entity],
    ticks: usize,
) -> Vec<Vec<PacketPayload>> {
    let raw = advance_ticks(server, ticks);
    anchors
        .iter()
        .map(|&anchor| {
            let session = server.session_of(anchor).unwrap();
            sent_to(session, raw.clone())
        })
        .collect()
}

fn extract_swings(packets: &[PacketPayload]) -> Vec<(i32, Hand, SwingAnimationKind, i32)> {
    let mut out = Vec::new();
    for pkt in packets {
        match pkt {
            PacketPayload::SwingAnimation(swing) => {
                out.push((
                    swing.entity_id.0,
                    swing.hand,
                    swing.animation,
                    swing.duration.0,
                ));
            }
            PacketPayload::Replication(_) => {
                for (id, bytes) in replicated(std::slice::from_ref(pkt)) {
                    if id == ClientboundSwingAnimation::ID {
                        let mut slice: &[u8] = &bytes;
                        let swing = ClientboundSwingAnimation::decode(&mut slice).unwrap();
                        out.push((
                            swing.entity_id.0,
                            swing.hand,
                            swing.animation,
                            swing.duration.0,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_order(packets: &[PacketPayload]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for pkt in packets {
        match pkt {
            PacketPayload::BlockUpdate(_) | PacketPayload::SectionBlocksUpdate(_) => {
                out.push("block_update");
            }
            PacketPayload::Replication(_) => {
                for (id, _) in replicated(std::slice::from_ref(pkt)) {
                    if id == ClientboundSwingAnimation::ID {
                        out.push("swing");
                    }
                }
            }
            PacketPayload::SwingAnimation(_) => out.push("swing"),
            _ => {}
        }
    }
    out
}

fn translate_keys(packets: &[PacketPayload]) -> Vec<String> {
    chats(packets)
        .into_iter()
        .filter_map(|text| match &text.content {
            TextContent::Translate { translate, .. } => Some(translate.to_string()),
            TextContent::Text { text } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

fn action(action: PlayerAction) -> ServerboundPlayerAction {
    ServerboundPlayerAction {
        action,
        pos: BlockPos::new(0, 0, 0),
        direction: Direction::Up,
        sequence: VarInt(1),
    }
}

fn confirm_move(server: &mut PlayServer, anchor: Entity, dim: Entity, to: DVec3) {
    let player = server.player_in(dim, anchor).unwrap();
    server
        .world_in(dim)
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = to;
    let _ = advance_ticks(server, 2);
    if let Some(id) = server
        .world_in(dim)
        .get::<TeleportState>(player)
        .and_then(|tp| tp.pending())
        .map(|pending| pending.id)
    {
        server.queue(
            anchor,
            &ServerboundAcceptTeleportation {
                teleport_id: VarInt(id),
                position: to.into(),
                look: Look::default(),
            },
        );
        let _ = advance_ticks(server, 2);
    }
}

fn join_pair(server: &mut PlayServer) -> (Entity, Entity, Entity, Entity) {
    let (alice, _) = server.join(ALICE, "Alice");
    let (bob, _) = server.join(BOB, "Bob");
    let _ = advance_ticks(server, 5);
    let alice_ent = server.player(alice);
    let bob_ent = server.player(bob);
    (alice, bob, alice_ent, bob_ent)
}

fn push_effect(server: &mut PlayServer, player: Entity, id: &str, amplifier: u8, duration: i32) {
    server
        .world()
        .get_mut::<PlayerFileState>(player)
        .unwrap()
        .active_effects
        .push(Effect {
            id: id.to_owned(),
            amplifier,
            duration,
            ambient: false,
            show_particles: false,
            show_icon: Some(false),
            hidden_effect: None,
        });
}

fn suggest_swing(server: &mut PlayServer, anchor: Entity, id: i32, text: &str) -> Vec<String> {
    let sug = server.send(
        anchor,
        &ServerboundCommandSuggestion {
            id: VarInt(id),
            text: Bounded(text),
        },
    );
    sug.iter()
        .find_map(|p| match p {
            PacketPayload::CommandSuggestions(res) => Some(
                res.suggestions
                    .iter()
                    .map(|m| m.text.clone().into_owned())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap()
}

#[test]
fn swg_loaded_client_punch_fallback_and_main_arm_invariance() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let (alice, bob, _, _) = join_pair(&mut server);

    server.queue(alice, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert!(extract_swings(&pkts[0]).is_empty());
    assert!(
        extract_swings(&pkts[1]).is_empty(),
        "punch before ServerboundPlayerLoaded or 60-tick fallback must be ignored"
    );

    server.queue(alice, &ServerboundPlayerLoaded);
    server.queue(
        alice,
        &ServerboundClientInformation(ClientInformation {
            locale: "en_us",
            view_distance: 2,
            chat_mode: ChatMode::Enabled,
            chat_colors: true,
            displayed_skin_parts: DisplayedSkinParts::new(),
            main_arm: MainArm::Left,
            enable_text_filtering: false,
            allow_server_listings: true,
            particle_status: ParticleStatus::All,
        }),
    );
    let _ = advance_ticks(&mut server, 1);

    server.queue(alice, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert!(
        extract_swings(&pkts[0]).is_empty(),
        "punch is observer-only"
    );
    let bob_swings = extract_swings(&pkts[1]);
    assert_eq!(bob_swings.len(), 1);
    assert_eq!(
        bob_swings[0].1,
        Hand::Main,
        "left main_arm still wires as Main"
    );
    assert_eq!(bob_swings[0].2, SwingAnimationKind::Whack);
    assert_eq!(bob_swings[0].3, 6);

    let _ = advance_ticks(&mut server, 55);
    server.queue(bob, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        extract_swings(&pkts[0]).len(),
        1,
        "60-tick fallback enables punch without ServerboundPlayerLoaded"
    );
}

#[test]
fn swg_item_components_and_cooldowns() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let (alice, bob, alice_ent, _) = join_pair(&mut server);
    server.queue(alice, &ServerboundPlayerLoaded);
    let _ = advance_ticks(&mut server, 1);

    let stick = Kind(Item::Stick.id().number());
    let slot0 =
        ItemStack::new(stick, Count::new(1).unwrap()).set(AttackAnimation(SwingAnimation {
            kind: SwingKind::None,
            duration: 10,
        }));
    let slot1 = ItemStack::new(stick, Count::new(2).unwrap())
        .set(PiercingWeapon::new())
        .set(AttackAnimation(SwingAnimation {
            kind: SwingKind::Stab,
            duration: 12,
        }));
    let slot2 =
        ItemStack::new(stick, Count::new(1).unwrap()).set(AttackAnimation(SwingAnimation {
            kind: SwingKind::Whack,
            duration: 0,
        }));
    let world = server.world();
    world.commands().spawn_stack(slot0).put_at(alice_ent, 0);
    world.commands().spawn_stack(slot1).put_at(alice_ent, 1);
    world.commands().spawn_stack(slot2).put_at(alice_ent, 2);
    world.flush();

    server.queue(alice, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        (extract_swings(&pkts[1])[0].2, extract_swings(&pkts[1])[0].3),
        (SwingAnimationKind::None, 10)
    );
    for _ in 0..5 {
        server.queue(alice, &ServerboundPunch);
        assert!(
            extract_swings(&step_and_collect(&mut server, &[alice, bob], 1)[1]).is_empty(),
            "cooldown must block ticks T+1..T+5"
        );
    }

    server.queue(alice, &ServerboundSetCarriedItem { slot: 1 });
    server.queue(alice, &action(PlayerAction::Stab));
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        (extract_swings(&pkts[1])[0].2, extract_swings(&pkts[1])[0].3),
        (SwingAnimationKind::Stab, 12),
        "stab must work immediately on same tick as equip"
    );

    server.queue(alice, &ServerboundSetCarriedItem { slot: 2 });
    let _ = advance_ticks(&mut server, 7);
    server.queue(alice, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(extract_swings(&pkts[1])[0].3, 0);
    server.queue(alice, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        extract_swings(&pkts[1]).len(),
        1,
        "duration 0 must unlock immediately at T + 1"
    );
}

#[test]
fn swg_drop_and_same_tick_dedup() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let (alice, bob, alice_ent, _) = join_pair(&mut server);
    server.queue(alice, &ServerboundPlayerLoaded);
    let _ = advance_ticks(&mut server, 1);

    let stick = Kind(Item::Stick.id().number());
    let spear = ItemStack::new(stick, Count::new(3).unwrap())
        .set(PiercingWeapon::new())
        .set(AttackAnimation(SwingAnimation {
            kind: SwingKind::Stab,
            duration: 12,
        }));
    let world = server.world();
    world.commands().spawn_stack(spear).put_at(alice_ent, 0);
    world.flush();

    server.queue(alice, &ServerboundPunch);
    server.queue(alice, &action(PlayerAction::DropItem));
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        extract_swings(&pkts[1]).len(),
        1,
        "SWG-STA-004: at most one swing per actor per tick"
    );

    let _ = advance_ticks(&mut server, 8);
    server.queue(alice, &action(PlayerAction::DropItem));
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    let bob_swings = extract_swings(&pkts[1]);
    assert_eq!(bob_swings.len(), 1);
    assert_eq!(
        (bob_swings[0].1, bob_swings[0].2, bob_swings[0].3),
        (Hand::Main, SwingAnimationKind::Whack, 6),
        "drop must use default main whack 6 regardless of item components"
    );
}

#[test]
fn swg_effects_fatigue_haste_conduit_and_decay() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let (alice, bob, alice_ent, _) = join_pair(&mut server);
    server.queue(alice, &ServerboundPlayerLoaded);
    let _ = advance_ticks(&mut server, 1);

    push_effect(&mut server, alice_ent, "minecraft:mining_fatigue", 1, 8);
    server.queue(alice, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        extract_swings(&pkts[1])[0].3,
        6,
        "wire duration must stay unscaled under mining fatigue"
    );
    for _ in 0..5 {
        server.queue(alice, &ServerboundPunch);
        assert!(
            extract_swings(&step_and_collect(&mut server, &[alice, bob], 1)[1]).is_empty(),
            "fatigue D=10 blocks up to T+5"
        );
    }
    server.queue(alice, &ServerboundPunch);
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        extract_swings(&pkts[1]).len(),
        1,
        "fatigue D=10 unlocks at T+6"
    );

    let _ = advance_ticks(&mut server, 6);
    assert!(
        server
            .world()
            .get::<PlayerFileState>(alice_ent)
            .unwrap()
            .active_effects
            .is_empty(),
        "temporary status effects must decay to removal"
    );

    push_effect(&mut server, alice_ent, "minecraft:mining_fatigue", 4, 10);
    push_effect(&mut server, alice_ent, "minecraft:conduit_power", 0, 10);
    push_effect(&mut server, alice_ent, "minecraft:haste", 2, 10);
    server.queue(alice, &ServerboundPunch);
    assert_eq!(
        extract_swings(&step_and_collect(&mut server, &[alice, bob], 1)[1])[0].3,
        6
    );
    server.queue(alice, &ServerboundPunch);
    assert!(
        extract_swings(&step_and_collect(&mut server, &[alice, bob], 1)[1]).is_empty(),
        "haste 2 D=3 blocks T+1"
    );
    server.queue(alice, &ServerboundPunch);
    assert_eq!(
        extract_swings(&step_and_collect(&mut server, &[alice, bob], 1)[1]).len(),
        1,
        "haste 2 overrides fatigue 4 -> D=3 unlocks at T+2"
    );
}

#[test]
fn swg_spectator_late_viewer_and_dimension_change() {
    let mut server = start_swing_server(&[
        ("minecraft:overworld", "minecraft:overworld"),
        ("minecraft:the_nether", "minecraft:the_nether"),
    ]);
    let (alice, bob, _, bob_ent) = join_pair(&mut server);
    server.queue(alice, &ServerboundPlayerLoaded);
    server.queue(bob, &ServerboundPlayerLoaded);
    let _ = advance_ticks(&mut server, 1);

    server.world().get_mut::<PlayerGameMode>(bob_ent).unwrap().0 = GameMode::Spectator;
    let _ = advance_ticks(&mut server, 1);
    server.queue(bob, &ServerboundPunch);
    assert!(
        extract_swings(&step_and_collect(&mut server, &[alice, bob], 1)[0]).is_empty(),
        "spectator punch must emit nothing"
    );
    server.queue(alice, &ServerboundPunch);
    assert_eq!(
        extract_swings(&step_and_collect(&mut server, &[alice, bob], 1)[1]).len(),
        1,
        "spectator viewer still receives observer swings"
    );

    let (charlie, charlie_pkts) = server.join(CHARLIE, "Charlie");
    assert!(
        extract_swings(&charlie_pkts).is_empty(),
        "late viewer joining must never replay old swings"
    );
    let _ = charlie;

    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("execute in minecraft:the_nether run tp @s 8 64 8"),
        },
    );
    let _ = advance_ticks(&mut server, 4);
    let nether = label_of(&server.app, "minecraft:the_nether");
    let (bob2, _) = server.join_in(nether, Uuid::from_u128(404), "BobNether");
    let _ = advance_ticks(&mut server, 4);

    server.queue(alice, &ServerboundPunch);
    assert!(
        extract_swings(&step_and_collect(&mut server, &[bob2], 1)[0]).is_empty(),
        "LoadedClient must reset after dimension change"
    );
    server.queue(alice, &ServerboundPlayerLoaded);
    let _ = advance_ticks(&mut server, 1);
    server.queue(alice, &ServerboundPunch);
    assert_eq!(
        extract_swings(&step_and_collect(&mut server, &[bob2], 1)[0]).len(),
        1
    );
}

#[test]
fn plc_block_place_swing_precedes_block_update_and_ignores_refusal() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let (alice, bob, alice_ent, _) = join_pair(&mut server);
    let dim = server.dim;
    confirm_move(&mut server, alice, dim, DVec3::new(8.5, 64.0, 8.5));
    confirm_move(&mut server, bob, dim, DVec3::new(10.5, 64.0, 8.5));
    server.queue(
        alice,
        &ServerboundChunkBatchReceived {
            desired_chunks_per_tick: 64.0,
        },
    );
    server.queue(
        bob,
        &ServerboundChunkBatchReceived {
            desired_chunks_per_tick: 64.0,
        },
    );
    server.world().write_message(ColumnVersion {
        column: ColumnPos::new(0, 0),
        packet: Bytes::from_static(b"chunk"),
    });
    let _ = advance_ticks(&mut server, 3);

    let stone = Kind(Item::Stone.id().number());
    let off_stack =
        ItemStack::new(stone, Count::new(16).unwrap()).set(InteractAnimation(SwingAnimation {
            kind: SwingKind::Stab,
            duration: 8,
        }));
    let world = server.world();
    world
        .commands()
        .spawn_stack(off_stack)
        .put_at(alice_ent, 40);
    world.flush();

    let place = ServerboundUseItemOn {
        hand: Hand::Off,
        block_pos: BlockPos::new(6, 63, 8),
        face: Direction::Up,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: VarInt(1),
    };
    server.queue(alice, &place);
    let pkts = step_and_collect(&mut server, &[alice, bob], 2);

    assert!(
        extract_swings(&pkts[0]).is_empty(),
        "client-predicted block placement swing must be observer-only"
    );
    let bob_swings = extract_swings(&pkts[1]);
    assert_eq!(bob_swings.len(), 1);
    assert_eq!(
        (bob_swings[0].1, bob_swings[0].2, bob_swings[0].3),
        (Hand::Off, SwingAnimationKind::Stab, 8)
    );
    let order = extract_order(&pkts[1]);
    let swing_idx = order.iter().position(|&k| k == "swing").unwrap();
    let block_idx = order.iter().position(|&k| k == "block_update").unwrap();
    assert!(
        swing_idx < block_idx,
        "SWG-DST-003: swing_animation must precede block_update in {order:?}"
    );

    let _ = advance_ticks(&mut server, 5);
    let out_of_reach = ServerboundUseItemOn {
        block_pos: BlockPos::new(100, 63, 100),
        sequence: VarInt(2),
        ..place
    };
    server.queue(alice, &out_of_reach);
    let pkts = step_and_collect(&mut server, &[alice, bob], 2);
    assert!(
        extract_swings(&pkts[1]).is_empty(),
        "PLC-INT-004: refused block place must emit no swing"
    );
}

#[test]
fn cmd_swing_suggestions_mobs_and_cross_dim() {
    let mut server = start_swing_server(&[
        ("minecraft:overworld", "minecraft:overworld"),
        ("minecraft:the_nether", "minecraft:the_nether"),
    ]);
    let (alice, bob, _, _) = join_pair(&mut server);

    assert_eq!(
        suggest_swing(&mut server, alice, 1, "/swing Alice mainhand "),
        vec!["none", "stab", "whack"]
    );
    assert_eq!(
        suggest_swing(&mut server, alice, 2, "/swing Alice mainhand S"),
        vec!["stab"]
    );

    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing"),
        },
    );
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert_eq!(
        translate_keys(&pkts[0]),
        vec!["commands.swing.success.single"]
    );
    assert_eq!(
        extract_swings(&pkts[0]).len(),
        1,
        "/swing must include self"
    );
    assert_eq!(
        extract_swings(&pkts[1]).len(),
        1,
        "/swing must reach observers"
    );

    let _ = advance_ticks(&mut server, 5);
    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("summon minecraft:pig 8 64 8"),
        },
    );
    let _ = advance_ticks(&mut server, 3);
    let pig_id = *server.world().resource::<Mobs>().0.keys().next().unwrap();

    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing @e offhand stab 2s"),
        },
    );
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert!(translate_keys(&pkts[0]).contains(&"commands.swing.success.multiple".to_owned()));
    assert!(
        extract_swings(&pkts[0])
            .iter()
            .any(|s| s.0 == pig_id.0 as i32
                && s.1 == Hand::Off
                && s.2 == SwingAnimationKind::Stab
                && s.3 == 40)
    );

    let nether = label_of(&server.app, "minecraft:the_nether");
    let (bob_nether, _) = server.join_in(nether, CHARLIE, "CharlieNether");
    let _ = advance_ticks(&mut server, 3);
    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing CharlieNether offhand stab 8"),
        },
    );
    let pkts = step_and_collect(&mut server, &[alice, bob_nether], 2);
    assert!(translate_keys(&pkts[0]).contains(&"commands.swing.success.single".to_owned()));
    let nether_swings = extract_swings(&pkts[1]);
    assert_eq!(
        (nether_swings[0].1, nether_swings[0].2, nether_swings[0].3),
        (Hand::Off, SwingAnimationKind::Stab, 8)
    );
}

#[test]
fn cmd_swing_cooldown_spectator_and_errors() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let (alice, bob, _, bob_ent) = join_pair(&mut server);

    let guest_denied = server.send(
        bob,
        &ServerboundChatCommand {
            command: Bounded("swing"),
        },
    );
    assert!(translate_keys(&guest_denied).contains(&"command.unknown.command".to_owned()));

    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing Bob offhand stab 2s"),
        },
    );
    let _ = advance_ticks(&mut server, 1);

    server.world().get_mut::<PlayerGameMode>(bob_ent).unwrap().0 = GameMode::Spectator;
    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing Bob mainhand whack 10"),
        },
    );
    let pkts = step_and_collect(&mut server, &[alice, bob], 1);
    assert!(translate_keys(&pkts[0]).contains(&"commands.swing.success.single".to_owned()));
    assert!(
        extract_swings(&pkts[1]).is_empty(),
        "cooldown from previous /swing blocks swing emission"
    );

    let _ = advance_ticks(&mut server, 22);
    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing Bob mainhand whack 6"),
        },
    );
    let pkts = step_and_collect(&mut server, &[alice, bob], 2);
    assert_eq!(
        extract_swings(&pkts[1]).len(),
        1,
        "/swing Bob in Spectator mode must emit swing to spectator"
    );

    let _ = advance_ticks(&mut server, 5);
    {
        let mut state = server.world().get_mut::<PlayerFileState>(bob_ent).unwrap();
        state.health = 0.0;
        state.death_time = 20;
    }
    server.queue(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing Bob mainhand whack 6"),
        },
    );
    let pkts = step_and_collect(&mut server, &[alice, bob], 2);
    assert!(translate_keys(&pkts[0]).contains(&"commands.swing.success.single".to_owned()));
    assert!(
        pkts.iter()
            .all(|packets| extract_swings(packets).is_empty()),
        "Bob still counts after corpse removal and the command emits no swing"
    );

    let bad = server.send(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing Alice mainhand WHACK"),
        },
    );
    assert!(translate_keys(&bad).contains(&"argument.swing_animation.invalid".to_owned()));

    let low = server.send(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing Alice mainhand whack 0t"),
        },
    );
    assert!(translate_keys(&low).contains(&"argument.time.tick_count_too_low".to_owned()));

    let notfound = server.send(
        alice,
        &ServerboundChatCommand {
            command: Bounded("swing @e[type=cow]"),
        },
    );
    assert!(translate_keys(&notfound).contains(&"argument.entity.notfound.entity".to_owned()));
}
