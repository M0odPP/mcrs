use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_math::DVec3;
use bevy_time::TimeUpdateStrategy;
use bytes::Bytes;
use mcrs_command::Limits;
use mcrs_minecraft_command_engine::{CommandLimits, CommandPlugin};
use mcrs_minecraft_command_tree::{CommandNodeKind, ParserProperties};
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaCommandsPlugin};
use mcrs_minecraft_core::Look;
use mcrs_minecraft_engine::bus::PacketPayload;
use mcrs_minecraft_engine::column_version::ColumnVersion;
use mcrs_minecraft_engine::coordinator::birth_clock::BirthClockPlugin;
use mcrs_minecraft_engine::coordinator::clock::WorldClockHostPlugin;
use mcrs_minecraft_engine::player_state::{Effect, PlayerFileState};
use mcrs_minecraft_engine::teleport::TeleportState;
use mcrs_minecraft_engine_entity_motion::MotionId;
use mcrs_minecraft_engine_item::{ItemCommandsExt, StackCommandsExt};
use mcrs_minecraft_engine_physics::Mob;
use mcrs_minecraft_engine_registries::{install_item_table, test_item_table};
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::world::storage::column::ColumnPos;
use mcrs_minecraft_engine_world::world::sub_app::AddDimensionPlugins;
use mcrs_minecraft_environment::{AdvanceTime, WorldClockPlugin};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_rule::GameRule;
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
use mcrs_minecraft_text::{Text, TextContent};
use mcrs_voxel::BlockPos;

use crate::commands::{child, child_names, load_stone_column, sent_tree};
use crate::host_app::{self, PlayServer, label_of, replicated, sent_to};
use crate::support::chats;

const ALICE: Uuid = Uuid::from_u128(101);
const BOB: Uuid = Uuid::from_u128(202);
const CHARLIE: Uuid = Uuid::from_u128(303);
const CARL_OP1: Uuid = Uuid::from_u128(404);
const DAN_OP2: Uuid = Uuid::from_u128(505);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObservedSwing {
    entity_id: i32,
    hand: Hand,
    animation: SwingAnimationKind,
    duration: i32,
}

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
    let mut server = PlayServer::start_in(
        app,
        dimensions,
        &[(ALICE, 2), (CARL_OP1, 1), (DAN_OP2, 2)],
        0,
    );
    load_stone_column(&mut server);
    server
}

fn step_and_collect(
    server: &mut PlayServer,
    anchors: &[Entity],
    ticks: usize,
) -> Vec<Vec<PacketPayload>> {
    let raw = server.ticks_all(ticks);
    anchors
        .iter()
        .map(|&anchor| {
            let session = server.session_of(anchor).unwrap();
            sent_to(session, raw.clone())
        })
        .collect()
}

fn extract_swings(packets: &[PacketPayload]) -> Vec<ObservedSwing> {
    let mut out = Vec::new();
    for packet in packets {
        if matches!(packet, PacketPayload::Replication(_)) {
            for (id, bytes) in replicated(std::slice::from_ref(packet)) {
                if id == ClientboundSwingAnimation::ID {
                    let mut slice: &[u8] = &bytes;
                    let swing = ClientboundSwingAnimation::decode(&mut slice).unwrap();
                    out.push(ObservedSwing {
                        entity_id: swing.entity_id.0,
                        hand: swing.hand,
                        animation: swing.animation,
                        duration: swing.duration.0,
                    });
                }
            }
        }
    }
    out
}

