use std::mem::{align_of, size_of};

use crate::ability::PlayerFlying;
use crate::bus::{OutboundPlayerPacket, PacketPayload, to};
use crate::client_on_ground::ClientOnGround;
use crate::coordinator::clock::GameTime;
use crate::player_sprinting::PlayerSprinting;
use crate::teleport::{
    AcceptOutcome, MAX_XZ, MAX_Y, TeleportState, landing, position_packet, teleport_to,
};
use crate::using_item::UsingItem;
use bevy_app::{FixedPostUpdate, FixedUpdate, Plugin};
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::prelude::{
    Changed, Component, DetectChangesMut, Entity, Message, MessageReader, MessageWriter, Mut, On,
    Populated, Query, Res, SystemSet, With, Without, on_message,
};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::any_component_removed;
use bevy_math::DVec3;
use mcrs_minecraft_collision_table::BlockWorld;
use mcrs_minecraft_engine_physics::PhysicsTables;
use mcrs_minecraft_engine_world::entity::physics::{Rotation, Transform};
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::Owner;
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_engine_world::voxel_update::VoxelUpdateSet;
use mcrs_minecraft_engine_world::world::dimension::InDimension;
use mcrs_minecraft_engine_world::world::storage::view::{DimensionView, WorldView};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_move_predict::{Change, ChangeStates, ClientTick, Limits, Outcome, Prediction};
use mcrs_minecraft_move_validate::Move;
use mcrs_minecraft_network::event::Received;
use mcrs_minecraft_physics::{
    ItemUse, Keys, MoveFlags, NoBodies, Orientation, PlayerFacts, PlayerInput, PlayerState,
    Relative, Teleport, View,
};
use mcrs_minecraft_protocol_game::serverbound::{
    ServerboundAcceptTeleportation, ServerboundMovePlayerPos, ServerboundMovePlayerPosRot,
    ServerboundMovePlayerRot, ServerboundMovePlayerStatusOnly, ServerboundMoveVehicle,
    ServerboundPlayerInput,
};
use mcrs_minecraft_text::Text;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemUsePredictionSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroundContact {
    Airborne,
    Grounded,
}

impl GroundContact {
    const fn from_on_ground(on_ground: bool) -> Self {
        if on_ground {
            Self::Grounded
        } else {
            Self::Airborne
        }
    }

    const fn is_grounded(self) -> bool {
        matches!(self, Self::Grounded)
    }
}

#[derive(Component, Debug)]
pub struct MovementPrediction {
    prediction: Prediction,
    input: PlayerInput,
    last_item_use: ItemUse,
    changes_count: u64,
    input_driven: bool,
    synced: bool,
    last_outcome: Option<Outcome>,
}

const _: () = assert!(
    size_of::<MovementPrediction>()
        >= size_of::<Prediction>() + size_of::<PlayerInput>() + size_of::<Option<Outcome>>() + 16,
    "MovementPrediction stores prediction state, input, item use, changes counter, and outcome"
);
const _: () = assert!(
    align_of::<MovementPrediction>() == align_of::<Prediction>(),
    "MovementPrediction matches Prediction alignment"
);

impl Default for MovementPrediction {
    fn default() -> Self {
        Self {
            prediction: Prediction::new(PlayerState::new(DVec3::ZERO), Limits::default()),
            input: PlayerInput::default(),
            last_item_use: ItemUse::default(),
            changes_count: 0,
            input_driven: false,
            synced: false,
            last_outcome: None,
        }
    }
}

impl MovementPrediction {
    #[must_use]
    pub fn prediction(&self) -> &Prediction {
        &self.prediction
    }

    #[must_use]
    pub fn last_outcome(&self) -> Option<Outcome> {
        self.last_outcome
    }

    #[must_use]
    pub fn is_using_item(&self) -> bool {
        self.prediction.accepted().item_use.using
    }

    #[must_use]
    pub fn item_use(&self) -> ItemUse {
        self.prediction.accepted().item_use
    }

    pub(crate) fn sync_item_use(&mut self, using: Option<&UsingItem>) {
        let next = using.map_or_else(ItemUse::default, |active| active.item_use());
        if next != self.last_item_use {
            self.last_item_use = next;
            self.prediction.change(Change::ItemUse(next));
            self.changes_count = self.changes_count.saturating_add(1);
        }
    }

