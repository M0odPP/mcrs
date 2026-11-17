use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageReader, MessageWriter};
use bevy_ecs::observer::On;
use bevy_ecs::prelude::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy_ecs::system::{Query, Res, ResMut};
use bevy_ecs::world::World;
use mcrs_engine_muxer::EntityEvent;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_network::event::Received;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::clientbound::ClientboundSwingAnimation;
use mcrs_minecraft_protocol_game::serverbound::ServerboundPlayerLoaded;
use mcrs_minecraft_protocol_game::{Hand, SwingAnimationKind};
use mcrs_minecraft_replication::PlayerData;
use mcrs_minecraft_swing::{
    EffectStep, LoadedClient, SpectatorRule, SwingDelivery, SwingHand, SwingKind, SwingModifiers,
    SwingSpec, SwingTracker,
};

use crate::bus::{self, OutboundPlayerPacket, PacketPayload};
use crate::network_id;
use crate::player_state::{Effect, EffectData, PlayerFileState};
use crate::replicated::Replicated;
use crate::send_own_attributes::OwnAttributesSet;
use crate::tile_events::TileEvents;

const PLAYER_CORPSE_TICKS: i16 = 20;

pub struct PlayerSwingPlugin;

impl Plugin for PlayerSwingPlugin {
    fn build(&self, app: &mut App) {
        if let Err(error) = app.try_register_required_components::<Player, LoadedClient>() {
            tracing::error!("a player does not require its loaded client state: {error}");
        }
        if let Err(error) = app.try_register_required_components::<Player, PlayerSwing>() {
            tracing::error!("a player does not require its swing state: {error}");
        }
        app.add_message::<SwingPlayer>();
        app.init_resource::<PendingPlayerSwings>();
        app.add_observer(on_player_loaded);
        app.configure_sets(
            FixedUpdate,
            (
                (TickSet::ClientInput, TickSet::Simulate, TickSet::Project).chain(),
                SwingSimulateSet
                    .in_set(TickSet::Simulate)
                    .after(tick_loaded_clients),
            ),
        );
        app.add_systems(
            FixedUpdate,
            (
                tick_loaded_clients.in_set(TickSet::Simulate),
                (
                    apply_player_swings.run_if(on_message::<SwingPlayer>),
                    tick_player_effects_system,
                )
                    .chain()
                    .in_set(TickSet::Project)
                    .after(OwnAttributesSet),
            ),
        );
    }
}

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SwingSimulateSet;

#[derive(
    Component, Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
pub struct PlayerSwing(pub SwingTracker);

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwingPlayer {
    pub player: Entity,
    pub spec: SwingSpec,
    pub delivery: SwingDelivery,
    pub spectator: SpectatorRule,
}

#[derive(Clone, Copy, Debug)]
struct PendingSwing {
    player: Entity,
    hand: Hand,
    animation: SwingAnimationKind,
    duration: i32,
}

#[derive(Resource, Default)]
pub(crate) struct PendingPlayerSwings {
    swings: Vec<PendingSwing>,
}

pub(crate) fn has_pending_player_swings(pending: Res<PendingPlayerSwings>) -> bool {
    !pending.swings.is_empty()
}

#[must_use]
pub const fn to_protocol_hand(hand: SwingHand) -> Hand {
    match hand {
        SwingHand::Main => Hand::Main,
        SwingHand::Off => Hand::Off,
    }
}

#[must_use]
pub const fn to_protocol_kind(kind: SwingKind) -> SwingAnimationKind {
    match kind {
        SwingKind::None => SwingAnimationKind::None,
        SwingKind::Whack => SwingAnimationKind::Whack,
        SwingKind::Stab => SwingAnimationKind::Stab,
    }
}

#[must_use]
pub fn player_is_dead(state: Option<&PlayerFileState>) -> bool {
    state.is_some_and(|file| file.health <= 0.0 || file.death_time > 0)
}

#[must_use]
pub fn swing_modifiers_for_player(state: Option<&PlayerFileState>) -> SwingModifiers {
    let Some(state) = state else {
        return SwingModifiers::default();
    };
    SwingModifiers::from_effects(
        state
            .active_effects
            .iter()
            .filter(|effect| effect.duration == -1 || effect.duration > 0)
            .map(|effect| (effect.id.as_str(), effect.amplifier)),
    )
}

