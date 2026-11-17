#![expect(clippy::unwrap_used, reason = "a test reports a failure by panicking")]

use bevy_app::{App, FixedUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::world::World;
use bytes::Bytes;
use mcrs_minecraft_core::Direction;
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::swing::{self, PlayerSwing, PlayerSwingPlugin, SwingPlayer};
use mcrs_minecraft_engine_item::{ItemCommandsExt, ItemPlugin, Selected, Slots, StackCommandsExt};
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_item_stack::{
    AttackAnimation, Count, ItemStack, Kind, SwingAnimation, SwingKind,
};
use mcrs_minecraft_item_table::{DefinitionFile, ItemTable, bake};
use mcrs_minecraft_network::event::Received;
use mcrs_minecraft_protocol::{Encode, VarInt};
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{ServerboundPlayerAction, ServerboundPunch};
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_swing::{LoadedClient, SpectatorRule, SwingDelivery, SwingSpec};
use mcrs_minecraft_vanilla_swing::SwingPlugin;
use mcrs_voxel::BlockPos;

const STICK: Kind = Kind(1);
const SPEAR: Kind = Kind(2);

fn test_app() -> App {
    let mut app = App::new();
    let files = [
        r#"{"minecraft:item":{"description":{"identifier":"minecraft:air","protocol_id":0},"components":{}}}"#,
        r#"{"minecraft:item":{"description":{"identifier":"minecraft:stick","protocol_id":1},"components":{"minecraft:max_stack_size":64}}}"#,
        r#"{"minecraft:item":{"description":{"identifier":"minecraft:spear","protocol_id":2},"components":{"minecraft:max_stack_size":1,"minecraft:piercing_weapon":{},"minecraft:attack_animation":{"type":"stab","duration":12}}}}"#,
    ];
    let table = bake(
        files.iter().map(|s| DefinitionFile {
            name: "test",
            bytes: s.as_bytes(),
        }),
        &ItemTable::leak([]),
    )
    .unwrap()
    .table;
    let world = app.world_mut();
    world.insert_resource(CurrentRegistries::new(
        mcrs_minecraft_game_registries::registry_load::test_registries().clone(),
    ));
    world.insert_resource(table);
    world.insert_resource(BirthClock(100));
    world.init_resource::<Messages<OutboundPlayerPacket>>();
    app.add_plugins((ItemPlugin::default(), PlayerSwingPlugin, SwingPlugin));
    app
}

fn spawn_player(world: &mut World, loaded: bool, mode: GameMode) -> Entity {
    let mut client = LoadedClient::default();
    if loaded {
        client.mark_loaded();
    }
    world
        .spawn((
            Owner(PlayerSession(1)),
            PlayerGameMode(mode),
            PlayerSwing::default(),
            PlayerFileState::default(),
            client,
            Slots::new(41),
            Selected(0),
        ))
        .id()
}

fn punch(world: &mut World, player: Entity) {
    let mut raw = Vec::new();
    ServerboundPunch.encode(&mut raw).unwrap();
    world.trigger(Received::<ServerboundPunch>::new(player, Bytes::from(raw)));
}

fn player_action(world: &mut World, player: Entity, action: PlayerAction) {
    let mut raw = Vec::new();
    ServerboundPlayerAction {
        action,
        pos: BlockPos::new(0, 64, 0),
        direction: Direction::Up,
        sequence: VarInt(0),
    }
    .encode(&mut raw)
    .unwrap();
    world.trigger(Received::<ServerboundPlayerAction>::new(
        player,
        Bytes::from(raw),
    ));
}

#[test]
fn punch_requires_loaded_alive_non_spectator_and_reads_attack_animation() {
    let mut app = test_app();
    let unloaded = spawn_player(app.world_mut(), false, GameMode::Survival);
    punch(app.world_mut(), unloaded);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(
        app.world().get::<PlayerSwing>(unloaded),
        Some(&PlayerSwing::default()),
        "SWG-SRC-002: unloaded client punch is ignored"
    );

    let spectator = spawn_player(app.world_mut(), true, GameMode::Spectator);
    punch(app.world_mut(), spectator);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(
        app.world().get::<PlayerSwing>(spectator),
        Some(&PlayerSwing::default()),
        "SWG-SRC-002: spectator punch is ignored"
    );

    let dead = spawn_player(app.world_mut(), true, GameMode::Survival);
    app.world_mut()
        .get_mut::<PlayerFileState>(dead)
        .unwrap()
        .health = 0.0;
    punch(app.world_mut(), dead);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(
        app.world().get::<PlayerSwing>(dead),
        Some(&PlayerSwing::default()),
        "SWG-SRC-002: dead player punch is ignored"
    );

    let loaded = spawn_player(app.world_mut(), true, GameMode::Survival);
    let custom_stick =
        ItemStack::new(STICK, Count::new(1).unwrap()).set(AttackAnimation(SwingAnimation {
            kind: SwingKind::None,
            duration: 14,
        }));
    app.world_mut()
        .commands()
        .spawn_stack(custom_stick)
        .put_at(loaded, 0);
    app.world_mut().flush();

    app.world_mut().resource_mut::<BirthClock>().0 = 200;
    punch(app.world_mut(), loaded);
    app.world_mut().run_schedule(FixedUpdate);

    app.world_mut().resource_mut::<BirthClock>().0 = 207;
    assert!(!swing::swing_player(
        app.world_mut(),
        SwingPlayer {
            player: loaded,
            spec: SwingSpec::DEFAULT_MAIN,
            delivery: SwingDelivery::ObserversOnly,
            spectator: SpectatorRule::Block,
        }
    ));
    app.world_mut().resource_mut::<BirthClock>().0 = 208;
    assert!(swing::swing_player(
        app.world_mut(),
        SwingPlayer {
            player: loaded,
            spec: SwingSpec::DEFAULT_MAIN,
            delivery: SwingDelivery::ObserversOnly,
            spectator: SpectatorRule::Block,
        }
    ));
}

#[test]
fn stab_requires_piercing_weapon_and_ignores_non_stab_actions() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut(), true, GameMode::Survival);
    app.world_mut()
        .commands()
        .spawn_stack(ItemStack::new(STICK, Count::new(1).unwrap()))
        .put_at(player, 0);
    app.world_mut()
        .commands()
        .spawn_stack(ItemStack::new(SPEAR, Count::new(1).unwrap()))
        .put_at(player, 1);
    app.world_mut().flush();

    player_action(app.world_mut(), player, PlayerAction::StartDestroyBlock);
    player_action(app.world_mut(), player, PlayerAction::Stab);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(
        app.world().get::<PlayerSwing>(player),
        Some(&PlayerSwing::default())
    );

    app.world_mut().get_mut::<Selected>(player).unwrap().0 = 1;
    app.world_mut().resource_mut::<BirthClock>().0 = 300;
    player_action(app.world_mut(), player, PlayerAction::Stab);
    app.world_mut().run_schedule(FixedUpdate);
    app.world_mut().resource_mut::<BirthClock>().0 = 306;
    assert!(!swing::swing_player(
        app.world_mut(),
        SwingPlayer {
            player,
            spec: SwingSpec::DEFAULT_MAIN,
            delivery: SwingDelivery::ObserversOnly,
            spectator: SpectatorRule::Block,
        }
    ));
    app.world_mut().resource_mut::<BirthClock>().0 = 307;
    assert!(swing::swing_player(
        app.world_mut(),
        SwingPlayer {
            player,
            spec: SwingSpec::DEFAULT_MAIN,
            delivery: SwingDelivery::ObserversOnly,
            spectator: SpectatorRule::Block,
        }
    ));
}
