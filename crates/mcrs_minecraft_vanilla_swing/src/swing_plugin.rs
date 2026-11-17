use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageReader, MessageWriter};
use bevy_ecs::observer::On;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::Query;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::swing::{self, SwingPlayer, SwingSimulateSet};
use mcrs_minecraft_engine_item::{Effective, Selected, Slots};
use mcrs_minecraft_engine_world::session::Owner;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_item_stack::{AttackAnimation, PiercingWeapon, SwingAnimation};
use mcrs_minecraft_network::event::Received;
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{ServerboundPlayerAction, ServerboundPunch};
use mcrs_minecraft_swing::{LoadedClient, SpectatorRule, SwingDelivery, SwingHand};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwingPlugin;

impl Plugin for SwingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PendingSwingInput>()
            .add_message::<SwingPlayer>()
            .add_observer(on_punch)
            .add_observer(on_player_action)
            .add_systems(
                FixedUpdate,
                process_swing_inputs
                    .run_if(on_message::<PendingSwingInput>)
                    .in_set(SwingSimulateSet),
            );
    }
}

#[derive(Message, Clone, Copy, Debug)]
struct PendingSwingInput {
    player: Entity,
    kind: SwingInputKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwingInputKind {
    Punch,
    Stab,
}

fn on_punch(
    received: On<Received<ServerboundPunch>>,
    mut pending: MessageWriter<PendingSwingInput>,
) {
    if received.packet().is_none() {
        return;
    }
    pending.write(PendingSwingInput {
        player: received.entity,
        kind: SwingInputKind::Punch,
    });
}

fn on_player_action(
    received: On<Received<ServerboundPlayerAction>>,
    mut pending: MessageWriter<PendingSwingInput>,
) {
    let Some(packet) = received.packet() else {
        return;
    };
    if packet.action == PlayerAction::Stab {
        pending.write(PendingSwingInput {
            player: received.entity,
            kind: SwingInputKind::Stab,
        });
    }
}

#[expect(
    clippy::type_complexity,
    reason = "Bevy system queries for player swing state and held item components"
)]
fn process_swing_inputs(
    mut inputs: MessageReader<PendingSwingInput>,
    players: Query<
        (
            &LoadedClient,
            &PlayerGameMode,
            Option<&PlayerFileState>,
            Option<&Slots>,
            Option<&Selected>,
        ),
        With<Owner>,
    >,
    stacks: Query<(
        Option<Effective<AttackAnimation>>,
        Option<Effective<PiercingWeapon>>,
    )>,
    mut swings: MessageWriter<SwingPlayer>,
) {
    for input in inputs.read() {
        let Ok((loaded, mode, file_state, slots, selected)) = players.get(input.player) else {
            continue;
        };
        if mode.0 == GameMode::Spectator || swing::player_is_dead(file_state) {
            continue;
        }
        if matches!(input.kind, SwingInputKind::Punch) && !loaded.is_loaded() {
            continue;
        }
        let main_stack =
            slots.and_then(|slots| slots.get(usize::from(selected.map_or(0, |s| s.0))));
        let anim = match input.kind {
            SwingInputKind::Punch => main_stack
                .and_then(|stack| stacks.get(stack).ok())
                .and_then(|(attack, _)| attack.and_then(|e| e.get().map(|a| a.0))),
            SwingInputKind::Stab => {
                let Some(stack) = main_stack else {
                    continue;
                };
                let Ok((attack, piercing)) = stacks.get(stack) else {
                    continue;
                };
                if piercing.and_then(|e| e.get()).is_none() {
                    continue;
                }
                attack.and_then(|e| e.get().map(|a| a.0))
            }
        };
        swings.write(SwingPlayer {
            player: input.player,
            spec: SwingAnimation::spec_or_default(SwingHand::Main, anim),
            delivery: SwingDelivery::ObserversOnly,
            spectator: SpectatorRule::Block,
        });
    }
}
