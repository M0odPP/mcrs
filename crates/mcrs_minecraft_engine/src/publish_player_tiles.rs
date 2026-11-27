use bevy_ecs::component::Component;
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::observer::On;
use bevy_ecs::prelude::{
    Commands, DetectChanges, Entity, Has, Populated, Query, Ref, ResMut, With, Without,
};
use bytes::Bytes;
use mcrs_engine_muxer::{EntityEvent, EntityId, TileKey, ViewerId};
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::world::dimension::{DimensionTypeConfig, InDimension};
use mcrs_minecraft_entity_attribute::AttributeInstances;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol_game::clientbound::ClientboundSystemChatPacket;
use mcrs_minecraft_replication::{AttributeState, PlayerData, PlayerSpawn, Pose};
use mcrs_minecraft_text::{Color, IntoText, RawText, Text};

use crate::cell_crossing::CrossedAt;
use crate::game_mode::ListedGameMode;
use crate::game_profile::GameProfile;
use crate::hot_ticket::CellCrossing;
use crate::network_id::player_network_id;
use crate::own_equipment::OwnEquipment;
use crate::publish_pose::PoseInputs;
use crate::replicated::{Replicated, Visibility};
use crate::tile_events::TileEvents;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct AnnounceJoin;

#[derive(Component, Clone, Copy, Debug, Default)]
#[component(storage = "SparseSet")]
pub struct HiddenPlayer;

#[must_use]
pub fn player_entity_id(session: PlayerSession) -> EntityId {
    EntityId(u64::from(player_network_id(session) as u32))
}

fn pose(transform: &Transform) -> Pose {
    Pose {
        position: transform.translation,
        look: transform.rotation.into(),
    }
}

#[cfg_attr(
    feature = "telemetry-tracy",
    tracing::instrument(name = "aoi::publish_player_tiles", skip_all)
)]
#[cfg_attr(
    mcrs_lint,
    expect(
        mcrs::alloc_in_tick,
        reason = "allocates only when a player first enters replication or joins, and the spawn data moves into the tile events"
    )
)]
#[cfg_attr(mcrs_lint, mcrs::alloc_sites(8))]
pub fn publish_player_tiles(
    mut events: ResMut<TileEvents>,
    dimensions: Query<(), With<DimensionTypeConfig>>,
    mut players: Populated<
        (
            Entity,
            &Owner,
            Ref<Transform>,
            &InDimension,
            Option<&mut Replicated>,
            Option<&GameProfile>,
            Option<&ListedGameMode>,
            Has<AnnounceJoin>,
            Option<&CrossedAt>,
            Option<&AttributeInstances>,
            PoseInputs,
            Option<&OwnEquipment>,
            Has<HiddenPlayer>,
        ),
        With<Player>,
    >,
    mut commands: Commands,
) {
    for (
        player,
        &Owner(session),
        transform,
        dimension,
        replicated,
        profile,
        game_mode,
        announce,
        crossed,
        attributes,
        pose_inputs,
        own_equipment,
        hidden,
    ) in &mut players
    {
        if crossed.is_some() {
            commands.entity(player).remove::<CrossedAt>();
        }
        if !dimensions.contains(dimension.0) {
            continue;
        }
        let tile = TileKey::at(transform.translation);
        let game_mode = game_mode.map(|mode| mode.0).unwrap_or_default();
        let visibility = visibility(hidden);
        let Some(mut replicated) = replicated else {
            let id = player_entity_id(session);
            events.push(
                tile,
                id,
                EntityEvent::Spawn(
                    PlayerData::Spawn(PlayerSpawn {
                        viewer: ViewerId(session.0),
                        uuid: profile.map_or(Uuid::nil(), |profile| profile.id),
                        username: profile
                            .map(|profile| profile.username.clone())
                            .unwrap_or_default(),
                        properties: profile
                            .map(|profile| profile.properties.clone())
                            .unwrap_or_default(),
                        pose: pose(&transform),
                        game_mode,
                        attributes: attributes.map(stored_attributes).unwrap_or_default(),
                        entries: pose_inputs.entries(),
                        equipment: own_equipment.map_or_else(Vec::new, OwnEquipment::to_wire_vec),
                    })
                    .into(),
                ),
            );
            if announce {
                commands.entity(player).remove::<AnnounceJoin>();
                if let Some(message) =
                    profile.and_then(|profile| presence(profile, "multiplayer.player.joined"))
                {
                    events.push(
                        tile,
                        id,
                        EntityEvent::Data(PlayerData::Joined(message).into()),
                    );
                }
            }
            if visibility == Visibility::Hidden {
                publish_visibility(&mut events, tile, id, visibility);
            }
            commands.entity(player).insert(Replicated {
                id,
                tile,
                game_mode,
                visibility,
            });
            continue;
        };
        let id = replicated.id;
        let moved = crossed.is_none_or(|crossed| crossed.0 != *transform);
        if transform.is_changed() && moved {
            let from = replicated.tile;
            if from != tile {
                events.push(from, id, EntityEvent::Leave { to: tile });
                events.push(tile, id, EntityEvent::Enter { from });
                replicated.tile = tile;
            }
            events.push(
                tile,
                id,
                EntityEvent::Move(PlayerData::Pose(pose(&transform)).into()),
            );
        }
        if replicated.game_mode != game_mode {
            replicated.game_mode = game_mode;
            events.push(
                tile,
                id,
                EntityEvent::Data(PlayerData::GameMode(game_mode).into()),
            );
        }
        if replicated.visibility != visibility {
            publish_visibility(&mut events, tile, id, visibility);
            replicated.visibility = visibility;
        }
    }
}