    pub(crate) fn sync_state(
        &mut self,
        position: DVec3,
        ground: GroundContact,
        rotation: Rotation,
    ) {
        let mut state = PlayerState::new(position);
        state.body.flags = MoveFlags {
            on_ground: ground.is_grounded(),
            ..MoveFlags::default()
        };
        state.item_use = self.last_item_use;
        self.input.rotation = orientation_of(rotation);
        self.prediction = Prediction::new(state, Limits::default());
        self.changes_count = 0;
        self.synced = true;
    }

    pub(crate) fn accept_teleport(&mut self, position: DVec3) {
        if self.synced {
            self.last_outcome = Some(self.prediction.accept(position));
        } else {
            self.sync_state(position, GroundContact::Airborne, Rotation::default());
        }
    }
}

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_observer(handle_move_pos);
        app.add_observer(handle_move_pos_rot);
        app.add_observer(handle_move_rot);
        app.add_observer(handle_move_status_only);
        app.add_observer(handle_move_vehicle);
        app.add_observer(handle_accept_teleportation);
        app.add_observer(handle_prediction_input);
        app.add_message::<PlayerMovement>();
        app.add_message::<PlayerMoved>();
        if let Err(error) = app.try_register_required_components::<Player, ClientOnGround>() {
            tracing::error!("a player does not require its on-ground flag: {error}");
        }
        if let Err(error) = app.try_register_required_components::<Player, MovementPrediction>() {
            tracing::error!("a player does not require its movement prediction: {error}");
        }
        app.add_systems(
            FixedUpdate,
            (
                process_movement
                    .run_if(on_message::<PlayerMovement>)
                    .in_set(TickSet::ClientInput),
                (
                    sync_changed_using_item,
                    sync_removed_using_item.run_if(any_component_removed::<UsingItem>),
                )
                    .in_set(ItemUsePredictionSet),
            ),
        );
        app.add_systems(
            FixedPostUpdate,
            teleport.before(VoxelUpdateSet::NetworkSync),
        );
    }
}

fn handle_prediction_input(
    on: On<Received<ServerboundPlayerInput>>,
    mut players: Query<&mut MovementPrediction, With<Owner>>,
) {
    let Some(packet) = on.packet() else {
        return;
    };
    let Ok(mut prediction) = players.get_mut(on.entity) else {
        return;
    };
    let keys = Keys {
        forward: packet.input.forward(),
        back: packet.input.backward(),
        left: packet.input.left(),
        right: packet.input.right(),
        jump: packet.input.jump(),
        shift: packet.input.shift(),
        sprint: packet.input.sprint(),
        auto_jump: false,
    };
    if prediction.input.keys != keys || !prediction.input_driven {
        prediction.input.keys = keys;
        prediction.input_driven = true;
    }
}

fn sync_changed_using_item(
    mut changed: Populated<(&UsingItem, &mut MovementPrediction), Changed<UsingItem>>,
) {
    for (using, mut prediction) in &mut changed {
        if using.item_use() != prediction.last_item_use {
            prediction.sync_item_use(Some(using));
        }
    }
}

fn sync_removed_using_item(
    mut removed: RemovedComponents<UsingItem>,
    mut without: Query<&mut MovementPrediction, Without<UsingItem>>,
) {
    for entity in removed.read() {
        if let Ok(mut prediction) = without.get_mut(entity)
            && prediction.last_item_use != ItemUse::default()
        {
            prediction.sync_item_use(None);
        }
    }
}

fn handle_move_pos(
    on: On<Received<ServerboundMovePlayerPos>>,
    mut writer: MessageWriter<PlayerMovement>,
) {
    if let Some(p) = on.packet() {
        writer.write(PlayerMovement {
            entity: on.entity,
            position: Some(p.position.into()),
            look: None,
            on_ground: Some(ClientOnGround(p.flags.on_ground())),
            horizontal_collision: p.flags.horizontal_collision(),
        });
    }
}

fn handle_move_pos_rot(
    on: On<Received<ServerboundMovePlayerPosRot>>,
    mut writer: MessageWriter<PlayerMovement>,
) {
    if let Some(p) = on.packet() {
        writer.write(PlayerMovement {
            entity: on.entity,
            position: Some(p.position.into()),
            look: Some(Rotation::new(p.look.yaw, p.look.pitch)),
            on_ground: Some(ClientOnGround(p.flags.on_ground())),
            horizontal_collision: p.flags.horizontal_collision(),
        });
    }
}

