use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::query::{Changed, Or, QueryData};
use bevy_ecs::system::{Populated, ResMut};
use mcrs_engine_muxer::EntityEvent;
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine_world::session::Owner;
use mcrs_minecraft_pose::Pose;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::clientbound::ClientboundSetEntityData;
use mcrs_minecraft_protocol_game::entity::Metadata;
use mcrs_minecraft_protocol_game::entity_data_index::{
    SHARED_FLAG_ON_FIRE, SHARED_FLAG_SHIFT, SHARED_FLAG_SPRINT,
};
use mcrs_minecraft_replication::{PlayerData, PlayerEntries};

use crate::bus::{OutboundPlayerPacket, send_own_entity_state};
use crate::network_id::player_network_id;
use crate::player_sprinting::PlayerSprinting;
use crate::player_state::PlayerFileState;
use crate::replicated::Replicated;
use crate::shift_key::ShiftKey;
use crate::tile_events::TileEvents;
use crate::using_item::LivingFlags;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct OwnPoseEntries(PlayerEntries);

type PoseInputsChanged = Or<(
    Changed<Pose>,
    Changed<ShiftKey>,
    Changed<PlayerSprinting>,
    Changed<Vitals>,
    Changed<PlayerFileState>,
    Changed<LivingFlags>,
)>;

#[derive(QueryData)]
pub(crate) struct PoseInputs {
    pose: &'static Pose,
    shift: &'static ShiftKey,
    sprinting: &'static PlayerSprinting,
    vitals: Option<&'static Vitals>,
    file_state: Option<&'static PlayerFileState>,
    living_flags: Option<&'static LivingFlags>,
}

impl PoseInputsItem<'_, '_> {
    #[must_use]
    pub(crate) fn entries(&self) -> PlayerEntries {
        pose_entries(
            *self.pose,
            *self.shift,
            *self.sprinting,
            self.vitals,
            self.file_state,
            self.living_flags,
        )
    }
}

pub(crate) fn publish_pose(
    mut players: Populated<
        (&Owner, PoseInputs, &mut OwnPoseEntries, Option<&Replicated>),
        PoseInputsChanged,
    >,
    mut writer: MessageWriter<OutboundPlayerPacket>,
    mut events: Option<ResMut<TileEvents>>,
) {
    for (&Owner(session), pose_inputs, mut own, replicated) in &mut players {
        let entries = pose_inputs.entries();
        let changed = entries.changed_from(&own.0);
        if changed.is_empty() {
            continue;
        }
        if let (Some(events), Some(replicated)) = (events.as_deref_mut(), replicated) {
            events.push(
                replicated.tile,
                replicated.id,
                EntityEvent::Data(PlayerData::Entries(entries).into()),
            );
        }
        own.set_if_neq(OwnPoseEntries(entries));
        let packet = ClientboundSetEntityData {
            entity_id: VarInt(player_network_id(session)),
            metadata: Metadata(changed),
        };
        send_own_entity_state(&mut writer, session, &packet, "pose entries");
    }
}