fn visibility(hidden: bool) -> Visibility {
    if hidden {
        Visibility::Hidden
    } else {
        Visibility::Shown
    }
}

fn publish_visibility(
    events: &mut TileEvents,
    tile: TileKey,
    id: EntityId,
    visibility: Visibility,
) {
    let data = match visibility {
        Visibility::Shown => PlayerData::Show,
        Visibility::Hidden => PlayerData::Hide,
    };
    events.push(tile, id, EntityEvent::Data(data.into()));
}

fn stored_attributes(attributes: &AttributeInstances) -> Vec<AttributeState> {
    attributes
        .set()
        .instances()
        .filter(|instance| instance.attribute().definition().syncable)
        .map(|instance| AttributeState {
            attribute: instance.attribute(),
            instance: Some(instance.clone()),
        })
        .collect()
}

pub fn publish_player_despawn(
    remove: On<Remove<Replicated>>,
    players: Query<&Replicated, Without<CellCrossing>>,
    mut events: ResMut<TileEvents>,
) {
    if let Ok(replicated) = players.get(remove.event().entity) {
        events.push(replicated.tile, replicated.id, EntityEvent::Despawn);
    }
}

pub fn publish_leave_message(
    events: &mut TileEvents,
    replicated: &Replicated,
    profile: &GameProfile,
) {
    if let Some(message) = presence(profile, "multiplayer.player.left") {
        events.push(
            replicated.tile,
            replicated.id,
            EntityEvent::Data(PlayerData::Left(message).into()),
        );
    }
}

fn presence(profile: &GameProfile, key: &'static str) -> Option<Bytes> {
    let text: Text =
        Text::translate(key, vec![profile.username.clone().into_text()]).color(Color::YELLOW);
    let content = RawText::from_text(&text).ok()?;
    mcrs_minecraft_replication::record(&ClientboundSystemChatPacket {
        content,
        overlay: false,
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_sprinting::PlayerSprinting;
    use crate::shift_key::ShiftKey;
    use bevy_ecs::message::MessageRegistry;
    use bevy_ecs::system::{IntoSystem, System};
    use mcrs_minecraft_equipment::EquipSlot;
    use mcrs_minecraft_protocol_game::entity::EquipmentSlot;
    use mcrs_minecraft_protocol_game::item::ProtoStack;
    use mcrs_minecraft_registries::Wire;
    use mcrs_minecraft_replication::ReplicationData;
    use mcrs_minecraft_resource::Id;

    fn wire_stack(raw: u16, count: i32) -> ProtoStack<Wire> {
        let id = Id::checked(raw, 4096).expect("id").into();
        ProtoStack {
            count,
            id,
            ..Default::default()
        }
    }

    #[test]
    fn publish_player_tiles_seeds_spawn_equipment_and_omits_when_empty() {
        let mut world = bevy_ecs::world::World::new();
        world.init_resource::<TileEvents>();
        MessageRegistry::register_message::<crate::bus::InboundPlayerDespawn>(&mut world);
        let dim = world.spawn(DimensionTypeConfig::new(-64, 384)).id();

        let mut eq = OwnEquipment::default();
        eq.0.set(EquipSlot::MainHand, Some(wire_stack(10, 1)));
        eq.0.set(EquipSlot::Head, Some(wire_stack(20, 1)));
        let base = || {
            (
                Player,
                Transform::default(),
                InDimension(dim),
                mcrs_minecraft_pose::Pose::default(),
                ShiftKey::default(),
                PlayerSprinting::default(),
            )
        };
        let armed = world.spawn((base(), Owner(PlayerSession(1)), eq)).id();
        let empty = world
            .spawn((base(), Owner(PlayerSession(2)), OwnEquipment::default()))
            .id();

        let mut sys = IntoSystem::into_system(publish_player_tiles);
        sys.initialize(&mut world);
        let _ = sys.run((), &mut world);
        sys.apply_deferred(&mut world);

        assert!(
            world.get::<Replicated>(armed).is_some() && world.get::<Replicated>(empty).is_some(),
            "spawn attaches Replicated"
        );
        let taken = world.resource_mut::<TileEvents>().take();
        let spawns: Vec<_> = taken
            .tiles
            .iter()
            .flat_map(|d| d.events.iter())
            .filter_map(|ev| match &ev.event {
                EntityEvent::Spawn(ReplicationData::Player(PlayerData::Spawn(s))) => {
                    Some((s.viewer, s.equipment.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            spawns,
            vec![
                (
                    ViewerId(1),
                    vec![
                        (EquipmentSlot::MainHand, wire_stack(10, 1)),
                        (EquipmentSlot::Head, wire_stack(20, 1))
                    ]
                ),
                (ViewerId(2), Vec::new()),
            ]
        );
    }
}