fn handle_move_rot(
    on: On<Received<ServerboundMovePlayerRot>>,
    mut writer: MessageWriter<PlayerMovement>,
) {
    if let Some(p) = on.packet() {
        writer.write(PlayerMovement {
            entity: on.entity,
            position: None,
            look: Some(Rotation::new(p.look.yaw, p.look.pitch)),
            on_ground: Some(ClientOnGround(p.flags.on_ground())),
            horizontal_collision: p.flags.horizontal_collision(),
        });
    }
}

fn handle_move_status_only(
    on: On<Received<ServerboundMovePlayerStatusOnly>>,
    mut writer: MessageWriter<PlayerMovement>,
) {
    if let Some(p) = on.packet() {
        writer.write(PlayerMovement {
            entity: on.entity,
            position: None,
            look: None,
            on_ground: Some(ClientOnGround(p.flags.on_ground())),
            horizontal_collision: p.flags.horizontal_collision(),
        });
    }
}

fn handle_move_vehicle(
    on: On<Received<ServerboundMoveVehicle>>,
    mut writer: MessageWriter<PlayerMovement>,
) {
    if on.packet().is_some() {
        writer.write(PlayerMovement {
            entity: on.entity,
            position: None,
            look: None,
            on_ground: None,
            horizontal_collision: false,
        });
    }
}

#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlayerMoved {
    pub entity: Entity,
    pub delta: DVec3,
    pub was_on_ground: bool,
    pub now_on_ground: bool,
}

#[derive(Message)]
pub struct PlayerMovement {
    entity: Entity,
    position: Option<DVec3>,
    look: Option<Rotation>,
    on_ground: Option<ClientOnGround>,
    horizontal_collision: bool,
}

type MovementPlayerRow<'a> = (
    &'a Owner,
    Mut<'a, TeleportState>,
    Mut<'a, Transform>,
    Mut<'a, ClientOnGround>,
    Option<Mut<'a, MovementPrediction>>,
    (
        Option<&'a UsingItem>,
        Option<&'a PlayerFlying>,
        Option<&'a PlayerGameMode>,
        Option<&'a PlayerSprinting>,
        Option<&'a InDimension>,
    ),
);

pub(crate) fn process_movement(
    mut reader: MessageReader<PlayerMovement>,
    mut query: Query<MovementPlayerRow<'_>>,
    world: WorldView,
    tables: Option<Res<PhysicsTables>>,
    time: Res<GameTime>,
    mut packets: MessageWriter<OutboundPlayerPacket>,
    mut moved: MessageWriter<PlayerMoved>,
) {
    for movement in reader.read() {
        let Ok((
            owner,
            mut state,
            mut transform,
            mut on_ground,
            mut prediction,
            (using, flying, game_mode, sprinting, dimension),
        )) = query.get_mut(movement.entity)
        else {
            continue;
        };
        if state.pending().is_some() {
            if let Some(destination) = state.resend_due(*time) {
                let id = state.sent(destination, *time);
                packets.write(resend(owner, id, destination, transform.rotation));
            }
            continue;
        }
        if let Some(prediction) = prediction.as_deref_mut()
            && let Some(correction) = step_prediction(
                prediction,
                movement,
                transform.as_ref(),
                on_ground.0,
                PredictionPlayerContext {
                    using,
                    flying,
                    game_mode,
                    sprinting,
                },
                dimension.and_then(|dimension| {
                    tables
                        .as_deref()
                        .zip(world.get(dimension.0))
                        .map(|(tables, blocks)| PredictionWorld { tables, blocks })
                }),
            )
        {
            let destination =
                Transform::from_translation(correction.position).with_rotation(transform.rotation);
            let id = state.sent(destination, *time);
            packets.write(position(owner, id, &correction));
            continue;
        }
        let previous_position = transform.translation;
        let was_on_ground = on_ground.0;
        if let Some(position) = movement.position {
            transform.set_if_neq(transform.with_translation(reachable(position)));
        }
        if let Some(look) = movement.look {
            transform.set_if_neq(transform.with_rotation(look));
        }
        if let Some(reported) = movement.on_ground {
            on_ground.set_if_neq(reported);
        }
        state.sync(*transform);
        if let Some(prediction) = prediction.as_deref_mut()
            && !prediction.input_driven
        {
            prediction.sync_state(
                transform.translation,
                GroundContact::from_on_ground(on_ground.0),
                transform.rotation,
            );
        }
        moved.write(PlayerMoved {
            entity: movement.entity,
            delta: transform.translation - previous_position,
            was_on_ground,
            now_on_ground: on_ground.0,
        });
    }
}