fn has_decaying_hidden(mut cursor: Option<&EffectData>) -> bool {
    while let Some(node) = cursor {
        if node.duration != -1 {
            return true;
        }
        cursor = node.hidden_effect.as_deref();
    }
    false
}

fn has_decaying_effects(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect| effect.duration != -1 || has_decaying_hidden(effect.hidden_effect.as_deref()))
}

fn step_player_hidden_data(mut cursor: &mut Option<Box<EffectData>>) {
    while let Some(node) = cursor.as_deref_mut() {
        if node.duration != -1 {
            if node.duration <= 0 {
                *cursor = node.hidden_effect.take();
                continue;
            }
            node.duration -= 1;
            if node.duration <= 0 {
                *cursor = node.hidden_effect.take();
                continue;
            }
        }
        let Some(node) = cursor.as_deref_mut() else {
            break;
        };
        cursor = &mut node.hidden_effect;
    }
}

pub fn tick_player_effects(effects: &mut Vec<Effect>) {
    let mut index = 0;
    while let Some(effect) = effects.get_mut(index) {
        match EffectStep::advance(&mut effect.duration) {
            EffectStep::Inactive => {
                effects.swap_remove(index);
            }
            EffectStep::Active => {
                step_player_hidden_data(&mut effect.hidden_effect);
                index += 1;
            }
            EffectStep::Expired => {
                step_player_hidden_data(&mut effect.hidden_effect);
                if let Some(hidden) = effect.hidden_effect.take() {
                    effect.amplifier = hidden.amplifier;
                    effect.duration = hidden.duration;
                    effect.ambient = hidden.ambient;
                    effect.show_particles = hidden.show_particles;
                    effect.show_icon = hidden.show_icon;
                    effect.hidden_effect = hidden.hidden_effect;
                    index += 1;
                } else {
                    effects.swap_remove(index);
                }
            }
        }
    }
}

fn on_player_loaded(
    received: On<Received<ServerboundPlayerLoaded>>,
    mut players: Query<&mut LoadedClient, With<Owner>>,
) {
    if let Ok(mut loaded) = players.get_mut(received.entity)
        && !loaded.is_loaded()
    {
        loaded.mark_loaded();
    }
}

fn tick_loaded_clients(mut players: Query<&mut LoadedClient, With<Owner>>) {
    for mut loaded in &mut players {
        if !loaded.is_loaded() {
            loaded.tick();
        }
    }
}

fn tick_player_effects_system(mut players: Query<&mut PlayerFileState, With<Owner>>) {
    for mut file_state in &mut players {
        if has_decaying_effects(&file_state.active_effects) {
            tick_player_effects(&mut file_state.active_effects);
        }
    }
}

fn start_swing_record(
    request: SwingPlayer,
    now: u64,
    session: PlayerSession,
    mode: Option<&PlayerGameMode>,
    file_state: Option<&PlayerFileState>,
    swing: &mut PlayerSwing,
) -> Option<(Option<OutboundPlayerPacket>, PendingSwing)> {
    if file_state.is_some_and(|state| state.death_time >= PLAYER_CORPSE_TICKS)
        || (request.spectator == SpectatorRule::Block
            && (mode.is_some_and(|m| m.0 == GameMode::Spectator) || player_is_dead(file_state)))
    {
        return None;
    }
    let modifiers = swing_modifiers_for_player(file_state);
    let base_duration = request.spec.duration.max(0);
    swing.0.try_start(now, base_duration, modifiers)?;
    let hand = to_protocol_hand(request.spec.hand);
    let animation = to_protocol_kind(request.spec.kind);
    let self_packet = (request.delivery == SwingDelivery::SelfAndObservers).then(|| {
        bus::to(
            session,
            PacketPayload::SwingAnimation(ClientboundSwingAnimation {
                entity_id: VarInt(network_id::player_network_id(session)),
                hand,
                animation,
                duration: VarInt(base_duration),
            }),
        )
    });
    Some((
        self_packet,
        PendingSwing {
            player: request.player,
            hand,
            animation,
            duration: base_duration,
        },
    ))
}