#[must_use]
pub(crate) fn pose_entries(
    pose: Pose,
    shift: ShiftKey,
    sprinting: PlayerSprinting,
    vitals: Option<&Vitals>,
    file_state: Option<&PlayerFileState>,
    living_flags: Option<&LivingFlags>,
) -> PlayerEntries {
    let mut entries = PlayerEntries {
        pose,
        living_flags: living_flags.map_or(0, |flags| flags.0),
        ..PlayerEntries::default()
    };
    if shift.0 {
        entries.shared_flags |= SHARED_FLAG_SHIFT;
    }
    if sprinting.0 {
        entries.shared_flags |= SHARED_FLAG_SPRINT;
    }
    if let Some(vitals) = vitals {
        entries.health = vitals.health;
        if !vitals.is_alive() {
            entries.pose = Pose::Dying;
        }
    }
    if let Some(state) = file_state {
        if state.fire > 0 || state.has_visual_fire {
            entries.shared_flags |= SHARED_FLAG_ON_FIRE;
        }
        entries.air_supply = state.air;
        entries.ticks_frozen = state.ticks_frozen.max(0);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::message::{MessageRegistry, Messages};
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;
    use mcrs_engine_muxer::{EntityId, TileKey};
    use mcrs_minecraft_engine_world::session::PlayerSession;
    use mcrs_minecraft_game_mode::GameMode;
    use mcrs_minecraft_item_use::UseHand;
    use mcrs_minecraft_protocol::{Decode, Packet};
    use mcrs_minecraft_protocol_game::entity::{MetaDataValue, MetadataEntry};
    use mcrs_minecraft_protocol_game::entity_data_index::LIVING_FLAGS;
    use mcrs_minecraft_replication::ReplicationData;

    use crate::bus::PacketPayload;
    use crate::replicated::{Replicated, Visibility};

    fn drain_self_living_flags(world: &mut World, out: &mut Vec<i8>) {
        let sent: Vec<_> = world
            .resource_mut::<Messages<OutboundPlayerPacket>>()
            .drain()
            .collect();
        for packet in sent {
            let PacketPayload::OwnEntityState(bytes) = packet.data else {
                continue;
            };
            let mut slice = bytes.as_ref();
            let packet_id = VarInt::decode(&mut slice).expect("decode packet id");
            assert_eq!(
                packet_id.0,
                ClientboundSetEntityData::ID,
                "OwnEntityState encodes ClientboundSetEntityData packet id"
            );
            let decoded = ClientboundSetEntityData::decode(&mut slice)
                .expect("decode ClientboundSetEntityData");
            for entry in decoded.metadata.0 {
                if let MetadataEntry {
                    index: LIVING_FLAGS,
                    value: MetaDataValue::Byte(byte),
                } = entry
                {
                    out.push(byte);
                }
            }
        }
    }

    fn drain_observer_living_flags(world: &mut World, out: &mut Vec<u8>) {
        let tile_events = world.resource_mut::<TileEvents>().take();
        for delta in tile_events.tiles {
            for event in delta.events {
                if let EntityEvent::Data(ReplicationData::Player(PlayerData::Entries(entries))) =
                    event.event
                {
                    out.push(entries.living_flags);
                }
            }
        }
    }

    #[test]
    fn living_flags_changes_emit_index_8_for_self_and_tile_observers() {
        let mut world = World::new();
        MessageRegistry::register_message::<OutboundPlayerPacket>(&mut world);
        world.insert_resource(TileEvents::default());

        let player = world
            .spawn((
                Owner(PlayerSession(7)),
                Pose::Standing,
                ShiftKey(false),
                PlayerSprinting(false),
                LivingFlags::default(),
                OwnPoseEntries::default(),
                Replicated {
                    id: EntityId(7),
                    tile: TileKey::new(0, 0, 0),
                    game_mode: GameMode::Survival,
                    visibility: Visibility::Shown,
                },
            ))
            .id();

        let transitions = [
            Some(UseHand::Main),
            None,
            Some(UseHand::Off),
            None,
            Some(UseHand::Main),
        ];
        let mut self_bytes = Vec::new();
        let mut observer_bytes = Vec::new();
        for hand in transitions {
            let next = world
                .get::<LivingFlags>(player)
                .copied()
                .unwrap_or_default()
                .with_active_hand(hand);
            world.entity_mut(player).insert(next);
            world
                .run_system_once(publish_pose)
                .expect("publish_pose runs");
            drain_self_living_flags(&mut world, &mut self_bytes);
            drain_observer_living_flags(&mut world, &mut observer_bytes);
        }

        assert_eq!(
            self_bytes,
            [1, 0, 3, 2, 1],
            "self receives 0 -> 1 -> 0 -> 3 -> 2 -> 1 on living_flags byte"
        );
        assert_eq!(
            observer_bytes,
            [1, 0, 3, 2, 1],
            "observers receive matching living_flags transitions"
        );
    }

    #[test]
    fn pose_entries_projects_fire_air_and_frozen_state() {
        let mut state = PlayerFileState {
            fire: 40,
            air: 150,
            ticks_frozen: 90,
            ..PlayerFileState::default()
        };
        let burning = pose_entries(
            Pose::Standing,
            ShiftKey(false),
            PlayerSprinting(false),
            None,
            Some(&state),
            None,
        );
        assert_eq!(
            burning.shared_flags & SHARED_FLAG_ON_FIRE,
            SHARED_FLAG_ON_FIRE,
            "positive fire ticks should set SHARED_FLAG_ON_FIRE"
        );
        assert_eq!(
            burning.air_supply, 150,
            "air_supply should mirror PlayerFileState.air"
        );
        assert_eq!(
            burning.ticks_frozen, 90,
            "ticks_frozen should mirror PlayerFileState.ticks_frozen"
        );

        state.fire = -20;
        state.ticks_frozen = -5;
        let cooled = pose_entries(
            Pose::Standing,
            ShiftKey(false),
            PlayerSprinting(false),
            None,
            Some(&state),
            None,
        );
        assert_eq!(
            cooled.shared_flags & SHARED_FLAG_ON_FIRE,
            0,
            "non-positive fire ticks without visual fire should clear SHARED_FLAG_ON_FIRE"
        );
        assert_eq!(
            cooled.ticks_frozen, 0,
            "negative ticks_frozen should clamp to 0"
        );
    }
}
