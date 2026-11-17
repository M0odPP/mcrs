use bevy_ecs::message::Messages;
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::swing::{self, PlayerSwing, PlayerSwingPlugin, SwingPlayer};
use mcrs_minecraft_engine_item::{ItemCommandsExt, Selected, StackCommandsExt};
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_item_stack::{
    AttackAnimation, Count, InteractAnimation, ItemStack, SwingAnimation, SwingKind,
};
use mcrs_minecraft_player_holders::HandAction;
use mcrs_minecraft_swing::{SpectatorRule, SwingDelivery, SwingSpec};
use mcrs_minecraft_vanilla_inventory::PlayerInventoryCommandsExt;

use crate::fixture::{STONE, gameplay_app, put};

fn setup_player(app: &mut bevy_app::App, mode: GameMode) -> bevy_ecs::entity::Entity {
    if !app.is_plugin_added::<PlayerSwingPlugin>() {
        app.add_plugins(PlayerSwingPlugin);
    }
    let world = app.world_mut();
    world.insert_resource(BirthClock(10));
    world.init_resource::<Messages<OutboundPlayerPacket>>();
    let player = world
        .spawn((
            Player,
            Owner(PlayerSession(1)),
            PlayerGameMode(mode),
            PlayerSwing::default(),
            Selected(0),
        ))
        .id();
    world.flush();
    player
}

#[test]
fn hand_drop_triggers_default_main_whack_6_cooldown_and_ignores_empty_and_spectator() {
    let mut app = gameplay_app();
    let player = setup_player(&mut app, GameMode::Survival);

    app.world_mut()
        .commands()
        .entity(player)
        .hand_action(HandAction::DropOne);
    app.world_mut().flush();
    app.world_mut().run_schedule(bevy_app::FixedUpdate);
    assert!(
        swing::swing_player(
            app.world_mut(),
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            },
        ),
        "dropping with an empty hand must not start a swing"
    );

    app.world_mut().resource_mut::<BirthClock>().0 = 20;
    let stack = ItemStack::new(STONE, Count::new(2).unwrap())
        .set(AttackAnimation(SwingAnimation {
            kind: SwingKind::Stab,
            duration: 20,
        }))
        .set(InteractAnimation(SwingAnimation {
            kind: SwingKind::None,
            duration: 20,
        }));
    app.world_mut()
        .commands()
        .spawn_stack(stack)
        .put_at(player, 0);
    app.world_mut().flush();

    app.world_mut()
        .commands()
        .entity(player)
        .hand_action(HandAction::DropOne);
    app.world_mut().flush();
    app.world_mut().run_schedule(bevy_app::FixedUpdate);

    app.world_mut().resource_mut::<BirthClock>().0 = 23;
    assert!(
        !swing::swing_player(
            app.world_mut(),
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            },
        ),
        "drop swing (whack, duration 6) must still be in cooldown at T + 3"
    );
    app.world_mut().resource_mut::<BirthClock>().0 = 24;
    assert!(
        swing::swing_player(
            app.world_mut(),
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            },
        ),
        "drop swing (whack, duration 6) must unlock at T + 4 even with duration-20 components on held stack"
    );

    let spectator = setup_player(&mut app, GameMode::Spectator);
    put(app.world_mut(), spectator, 0, STONE, 1);
    app.world_mut().resource_mut::<BirthClock>().0 = 40;
    app.world_mut()
        .commands()
        .entity(spectator)
        .hand_action(HandAction::DropOne);
    app.world_mut().flush();
    app.world_mut().run_schedule(bevy_app::FixedUpdate);
    assert_eq!(
        app.world().get::<PlayerSwing>(spectator),
        Some(&PlayerSwing::default()),
        "spectator drop must not trigger a swing"
    );
}