fn apply_player_swings(
    mut requests: MessageReader<SwingPlayer>,
    clock: Option<Res<BirthClock>>,
    mut players: Query<(
        &Owner,
        Option<&PlayerGameMode>,
        Option<&PlayerFileState>,
        &mut PlayerSwing,
    )>,
    mut outbound: MessageWriter<OutboundPlayerPacket>,
    mut pending: ResMut<PendingPlayerSwings>,
) {
    let now = clock.map_or(0, |clock| clock.0);
    for &request in requests.read() {
        let Ok((&Owner(session), mode, file_state, mut swing)) = players.get_mut(request.player)
        else {
            continue;
        };
        let Some((self_packet, queued)) =
            start_swing_record(request, now, session, mode, file_state, &mut swing)
        else {
            continue;
        };
        if let Some(packet) = self_packet {
            outbound.write(packet);
        }
        pending.swings.push(queued);
    }
}

pub fn swing_player(world: &mut World, request: SwingPlayer) -> bool {
    let now = world
        .get_resource::<BirthClock>()
        .map_or(0, |clock| clock.0);
    let mut query = world.query::<(
        &Owner,
        Option<&PlayerGameMode>,
        Option<&PlayerFileState>,
        &mut PlayerSwing,
    )>();
    let Some((self_packet, queued)) = query.get_mut(world, request.player).ok().and_then(
        |(&Owner(session), mode, file_state, mut swing)| {
            start_swing_record(request, now, session, mode, file_state, &mut swing)
        },
    ) else {
        return false;
    };
    if let Some(packet) = self_packet {
        world.write_message(packet);
    }
    world
        .get_resource_or_init::<PendingPlayerSwings>()
        .swings
        .push(queued);
    true
}