fn extract_order(packets: &[PacketPayload]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for packet in packets {
        match packet {
            PacketPayload::BlockUpdate(_) | PacketPayload::SectionBlocksUpdate(_) => {
                out.push("block_update");
            }
            PacketPayload::Replication(_) => {
                for (id, _) in replicated(std::slice::from_ref(packet)) {
                    if id == ClientboundSwingAnimation::ID {
                        out.push("swing");
                    }
                }
            }
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

fn queue_cmd(server: &mut PlayServer, anchor: Entity, command: &str) {
    server.queue(
        anchor,
        &ServerboundChatCommand {
            command: Bounded(command),
        },
    );
}

fn send_cmd(server: &mut PlayServer, anchor: Entity, command: &str) -> Vec<PacketPayload> {
    server.send(
        anchor,
        &ServerboundChatCommand {
            command: Bounded(command),
        },
    )
}

fn suggest_swing(server: &mut PlayServer, anchor: Entity, id: i32, text: &str) -> Vec<String> {
    let reply = server.send(
        anchor,
        &ServerboundCommandSuggestion {
            id: VarInt(id),
            text: Bounded(text),
        },
    );
    reply
        .iter()
        .find_map(|packet| match packet {
            PacketPayload::CommandSuggestions(suggestions) => Some(
                suggestions
                    .suggestions
                    .iter()
                    .map(|entry| entry.text.clone().into_owned())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap()
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
    let _ = server.ticks_all(2);
    if let Some(id) = server
        .world_in(dim)
        .get::<TeleportState>(player)
        .and_then(|teleport| teleport.pending())
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
        let _ = server.ticks_all(2);
    }
}

struct JoinedPair {
    alice: Entity,
    bob: Entity,
    alice_entity: Entity,
    bob_entity: Entity,
}

fn join_pair(server: &mut PlayServer) -> JoinedPair {
    let (alice, _) = server.join(ALICE, "Alice");
    let (bob, _) = server.join(BOB, "Bob");
    let _ = server.ticks_all(5);
    JoinedPair {
        alice,
        bob,
        alice_entity: server.player(alice),
        bob_entity: server.player(bob),
    }
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

#[test]
fn swg_loaded_client_punch_fallback_and_main_arm_invariance() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let pair = join_pair(&mut server);

    server.queue(pair.alice, &ServerboundPunch);
    let packets = step_and_collect(&mut server, &[pair.alice, pair.bob], 1);
    assert!(
        extract_swings(&packets[0]).is_empty(),
        "actor never receives observer-only punch"
    );
    assert!(
        extract_swings(&packets[1]).is_empty(),
        "SWG-SRC-002: punch before ServerboundPlayerLoaded or 60-tick fallback must be ignored"
    );

    server.queue(pair.alice, &ServerboundPlayerLoaded);
    server.queue(
        pair.alice,
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
    let _ = server.ticks_all(1);

    server.queue(pair.alice, &ServerboundPunch);
    let packets = step_and_collect(&mut server, &[pair.alice, pair.bob], 1);
    assert!(
        extract_swings(&packets[0]).is_empty(),
        "SWG-SRC-001: punch is observer-only"
    );
    let bob_swings = extract_swings(&packets[1]);
    assert_eq!(bob_swings.len(), 1);
    assert_eq!(
        (
            bob_swings[0].hand,
            bob_swings[0].animation,
            bob_swings[0].duration
        ),
        (Hand::Main, SwingAnimationKind::Whack, 6),
        "SWG-PKT-001: left main_arm still wires as Main"
    );

    let _ = server.ticks_all(55);
    server.queue(pair.bob, &ServerboundPunch);
    let packets = step_and_collect(&mut server, &[pair.alice, pair.bob], 1);
    assert_eq!(
        extract_swings(&packets[0]).len(),
        1,
        "SWG-SRC-001: 60-tick fallback enables punch without ServerboundPlayerLoaded"
    );
}

fn equip_test_items(server: &mut PlayServer, player: Entity) {
    let stick = Kind(Item::Stick.id().number());
    let items = [
        (
            0,
            ItemStack::new(stick, Count::new(1).unwrap()).set(AttackAnimation(SwingAnimation {
                kind: SwingKind::None,
                duration: 10,
            })),
        ),
        (
            1,
            ItemStack::new(stick, Count::new(3).unwrap())
                .set(PiercingWeapon::new())
                .set(AttackAnimation(SwingAnimation {
                    kind: SwingKind::Stab,
                    duration: 12,
                })),
        ),
        (
            2,
            ItemStack::new(stick, Count::new(1).unwrap()).set(AttackAnimation(SwingAnimation {
                kind: SwingKind::Whack,
                duration: 0,
            })),
        ),
    ];
    let world = server.world();
    for (slot, stack) in items {
        world.commands().spawn_stack(stack).put_at(player, slot);
    }
    world.flush();
}

fn swings_for(server: &mut PlayServer, viewer: Entity) -> Vec<ObservedSwing> {
    extract_swings(&step_and_collect(server, &[viewer], 1)[0])
}

#[test]
fn swg_item_components_effects_and_cooldowns() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let pair = join_pair(&mut server);
    server.queue(pair.alice, &ServerboundPlayerLoaded);
    let _ = server.ticks_all(1);
    equip_test_items(&mut server, pair.alice_entity);

    server.queue(pair.alice, &ServerboundPunch);
    let swing = swings_for(&mut server, pair.bob)[0];
    assert_eq!(
        (swing.animation, swing.duration),
        (SwingAnimationKind::None, 10)
    );
    for _ in 0..5 {
        server.queue(pair.alice, &ServerboundPunch);
        assert!(
            swings_for(&mut server, pair.bob).is_empty(),
            "SWG-STA-005: cooldown must block ticks T+1..T+5"
        );
    }

    server.queue(pair.alice, &ServerboundSetCarriedItem { slot: 1 });
    server.queue(pair.alice, &action(PlayerAction::Stab));
    let swing = swings_for(&mut server, pair.bob)[0];
    assert_eq!(
        (swing.animation, swing.duration),
        (SwingAnimationKind::Stab, 12),
        "SWG-SRC-003: stab must work immediately on same tick as equip"
    );

    let _ = server.ticks_all(7);
    server.queue(pair.alice, &ServerboundPunch);
    server.queue(pair.alice, &action(PlayerAction::DropItem));
    assert_eq!(
        swings_for(&mut server, pair.bob).len(),
        1,
        "SWG-STA-004: at most one swing per actor per tick"
    );

    let _ = server.ticks_all(7);
    server.queue(pair.alice, &action(PlayerAction::DropItem));
    let bob_swings = swings_for(&mut server, pair.bob);
    assert_eq!(bob_swings.len(), 1);
    assert_eq!(
        (
            bob_swings[0].hand,
            bob_swings[0].animation,
            bob_swings[0].duration
        ),
        (Hand::Main, SwingAnimationKind::Whack, 6),
        "SWG-CMP-002: drop must use default main whack 6 regardless of item components"
    );

    server.queue(pair.alice, &ServerboundSetCarriedItem { slot: 2 });
    let _ = server.ticks_all(4);
    server.queue(pair.alice, &ServerboundPunch);
    assert_eq!(swings_for(&mut server, pair.bob)[0].duration, 0);
    server.queue(pair.alice, &ServerboundPunch);
    assert_eq!(
        swings_for(&mut server, pair.bob).len(),
        1,
        "SWG-STA-003: duration 0 must unlock immediately at T + 1"
    );

    server.queue(pair.alice, &ServerboundSetCarriedItem { slot: 3 });
    push_effect(
        &mut server,
        pair.alice_entity,
        "minecraft:mining_fatigue",
        1,
        10,
    );
    server.queue(pair.alice, &ServerboundPunch);
    assert_eq!(
        swings_for(&mut server, pair.bob)[0].duration,
        6,
        "SWG-CMP-003: wire duration must stay unscaled under mining fatigue"
    );
    for _ in 0..5 {
        server.queue(pair.alice, &ServerboundPunch);
        assert!(
            swings_for(&mut server, pair.bob).is_empty(),
            "SWG-STA-002: fatigue D=10 blocks up to T+5"
        );
    }
    server.queue(pair.alice, &ServerboundPunch);
    assert_eq!(
        swings_for(&mut server, pair.bob).len(),
        1,
        "SWG-STA-002: fatigue D=10 unlocks at T+6"
    );
}

#[test]
fn swg_spectator_range_late_viewer_and_dimension_change() {
    let mut server = start_swing_server(&[
        ("minecraft:overworld", "minecraft:overworld"),
        ("minecraft:the_nether", "minecraft:the_nether"),
    ]);
    let pair = join_pair(&mut server);
    server.queue(pair.alice, &ServerboundPlayerLoaded);
    server.queue(pair.bob, &ServerboundPlayerLoaded);
    let _ = server.ticks_all(1);

    server
        .world()
        .get_mut::<PlayerGameMode>(pair.bob_entity)
        .unwrap()
        .0 = GameMode::Spectator;
    let _ = server.ticks_all(1);
    server.queue(pair.bob, &ServerboundPunch);
    assert!(
        extract_swings(&step_and_collect(&mut server, &[pair.alice, pair.bob], 1)[0]).is_empty(),
        "SWG-SRC-002: spectator punch must emit nothing"
    );
    server.queue(pair.alice, &ServerboundPunch);
    assert_eq!(
        extract_swings(&step_and_collect(&mut server, &[pair.alice, pair.bob], 1)[1]).len(),
        1,
        "SWG-DST-001: spectator viewer still receives observer swings"
    );

    let (charlie, charlie_packets) = server.join(CHARLIE, "Charlie");
    assert!(
        extract_swings(&charlie_packets).is_empty(),
        "SWG-DST-002: late viewer joining must never replay old swings"
    );

    let dim = server.dim;
    confirm_move(&mut server, charlie, dim, DVec3::new(500.5, 64.0, 500.5));
    let _ = server.ticks_all(6);
    server.queue(pair.alice, &ServerboundPunch);
    let packets = step_and_collect(&mut server, &[pair.bob, charlie], 1);
    assert_eq!(
        extract_swings(&packets[0]).len(),
        1,
        "SWG-DST-001: tracking observer in range receives swing"
    );
    assert!(
        extract_swings(&packets[1]).is_empty(),
        "SWG-DST-001: player outside tracking range receives no swing"
    );

    server.queue(
        pair.alice,
        &ServerboundChatCommand {
            command: Bounded("execute in minecraft:the_nether run tp @s 8 64 8"),
        },
    );
    let _ = server.ticks_all(4);
    let nether = label_of(&server.app, "minecraft:the_nether");
    let (bob_nether, _) = server.join_in(nether, Uuid::from_u128(404), "BobNether");
    let _ = server.ticks_all(4);

    server.queue(pair.alice, &ServerboundPunch);
    assert!(
        extract_swings(&step_and_collect(&mut server, &[bob_nether], 1)[0]).is_empty(),
        "SWG-SRC-002: client load state must reset after dimension change"
    );
    server.queue(pair.alice, &ServerboundPlayerLoaded);
    let _ = server.ticks_all(1);
    server.queue(pair.alice, &ServerboundPunch);
    let nether_swings = extract_swings(&step_and_collect(&mut server, &[bob_nether], 1)[0]);
    assert_eq!(nether_swings.len(), 1);
    assert_ne!(
        nether_swings[0].entity_id, 0,
        "SWG-PKT-001: swing carries valid protocol entity id"
    );
}

#[test]
fn plc_block_place_swing_precedes_block_update_and_ignores_refusal() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let pair = join_pair(&mut server);
    server.queue(pair.alice, &ServerboundPlayerLoaded);
    let dim = server.dim;
    confirm_move(&mut server, pair.alice, dim, DVec3::new(8.5, 64.0, 8.5));
    confirm_move(&mut server, pair.bob, dim, DVec3::new(10.5, 64.0, 8.5));
    server.queue(
        pair.alice,
        &ServerboundChunkBatchReceived {
            desired_chunks_per_tick: 64.0,
        },
    );
    server.queue(
        pair.bob,
        &ServerboundChunkBatchReceived {
            desired_chunks_per_tick: 64.0,
        },
    );
    server.world().write_message(ColumnVersion {
        column: ColumnPos::new(0, 0),
        packet: Bytes::from_static(b"chunk"),
    });
    let _ = server.ticks_all(3);

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
        .put_at(pair.alice_entity, 40);
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
    server.queue(pair.alice, &place);
    let packets = step_and_collect(&mut server, &[pair.alice, pair.bob], 2);

    assert!(
        extract_swings(&packets[0]).is_empty(),
        "SWG-SRC-006: client-predicted block placement swing must be observer-only"
    );
    let bob_swings = extract_swings(&packets[1]);
    assert_eq!(bob_swings.len(), 1);
    assert_eq!(
        (
            bob_swings[0].hand,
            bob_swings[0].animation,
            bob_swings[0].duration
        ),
        (Hand::Off, SwingAnimationKind::Stab, 8)
    );
    let order = extract_order(&packets[1]);
    let swing_index = order.iter().position(|&kind| kind == "swing").unwrap();
    let block_index = order
        .iter()
        .position(|&kind| kind == "block_update")
        .unwrap();
    assert!(
        swing_index < block_index,
        "SWG-DST-003: swing_animation must precede block_update in {order:?}"
    );

    let _ = server.ticks_all(5);
    let out_of_reach = ServerboundUseItemOn {
        block_pos: BlockPos::new(100, 63, 100),
        sequence: VarInt(2),
        ..place
    };
    server.queue(pair.alice, &out_of_reach);
    let packets = step_and_collect(&mut server, &[pair.alice, pair.bob], 2);
    assert!(
        extract_swings(&packets[1]).is_empty(),
        "SWG-SRC-006: refused block place must emit no swing"
    );
}

fn run_cmd(
    server: &mut PlayServer,
    sender: Entity,
    command: &str,
    viewers: &[Entity],
    ticks: usize,
) -> Vec<Vec<PacketPayload>> {
    queue_cmd(server, sender, command);
    step_and_collect(server, viewers, ticks)
}

fn swing_tuple(swing: ObservedSwing) -> (Hand, SwingAnimationKind, i32) {
    (swing.hand, swing.animation, swing.duration)
}

fn assert_chat_key(packets: &[PacketPayload], key: &str, reason: &str) {
    assert!(translate_keys(packets).iter().any(|k| k == key), "{reason}");
}

fn assert_swing_tree(packets: &[PacketPayload]) {
    let tree = sent_tree(packets);
    let swing = child(&tree, &tree.nodes[tree.root.0 as usize], "swing");
    assert!(swing.executable && swing.restricted, "CMD-SWG-001: root");
    let targets = child(&tree, swing, "targets");
    assert!(targets.executable && !targets.restricted, "CMD-SWG-001");
    assert!(matches!(
        &targets.kind,
        CommandNodeKind::Argument { parser, suggestions: None, .. }
            if parser.properties == ParserProperties::Entity { single: false, players_only: false }
    ));
    assert_eq!(child_names(&tree, targets), ["mainhand", "offhand"]);
    for name in ["mainhand", "offhand"] {
        let hand = child(&tree, targets, name);
        let anim = child(&tree, hand, "animation");
        let dur = child(&tree, anim, "duration");
        assert!(hand.executable && !hand.restricted && anim.executable && !anim.restricted);
        assert!(dur.executable && !dur.restricted);
        assert!(matches!(
            &anim.kind,
            CommandNodeKind::Argument { parser, suggestions: None, .. }
                if parser.id == VarInt(60) && parser.properties == ParserProperties::None
        ));
        assert!(matches!(
            &dur.kind,
            CommandNodeKind::Argument { parser, suggestions: None, .. }
                if parser.properties == ParserProperties::Time(1)
        ));
    }
}

#[test]
fn cmd_swing_suggestions_defaults_and_cross_dim() {
    let mut server = start_swing_server(&[
        ("minecraft:overworld", "minecraft:overworld"),
        ("minecraft:the_nether", "minecraft:the_nether"),
    ]);
    let pair = join_pair(&mut server);
    let (_, dan_join) = server.join(DAN_OP2, "Dan");
    let (_, carl_join) = server.join(CARL_OP1, "Carl");
    assert_swing_tree(&dan_join);
    let carl_tree = sent_tree(&carl_join);
    assert!(
        !child_names(&carl_tree, &carl_tree.nodes[carl_tree.root.0 as usize])
            .contains(&"swing".to_owned()),
        "CMD-SWG-001: level-1 operator sees no swing subtree"
    );
    assert_eq!(
        suggest_swing(&mut server, pair.alice, 1, "/swing Alice mainhand "),
        ["none", "stab", "whack"]
    );
    assert_eq!(
        suggest_swing(&mut server, pair.alice, 2, "/swing Alice mainhand S"),
        ["stab"]
    );

    let stick = Kind(Item::Stick.id().number());
    let custom = ItemStack::new(stick, Count::new(1).unwrap())
        .set(AttackAnimation(SwingAnimation {
            kind: SwingKind::Stab,
            duration: 20,
        }))
        .set(InteractAnimation(SwingAnimation {
            kind: SwingKind::None,
            duration: 18,
        }));
    let world = server.world();
    world
        .commands()
        .spawn_stack(custom)
        .put_at(pair.alice_entity, 0);
    world.flush();

    let packets = run_cmd(&mut server, pair.alice, "swing", &[pair.alice, pair.bob], 1);
    assert_chat_key(&packets[0], "commands.swing.success.single", "CMD-SWG-007");
    let self_swings = extract_swings(&packets[0]);
    assert_eq!(self_swings.len(), 1, "/swing must include self");
    assert_eq!(
        swing_tuple(self_swings[0]),
        (Hand::Main, SwingAnimationKind::Whack, 6),
        "CMD-SWG-004: /swing defaults ignore held attack and interact animations"
    );
    assert_eq!(extract_swings(&packets[1]).len(), 1);

    let nether = label_of(&server.app, "minecraft:the_nether");
    let (charlie, _) = server.join_in(nether, CHARLIE, "CharlieNether");
    let _ = server.ticks_all(3);
    let packets = run_cmd(
        &mut server,
        pair.alice,
        "swing CharlieNether offhand stab 8",
        &[pair.alice, charlie],
        2,
    );
    assert_chat_key(&packets[0], "commands.swing.success.single", "CMD-SWG-007");
    assert!(extract_swings(&packets[0]).is_empty(), "CMD-SWG-006");
    let nether_swings = extract_swings(&packets[1]);
    assert_eq!(nether_swings.len(), 1);
    assert_eq!(
        swing_tuple(nether_swings[0]),
        (Hand::Off, SwingAnimationKind::Stab, 8)
    );
}

#[test]
fn cmd_swing_filters_non_living_items_and_swings_mobs() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let pair = join_pair(&mut server);
    let (dan, _) = server.join(DAN_OP2, "Dan");
    let _ = server.ticks_all(2);

    queue_cmd(&mut server, pair.alice, "summon minecraft:pig 8 64 8");
    queue_cmd(
        &mut server,
        pair.alice,
        r#"summon minecraft:item 8 64 8 {Item:{id:"minecraft:stone",count:1},PickupDelay:32767s}"#,
    );
    let _ = server.ticks_all(3);
    let pig_id = {
        let world = server.world();
        world
            .query_filtered::<&MotionId, With<Mob>>()
            .iter(world)
            .next()
            .expect("summoned pig has MotionId")
            .0
    };

    let failed = run_cmd(
        &mut server,
        pair.alice,
        "swing @e[type=minecraft:item]",
        &[pair.alice, dan],
        1,
    );
    assert_chat_key(&failed[0], "commands.swing.failed.notliving", "CMD-SWG-005");
    assert!(chats(&failed[1]).is_empty(), "CMD-SWG-007: no op broadcast");

    let packets = run_cmd(
        &mut server,
        pair.alice,
        "swing @e offhand stab 2s",
        &[pair.alice, pair.bob, dan],
        1,
    );
    assert!(
        chats(&packets[0]).contains(&Text::translate_args(
            "commands.swing.success.multiple",
            vec![4i32.into()],
        )),
        "CMD-SWG-005/007: @e counts only the 4 living targets and excludes the item"
    );
    assert_chat_key(&packets[2], "chat.type.admin", "CMD-SWG-007: op broadcast");
    assert!(
        extract_swings(&packets[0]).iter().any(|s| {
            s.entity_id == pig_id.0 as i32
                && swing_tuple(*s) == (Hand::Off, SwingAnimationKind::Stab, 40)
        }),
        "CMD-SWG-005/006: @e swings living pig with resolved 2s = 40 ticks"
    );

    let _ = server.ticks_all(22);
    let mut rules = GameRules::default();
    rules.set(GameRule::SendCommandFeedback, 0);
    server.app.world_mut().insert_resource(rules);
    let _ = server.ticks_all(1);
    let silent = run_cmd(
        &mut server,
        pair.alice,
        "swing Alice mainhand whack 6",
        &[pair.alice, pair.bob, dan],
        1,
    );
    assert!(
        chats(&silent[0]).is_empty() && chats(&silent[2]).is_empty(),
        "CMD-SWG-007: send_command_feedback=false silences source and operator chat"
    );
    assert_eq!(extract_swings(&silent[0]).len(), 1);

    queue_cmd(&mut server, pair.alice, "kill @e[type=minecraft:item]");
    let _ = server.ticks_all(2);
    assert_chat_key(
        &send_cmd(&mut server, pair.alice, "swing @e[type=minecraft:item]"),
        "argument.entity.notfound.entity",
        "/kill @e[type=minecraft:item] despawns the item entity",
    );
}

fn assert_swing_errors(server: &mut PlayServer, alice: Entity, denied: &[Entity]) {
    for actor in denied {
        assert_chat_key(
            &send_cmd(server, *actor, "swing"),
            "command.unknown.command",
            "CMD-SWG-001: guests and level-1 operators cannot run /swing",
        );
    }
    for (line, expected) in [
        (
            "swing Alice mainhand WHACK",
            "argument.swing_animation.invalid",
        ),
        (
            "swing Alice mainhand whack 0t",
            "argument.time.tick_count_too_low",
        ),
        ("swing @e[type=cow]", "argument.entity.notfound.entity"),
    ] {
        assert_chat_key(&send_cmd(server, alice, line), expected, line);
    }
}

#[test]
fn cmd_swing_cooldown_spectator_same_tick_and_errors() {
    let mut server = start_swing_server(&[("minecraft:overworld", "minecraft:overworld")]);
    let pair = join_pair(&mut server);
    let (carl, _) = server.join(CARL_OP1, "Carl");
    let _ = server.ticks_all(2);
    let both = [pair.alice, pair.bob];
    assert_swing_errors(&mut server, pair.alice, &[pair.bob, carl]);

    server.queue(pair.alice, &ServerboundPlayerLoaded);
    let packets = run_cmd(
        &mut server,
        pair.alice,
        "swing Alice mainhand none 15",
        &both,
        1,
    );
    assert_eq!(
        swing_tuple(extract_swings(&packets[0])[0]),
        (Hand::Main, SwingAnimationKind::None, 15)
    );
    server.queue(pair.alice, &ServerboundPunch);
    assert!(
        extract_swings(&step_and_collect(&mut server, &[pair.bob], 1)[0]).is_empty(),
        "CMD-SWG-006: none animation starts cooldown blocking later punch"
    );

    queue_cmd(&mut server, pair.alice, "swing Bob offhand stab 2s");
    let _ = server.ticks_all(1);
    server
        .world()
        .get_mut::<PlayerGameMode>(pair.bob_entity)
        .unwrap()
        .0 = GameMode::Spectator;
    let packets = run_cmd(
        &mut server,
        pair.alice,
        "swing Bob mainhand whack 10",
        &both,
        1,
    );
    assert_chat_key(&packets[0], "commands.swing.success.single", "CMD-SWG-007");
    assert!(extract_swings(&packets[1]).is_empty(), "CMD-SWG-006");

    let _ = server.ticks_all(22);
    let unlocked = run_cmd(
        &mut server,
        pair.alice,
        "swing Bob mainhand whack 6",
        &both,
        2,
    );
    assert_eq!(extract_swings(&unlocked[1]).len(), 1);

    let _ = server.ticks_all(5);
    queue_cmd(&mut server, pair.alice, "swing Alice mainhand whack 6");
    let packets = run_cmd(
        &mut server,
        pair.alice,
        "swing Alice mainhand whack 6",
        &both,
        1,
    );
    let ok_count = translate_keys(&packets[0])
        .iter()
        .filter(|k| k.as_str() == "commands.swing.success.single")
        .count();
    assert_eq!(ok_count, 2, "CMD-SWG-DEV-001: both calls succeed");
    assert_eq!(
        (
            extract_swings(&packets[0]).len(),
            extract_swings(&packets[1]).len()
        ),
        (1, 1)
    );

    let _ = server.ticks_all(5);
    server
        .world()
        .entity_mut(pair.bob_entity)
        .insert(mcrs_minecraft_damage::Vitals {
            death_time: Some(20),
            ..mcrs_minecraft_damage::Vitals::new(0.0)
        });
    let packets = run_cmd(
        &mut server,
        pair.alice,
        "swing Bob mainhand whack 6",
        &both,
        2,
    );
    assert_chat_key(&packets[0], "commands.swing.success.single", "CMD-SWG-007");
    assert!(
        packets.iter().all(|p| extract_swings(p).is_empty()),
        "CMD-SWG-006: past corpse removal emits zero swings"
    );
}