struct PredictionPlayerContext<'a> {
    using: Option<&'a UsingItem>,
    flying: Option<&'a PlayerFlying>,
    game_mode: Option<&'a PlayerGameMode>,
    sprinting: Option<&'a PlayerSprinting>,
}

struct PredictionWorld<'a> {
    tables: &'a PhysicsTables,
    blocks: DimensionView<'a>,
}

fn step_prediction(
    prediction: &mut MovementPrediction,
    movement: &PlayerMovement,
    transform: &Transform,
    current_on_ground: bool,
    context: PredictionPlayerContext<'_>,
    world: Option<PredictionWorld<'_>>,
) -> Option<Teleport> {
    if !prediction.input_driven {
        return None;
    }
    prediction.sync_item_use(context.using);
    if !prediction.synced {
        prediction.sync_state(
            transform.translation,
            GroundContact::from_on_ground(current_on_ground),
            transform.rotation,
        );
    }
    let PredictionWorld { tables, blocks } = world?;
    let look = movement.look.unwrap_or(transform.rotation);
    let flight = context.flying.is_some_and(|flag| flag.0);
    let spectator = context
        .game_mode
        .is_some_and(|mode| mode.0 == GameMode::Spectator);
    prediction.input.rotation = orientation_of(look);
    prediction.input.facts = PlayerFacts {
        spectator,
        passenger: false,
        ..PlayerFacts::default()
    };
    let block_world = BlockWorld {
        blocks: &blocks,
        table: tables.collision(),
    };
    let view = View::new(&block_world, &NoBodies, tables.tables());
    let tick = ClientTick {
        input: prediction.input,
        reported: Move {
            position: movement.position.map(reachable),
            look: movement.look.map(validate_look_of),
            on_ground: movement.on_ground.map_or(current_on_ground, |flag| flag.0),
            horizontal_collision: movement.horizontal_collision,
        },
        sprint: Some(context.sprinting.is_some_and(|flag| flag.0)),
        flight: Some(flight),
        item_use: Some(prediction.last_item_use.using),
        changes: ChangeStates {
            applied: prediction.changes_count,
            seen: prediction.changes_count,
        },
    };
    let outcome = prediction.prediction.tick(view, &[], &tick);
    prediction.last_outcome = Some(outcome);
    outcome.setback
}

fn orientation_of(rotation: Rotation) -> Orientation {
    Orientation::facing(rotation.yaw(), rotation.pitch())
}

fn validate_look_of(rotation: Rotation) -> mcrs_minecraft_move_validate::Look {
    mcrs_minecraft_move_validate::Look {
        yaw: rotation.yaw(),
        pitch: rotation.pitch(),
    }
}

fn reachable(position: DVec3) -> DVec3 {
    const MAX_POS: DVec3 = DVec3::new(MAX_XZ, MAX_Y, MAX_XZ);
    position.clamp(-MAX_POS, MAX_POS)
}

fn resend(
    owner: &Owner,
    teleport_id: i32,
    destination: Transform,
    rotation: Rotation,
) -> OutboundPlayerPacket {
    let teleport = Teleport {
        position: destination.translation,
        velocity: DVec3::ZERO,
        yaw: rotation.yaw(),
        pitch: rotation.pitch(),
        relative: Relative::empty(),
    };
    position(owner, teleport_id, &teleport)
}

fn correction(owner: &Owner, teleport_id: i32, destination: Transform) -> OutboundPlayerPacket {
    position(owner, teleport_id, &teleport_to(destination, destination))
}

fn position(owner: &Owner, teleport_id: i32, teleport: &Teleport) -> OutboundPlayerPacket {
    to(
        owner.0,
        PacketPayload::PlayerPosition(position_packet(teleport_id, teleport)),
    )
}

fn teleport(
    mut clients: Populated<
        (
            &Owner,
            &mut TeleportState,
            &Transform,
            Option<&mut MovementPrediction>,
        ),
        (Changed<Transform>, With<Owner>),
    >,
    time: Res<GameTime>,
    mut packet_writer: MessageWriter<OutboundPlayerPacket>,
) {
    for (owner, mut state, transform, mut prediction) in &mut clients {
        let known = state.synced_transform();
        if *transform == known {
            continue;
        }
        let teleport = teleport_to(known, *transform);
        let destination = landing(known, &teleport);
        let teleport_id = state.sent(destination, *time);
        if let Some(prediction) = prediction.as_deref_mut() {
            prediction.prediction.teleport(teleport);
        }
        packet_writer.write(to(
            owner.0,
            PacketPayload::PlayerPosition(position_packet(teleport_id, &teleport)),
        ));
    }
}