pub(crate) fn publish_player_swings(
    mut pending: ResMut<PendingPlayerSwings>,
    players: Query<&Replicated>,
    mut events: ResMut<TileEvents>,
) {
    for swing in pending.swings.drain(..) {
        if let Ok(replicated) = players.get(swing.player) {
            events.push(
                replicated.tile,
                replicated.id,
                EntityEvent::Data(
                    PlayerData::Swing {
                        hand: swing.hand,
                        animation: swing.animation,
                        duration: swing.duration,
                    }
                    .into(),
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::message::Messages;
    use bevy_ecs::system::RunSystemOnce;
    use mcrs_engine_muxer::{EntityId, TileKey};
    use mcrs_minecraft_replication::ReplicationData;

    fn test_world() -> (World, Entity) {
        let mut world = World::new();
        world.insert_resource(BirthClock(10));
        world.init_resource::<PendingPlayerSwings>();
        world.init_resource::<TileEvents>();
        world.init_resource::<Messages<OutboundPlayerPacket>>();
        let player = world
            .spawn((
                Owner(PlayerSession(7)),
                PlayerGameMode(GameMode::Survival),
                PlayerSwing::default(),
                LoadedClient::default(),
                PlayerFileState::default(),
                Replicated {
                    id: EntityId(99),
                    tile: TileKey::new(0, 0, 0),
                    game_mode: GameMode::Survival,
                },
            ))
            .id();
        (world, player)
    }

    #[test]
    fn swing_player_dedups_same_tick_and_respects_haste_fatigue_unscaled_wire() {
        let (mut world, player) = test_world();
        let spec = SwingSpec {
            hand: SwingHand::Off,
            kind: SwingKind::Stab,
            duration: 10,
        };
        assert!(swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec,
                delivery: SwingDelivery::SelfAndObservers,
                spectator: SpectatorRule::Block,
            }
        ));
        assert!(!swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            }
        ));

        let messages = world.resource::<Messages<OutboundPlayerPacket>>();
        let mut reader = messages.get_cursor();
        let pkts: Vec<_> = reader.read(messages).collect();
        assert_eq!(pkts.len(), 1);
        match &pkts[0].data {
            PacketPayload::SwingAnimation(pkt) => {
                assert_eq!(pkt.hand, Hand::Off);
                assert_eq!(pkt.animation, SwingAnimationKind::Stab);
                assert_eq!(pkt.duration, VarInt(10));
            }
            other => panic!("unexpected packet: {other:?}"),
        }

        world.run_system_once(publish_player_swings).unwrap();
        let deltas = world.resource_mut::<TileEvents>().take();
        assert_eq!(deltas.tiles.len(), 1);
        assert_eq!(deltas.tiles[0].events.len(), 1);
        match &deltas.tiles[0].events[0].event {
            EntityEvent::Data(ReplicationData::Player(PlayerData::Swing {
                hand,
                animation,
                duration,
            })) => {
                assert_eq!(*hand, Hand::Off);
                assert_eq!(*animation, SwingAnimationKind::Stab);
                assert_eq!(*duration, 10);
            }
            other => panic!("unexpected entity event: {other:?}"),
        }

        world
            .get_mut::<PlayerFileState>(player)
            .unwrap()
            .active_effects
            .push(Effect {
                id: "minecraft:mining_fatigue".to_owned(),
                amplifier: 0,
                duration: 20,
                ambient: false,
                show_particles: false,
                show_icon: Some(false),
                hidden_effect: None,
            });
        world.resource_mut::<BirthClock>().0 = 15;
        assert!(!swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            }
        ));
        world.resource_mut::<BirthClock>().0 = 16;
        assert!(swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            }
        ));
        world.resource_mut::<BirthClock>().0 = 17;
        assert!(!swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            }
        ));
    }

    #[test]
    fn command_swing_skips_the_removed_corpse() {
        let (mut world, player) = test_world();
        let request = SwingPlayer {
            player,
            spec: SwingSpec::DEFAULT_MAIN,
            delivery: SwingDelivery::SelfAndObservers,
            spectator: SpectatorRule::Allow,
        };
        let mut state = world.get_mut::<PlayerFileState>(player).unwrap();
        state.health = 0.0;
        state.death_time = PLAYER_CORPSE_TICKS - 1;
        assert!(
            swing_player(&mut world, request),
            "a command can swing a tracked corpse"
        );

        world.resource_mut::<BirthClock>().0 += 10;
        world.get_mut::<PlayerFileState>(player).unwrap().death_time = PLAYER_CORPSE_TICKS;
        assert!(
            !swing_player(&mut world, request),
            "an online player past corpse removal must send no new swing packets"
        );
        assert_eq!(world.resource::<PendingPlayerSwings>().swings.len(), 1);
    }

    #[test]
    fn swing_player_checks_spectator_and_dead_state() {
        let (mut world, player) = test_world();
        world.get_mut::<PlayerGameMode>(player).unwrap().0 = GameMode::Spectator;
        assert!(!swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            }
        ));
        assert!(swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::SelfAndObservers,
                spectator: SpectatorRule::Allow,
            }
        ));

        world.resource_mut::<BirthClock>().0 = 20;
        world.get_mut::<PlayerGameMode>(player).unwrap().0 = GameMode::Survival;
        world.get_mut::<PlayerFileState>(player).unwrap().health = 0.0;
        assert!(!swing_player(
            &mut world,
            SwingPlayer {
                player,
                spec: SwingSpec::DEFAULT_MAIN,
                delivery: SwingDelivery::ObserversOnly,
                spectator: SpectatorRule::Block,
            }
        ));
    }

    #[test]
    fn player_effect_decay_promotes_hidden_effect_and_skips_infinite() {
        let mut effects = vec![
            Effect {
                id: "minecraft:haste".to_owned(),
                amplifier: 1,
                duration: 1,
                ambient: false,
                show_particles: true,
                show_icon: Some(true),
                hidden_effect: Some(Box::new(EffectData {
                    amplifier: 0,
                    duration: 5,
                    ambient: true,
                    show_particles: false,
                    show_icon: Some(false),
                    hidden_effect: None,
                })),
            },
            Effect {
                id: "minecraft:conduit_power".to_owned(),
                amplifier: 0,
                duration: -1,
                ambient: false,
                show_particles: true,
                show_icon: Some(true),
                hidden_effect: None,
            },
        ];
        assert!(has_decaying_effects(&effects));
        tick_player_effects(&mut effects);
        assert_eq!(effects.len(), 2);
        let haste = effects.iter().find(|e| e.id == "minecraft:haste").unwrap();
        assert_eq!(haste.amplifier, 0);
        assert_eq!(haste.duration, 4);
        assert!(haste.ambient);
        assert_eq!(
            effects
                .iter()
                .find(|e| e.id == "minecraft:conduit_power")
                .unwrap()
                .duration,
            -1
        );

        effects.retain(|e| e.id == "minecraft:conduit_power");
        assert!(!has_decaying_effects(&effects));
    }
}