fn handle_accept_teleportation(
    event: On<Received<ServerboundAcceptTeleportation>>,
    mut players: Query<(&Owner, &mut TeleportState, Option<&mut MovementPrediction>)>,
    time: Res<GameTime>,
    mut packets: MessageWriter<OutboundPlayerPacket>,
) {
    let Some(accept) = event.packet() else {
        return;
    };
    let Ok((owner, mut state, mut prediction)) = players.get_mut(event.entity) else {
        return;
    };
    let position: DVec3 = accept.position.into();
    match state.accept(accept.teleport_id.0, position) {
        AcceptOutcome::Confirmed => {
            if let Some(prediction) = prediction.as_deref_mut() {
                prediction.accept_teleport(position);
            }
        }
        AcceptOutcome::Mismatch { destination } => {
            let id = state.sent(destination, *time);
            if let Some(prediction) = prediction.as_deref_mut() {
                prediction
                    .prediction
                    .teleport(teleport_to(destination, destination));
            }
            packets.write(correction(owner, id, destination));
        }
        AcceptOutcome::Invalid => {
            packets.write(to(
                owner.0,
                PacketPayload::Disconnect(Text::translate(
                    "multiplayer.disconnect.invalid_player_movement",
                    vec![],
                )),
            ));
        }
        AcceptOutcome::Ignored => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::PacketTarget;
    use bevy_ecs::message::{MessageRegistry, Messages};
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;
    use mcrs_minecraft_engine_world::session::PlayerSession;

    #[test]
    fn server_move_sends_one_position_with_the_next_teleport_id() {
        let mut world = World::new();
        MessageRegistry::register_message::<OutboundPlayerPacket>(&mut world);
        world.init_resource::<GameTime>();
        let moved_session = PlayerSession(1);
        let moved = world
            .spawn((
                Owner(moved_session),
                Transform::default().with_translation(DVec3::new(1.0, 0.0, 0.0)),
                TeleportState::default(),
            ))
            .id();
        let synced = world
            .spawn((
                Owner(PlayerSession(2)),
                Transform::default(),
                TeleportState::default(),
            ))
            .id();

        world.run_system_once(teleport).unwrap();

        let sent: Vec<_> = world
            .resource_mut::<Messages<OutboundPlayerPacket>>()
            .drain()
            .collect();
        assert_eq!(sent.len(), 1, "only the moved player is told");
        assert!(
            matches!(sent[0].target, PacketTarget::SinglePlayer(session) if session == moved_session)
        );
        let PacketPayload::PlayerPosition(position) = &sent[0].data else {
            panic!("a move is sent as a player position");
        };
        assert_eq!(position.teleport_id.0, 1, "TP-CONFIRM-ID-001");
        assert_eq!(position.position, DVec3::new(1.0, 0.0, 0.0));

        let pending = |entity| {
            world
                .get::<TeleportState>(entity)
                .unwrap()
                .pending()
                .map(|p| p.id)
        };
        assert_eq!((pending(moved), pending(synced)), (Some(1), None));
    }

    fn movement(entity: Entity, position: DVec3) -> PlayerMovement {
        PlayerMovement {
            entity,
            position: Some(position),
            look: None,
            on_ground: None,
            horizontal_collision: false,
        }
    }

    fn movement_world() -> World {
        let mut world = World::new();
        MessageRegistry::register_message::<PlayerMovement>(&mut world);
        MessageRegistry::register_message::<PlayerMoved>(&mut world);
        MessageRegistry::register_message::<OutboundPlayerPacket>(&mut world);
        world.init_resource::<GameTime>();
        world
    }

    fn sent_ids(world: &mut World) -> Vec<i32> {
        world
            .resource_mut::<Messages<OutboundPlayerPacket>>()
            .drain()
            .filter_map(|packet| match packet.data {
                PacketPayload::PlayerPosition(position) => Some(position.teleport_id.0),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_client_move_applies_only_once_the_teleport_is_confirmed() {
        let mut world = movement_world();
        let mut unconfirmed = TeleportState::default();
        unconfirmed.sent(Transform::default(), GameTime(0));
        let awaiting = world
            .spawn((
                Owner(PlayerSession(1)),
                Transform::default(),
                unconfirmed,
                ClientOnGround::default(),
            ))
            .id();
        let confirmed = world
            .spawn((
                Owner(PlayerSession(2)),
                Transform::default(),
                TeleportState::default(),
                ClientOnGround::default(),
            ))
            .id();
        let to = DVec3::new(5.0, 64.0, 5.0);
        for entity in [awaiting, confirmed] {
            world.write_message(movement(entity, to));
        }

        world.run_system_once(process_movement).unwrap();

        let at = |entity| world.get::<Transform>(entity).unwrap().translation;
        assert_eq!((at(awaiting), at(confirmed)), (DVec3::ZERO, to));
        assert_eq!(
            sent_ids(&mut world),
            [] as [i32; 0],
            "the teleport is fresh"
        );
        let emitted: Vec<_> = world
            .resource_mut::<Messages<PlayerMoved>>()
            .drain()
            .collect();
        assert_eq!(
            emitted,
            vec![PlayerMoved {
                entity: confirmed,
                delta: to,
                was_on_ground: false,
                now_on_ground: false,
            }]
        );
    }

    #[test]
    fn accept_away_from_destination_gets_a_correction_without_moving() {
        use mcrs_minecraft_core::Look;
        use mcrs_minecraft_protocol::Encode;

        let destination = Transform::default().with_translation(DVec3::new(3.0, 64.0, 0.0));
        let mut world = movement_world();
        world.add_observer(handle_accept_teleportation);
        let mut state = TeleportState::default();
        let id = state.sent(destination, GameTime(0));
        let player = world
            .spawn((Owner(PlayerSession(1)), destination, state))
            .id();
        let mut body = Vec::new();
        ServerboundAcceptTeleportation {
            teleport_id: id.into(),
            position: DVec3::new(8.0, 64.0, 0.0).into(),
            look: Look {
                yaw: 90.0,
                pitch: 10.0,
            },
        }
        .encode(&mut body)
        .unwrap();

        world.trigger(Received::<ServerboundAcceptTeleportation>::new(
            player,
            body.into(),
        ));

        assert_eq!(
            world.get::<Transform>(player),
            Some(&destination),
            "MVC-LAT-008"
        );
        let pending = world.get::<TeleportState>(player).unwrap().pending();
        assert!(pending.is_some());
        assert_eq!(sent_ids(&mut world).len(), 1);
    }

    #[test]
    fn a_vehicle_move_after_21_ticks_resends_the_pending_teleport() {
        use mcrs_minecraft_protocol::Encode;

        let mut world = movement_world();
        world.add_observer(handle_move_vehicle);
        let destination = Transform::default().with_translation(DVec3::new(3.0, 64.0, 0.0));
        let mut state = TeleportState::default();
        state.sent(destination, GameTime(0));
        let player = world
            .spawn((
                Owner(PlayerSession(1)),
                destination,
                state,
                ClientOnGround::default(),
            ))
            .id();
        let mut body = Vec::new();
        ServerboundMoveVehicle {
            position: DVec3::new(9.0, 64.0, 0.0).into(),
            look: Default::default(),
            on_ground: false,
        }
        .encode(&mut body)
        .unwrap();
        let resent_at = |world: &mut World, tick| {
            world.insert_resource(GameTime(tick));
            world.trigger(Received::<ServerboundMoveVehicle>::new(
                player,
                body.clone().into(),
            ));
            world.run_system_once(process_movement).unwrap();
            sent_ids(world)
        };

        assert_eq!(
            resent_at(&mut world, 20),
            [] as [i32; 0],
            "TP-CONFIRM-RES-006"
        );
        assert_eq!(
            resent_at(&mut world, 21),
            [2],
            "TP-CONFIRM-RES-006, TP-CONFIRM-ID-002"
        );
        let state = world.get::<TeleportState>(player).unwrap();
        assert_eq!(
            state.pending().map(|pending| pending.destination),
            Some(destination),
            "TP-CONFIRM-RES-004"
        );
        assert_eq!(world.get::<Transform>(player), Some(&destination));
    }

    #[test]
    fn the_on_ground_flag_follows_each_move_packet_of_the_player_and_no_vehicle_move() {
        use mcrs_minecraft_protocol::Encode;
        use mcrs_minecraft_protocol::packets::table::Row;
        use mcrs_minecraft_protocol_game::MoveFlags;

        fn flag_after<P: Row + Encode>(world: &mut World, player: Entity, packet: &P) -> bool {
            let mut body = Vec::new();
            packet.encode(&mut body).unwrap();
            world.trigger(Received::<P>::new(player, body.into()));
            world.run_system_once(process_movement).unwrap();
            world.get::<ClientOnGround>(player).unwrap().0
        }
        let flags = |on_ground| MoveFlags::new().with_on_ground(on_ground);
        let vehicle = |on_ground| ServerboundMoveVehicle {
            position: Default::default(),
            look: Default::default(),
            on_ground,
        };

        let mut world = movement_world();
        world.add_observer(handle_move_pos);
        world.add_observer(handle_move_pos_rot);
        world.add_observer(handle_move_rot);
        world.add_observer(handle_move_status_only);
        world.add_observer(handle_move_vehicle);
        let player = world
            .spawn((
                Owner(PlayerSession(1)),
                Transform::default(),
                TeleportState::default(),
                ClientOnGround::default(),
            ))
            .id();

        let reported = [
            flag_after(
                &mut world,
                player,
                &ServerboundMovePlayerStatusOnly { flags: flags(true) },
            ),
            flag_after(&mut world, player, &vehicle(false)),
            flag_after(
                &mut world,
                player,
                &ServerboundMovePlayerPos {
                    position: Default::default(),
                    flags: flags(false),
                },
            ),
            flag_after(
                &mut world,
                player,
                &ServerboundMovePlayerPosRot {
                    position: Default::default(),
                    look: Default::default(),
                    flags: flags(true),
                },
            ),
            flag_after(
                &mut world,
                player,
                &ServerboundMovePlayerRot {
                    look: Default::default(),
                    flags: flags(false),
                },
            ),
            flag_after(&mut world, player, &vehicle(true)),
        ];

        assert_eq!(
            reported,
            [true, true, false, true, false, false],
            "BRK-PRG-002"
        );
    }

    struct PredictionTestWorld {
        world: World,
        dimension: Entity,
        player: Entity,
        start: DVec3,
    }

    fn prediction_test_world() -> PredictionTestWorld {
        use mcrs_minecraft_engine_world::palette::{ChunkBlocks, SectionStates, air_section};
        use mcrs_minecraft_engine_world::world::dimension::DimensionTypeConfig;
        use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
        use mcrs_minecraft_physics::{
            Aabb, BlockFlags, BlockMotion, CollisionTableBuilder, InsideBoxes, InsideEffect,
            PathType, StateSpec, Tables,
        };
        use mcrs_voxel::{BlockStateId, SectionPos};

        let mut builder = CollisionTableBuilder::new();
        let air = builder
            .push(StateSpec {
                boxes: &[],
                context: None,
                flags: BlockFlags::empty(),
                inside: InsideEffect::None,
                inside_shape: InsideBoxes::FullCube,
                motion: BlockMotion::DEFAULT,
                path: PathType::Open,
            })
            .expect("push air");
        assert_eq!(air, BlockStateId::AIR, "air must occupy state 0");
        let solid = [Aabb::new(DVec3::ZERO, DVec3::ONE)];
        let stone = builder
            .push(StateSpec {
                boxes: &solid,
                context: None,
                flags: BlockFlags::empty(),
                inside: InsideEffect::None,
                inside_shape: InsideBoxes::FullCube,
                motion: BlockMotion::DEFAULT,
                path: PathType::Blocked,
            })
            .expect("push stone");
        let mut world = movement_world();
        world.insert_resource(PhysicsTables::new(Tables::new(builder.build())));
        world.add_observer(handle_move_pos);
        world.add_observer(handle_prediction_input);

        let mut section_index = SectionIndex::new();
        let floor_entity = world.spawn_empty().id();
        section_index.land(
            SectionPos::new(0, -1, 0),
            floor_entity,
            ChunkBlocks::new(SectionStates::homogeneous(stone)),
            None,
            false,
        );
        let air_entity = world.spawn_empty().id();
        section_index.land(
            SectionPos::new(0, 0, 0),
            air_entity,
            air_section(),
            None,
            false,
        );
        let dimension = world
            .spawn((section_index, DimensionTypeConfig::new(-64, 384)))
            .id();
        let start = DVec3::new(0.5, 0.0, 0.5);
        let player = world
            .spawn((
                Owner(PlayerSession(9)),
                Transform::from_translation(start),
                TeleportState::default(),
                ClientOnGround(true),
                MovementPrediction::default(),
                InDimension(dimension),
                PlayerGameMode(GameMode::Survival),
                PlayerFlying(false),
                PlayerSprinting(false),
            ))
            .id();
        PredictionTestWorld {
            world,
            dimension,
            player,
            start,
        }
    }

    fn expected_client_step(
        world: &mut World,
        dimension: Entity,
        start: DVec3,
        using: UsingItem,
    ) -> PlayerState {
        let mut expected_state = PlayerState::new(start);
        expected_state.body.flags.on_ground = true;
        expected_state.item_use = using.item_use();
        let client_input = PlayerInput {
            keys: Keys {
                forward: true,
                sprint: true,
                ..Keys::default()
            },
            ..PlayerInput::default()
        };
        let mut next_state = expected_state;
        let mut scratch = mcrs_minecraft_physics::Scratch::new();
        let mut actions = Vec::new();
        let mut state =
            bevy_ecs::system::SystemState::<(WorldView, Res<PhysicsTables>)>::new(world);
        let (world_view, tables) = state.get(world).expect("world view and tables");
        let blocks = world_view.get(dimension).expect("dimension blocks");
        let block_world = BlockWorld {
            blocks: &blocks,
            table: tables.collision(),
        };
        let view = View::new(&block_world, &NoBodies, tables.tables());
        mcrs_minecraft_physics::tick_player(
            view,
            mcrs_minecraft_physics::Side::OwnClient,
            &expected_state,
            &client_input,
            &mut next_state,
            &mut actions,
            &mut scratch,
        );
        next_state
    }

    fn trigger_packet<
        P: mcrs_minecraft_protocol::packets::table::Row + mcrs_minecraft_protocol::Encode,
    >(
        world: &mut World,
        player: Entity,
        packet: &P,
    ) {
        let mut body = Vec::new();
        packet.encode(&mut body).expect("encode test packet");
        world.trigger(Received::<P>::new(player, body.into()));
    }

    #[test]
    fn using_item_syncs_change_item_use_and_enforces_slow_movement_prediction() {
        use mcrs_minecraft_item_use::{ItemUseState, SprintPolicy, UseHand, UseSpec};
        use mcrs_minecraft_move_predict::Mode;
        use mcrs_minecraft_protocol_game::MoveFlags;
        use mcrs_minecraft_protocol_game::serverbound::PlayerInputFlags;

        let PredictionTestWorld {
            mut world,
            dimension,
            player,
            start,
        } = prediction_test_world();
        let spec = UseSpec::consumable(1.6, 0.2, SprintPolicy::Forbid);
        let using = UsingItem(ItemUseState::new(UseHand::Main, 0, spec));
        world.entity_mut(player).insert(using);
        world
            .run_system_once(sync_changed_using_item)
            .expect("sync_changed_using_item");
        let prediction = world
            .get::<MovementPrediction>(player)
            .expect("player has MovementPrediction");
        assert!(
            !prediction.is_using_item(),
            "accepted state updates on next tick after Change::ItemUse"
        );

        trigger_packet(
            &mut world,
            player,
            &ServerboundPlayerInput {
                input: PlayerInputFlags::new().with_forward(true).with_sprint(true),
            },
        );
        let next_state = expected_client_step(&mut world, dimension, start, using);
        trigger_packet(
            &mut world,
            player,
            &ServerboundMovePlayerPos {
                position: next_state.body.position.into(),
                flags: MoveFlags::new().with_on_ground(next_state.body.flags.on_ground),
            },
        );
        world
            .run_system_once(process_movement)
            .expect("process_movement passes at 0.2x");

        let prediction = world
            .get::<MovementPrediction>(player)
            .expect("player has MovementPrediction");
        assert!(
            prediction.is_using_item(),
            "accepted PlayerState::item_use must track UsingItem"
        );
        assert_eq!(
            prediction.last_outcome().map(|outcome| outcome.mode),
            Some(Mode::Predicted),
            "0.2x item-use move must pass MovePredict"
        );
        assert!(
            sent_ids(&mut world).is_empty(),
            "valid 0.2x item-use movement emits no setback teleport"
        );

        trigger_packet(
            &mut world,
            player,
            &ServerboundMovePlayerPos {
                position: (next_state.body.position + DVec3::new(0.0, 0.0, 0.25)).into(),
                flags: MoveFlags::new().with_on_ground(true),
            },
        );
        world
            .run_system_once(process_movement)
            .expect("process_movement rejects full speed while using");

        assert_eq!(
            sent_ids(&mut world).len(),
            1,
            "excessive speed while using an item triggers a setback teleport"
        );
    }
}
