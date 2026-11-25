use std::mem::{align_of, size_of};

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageReader};
use bevy_ecs::prelude::Query;
use bevy_ecs::query::With;
use bevy_ecs::schedule::SystemSet;
use bevy_ecs::system::ResMut;
use mcrs_engine_muxer::EntityEvent;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_equipment::EquipmentSlots;
use mcrs_minecraft_protocol_game::entity::EquipmentSlot;
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_registries::Wire;
use mcrs_minecraft_replication::PlayerData;

use crate::replicated::Replicated;
use crate::tile_events::TileEvents;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PublishPlayerTilesSet;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlayerEquipmentSyncSet;

#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct OwnEquipment(pub EquipmentSlots<ProtoStack<Wire>>);

const _: () = assert!(
    size_of::<OwnEquipment>() == size_of::<EquipmentSlots<ProtoStack<Wire>>>()
        && size_of::<OwnEquipment>() <= 384,
    "OwnEquipment must wrap EquipmentSlots without overhead"
);
const _: () = assert!(
    align_of::<OwnEquipment>() == align_of::<EquipmentSlots<ProtoStack<Wire>>>(),
    "OwnEquipment alignment must match EquipmentSlots"
);

impl OwnEquipment {
    #[must_use]
    pub fn to_wire_vec(&self) -> Vec<(EquipmentSlot, ProtoStack<Wire>)> {
        self.0
            .non_empty()
            .map(|(slot, stack)| {
                (
                    mcrs_minecraft_replication::protocol_slot(slot),
                    stack.clone(),
                )
            })
            .collect()
    }
}

#[derive(Message, Clone, Debug)]
pub enum PlayerEquipmentDelta {
    SwapHands {
        player: Entity,
    },
    Slots {
        player: Entity,
        slots: Vec<(EquipmentSlot, ProtoStack<Wire>)>,
    },
}

const _: () = assert!(
    size_of::<PlayerEquipmentDelta>()
        == size_of::<Entity>() + size_of::<Vec<(EquipmentSlot, ProtoStack<Wire>)>>(),
    "PlayerEquipmentDelta enum tag must fit in Vec niche"
);
const _: () = assert!(
    align_of::<PlayerEquipmentDelta>() == align_of::<Entity>(),
    "PlayerEquipmentDelta alignment must match Entity"
);

pub(crate) fn publish_player_equipment(
    mut events: ResMut<TileEvents>,
    mut deltas: MessageReader<PlayerEquipmentDelta>,
    players: Query<&Replicated, With<Player>>,
) {
    for delta in deltas.read() {
        let player = match delta {
            PlayerEquipmentDelta::SwapHands { player }
            | PlayerEquipmentDelta::Slots { player, .. } => *player,
        };
        let Ok(replicated) = players.get(player) else {
            continue;
        };
        let event = match delta {
            PlayerEquipmentDelta::SwapHands { .. } => {
                EntityEvent::Data(PlayerData::SwapHands.into())
            }
            PlayerEquipmentDelta::Slots { slots, .. } => {
                EntityEvent::Data(PlayerData::Equipment(slots.clone()).into())
            }
        };
        events.push(replicated.tile, replicated.id, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_app::App;
    use bevy_ecs::message::{MessageRegistry, Messages};
    use bevy_ecs::system::{IntoSystem, System};
    use mcrs_engine_muxer::{EntityId, TileKey};
    use mcrs_minecraft_equipment::EquipSlot;
    use mcrs_minecraft_game_mode::GameMode;
    use mcrs_minecraft_replication::ReplicationData;
    use mcrs_minecraft_resource::Id;

    use crate::dim_player_plugin::DimPlayerPlugin;
    use crate::replicated::Visibility;

    fn stack(raw: u16, count: i32) -> ProtoStack<Wire> {
        let id = Id::checked(raw, 4096).expect("id").into();
        ProtoStack {
            count,
            id,
            ..Default::default()
        }
    }

    #[test]
    fn own_equipment_required_on_player_and_converts_via_protocol_slot() {
        let mut app = App::new();
        app.add_plugins(DimPlayerPlugin);
        let player = app.world_mut().spawn(Player).id();
        assert!(
            app.world().get::<OwnEquipment>(player).is_some(),
            "Player requires OwnEquipment"
        );
        let mut eq = OwnEquipment::default();
        eq.0.set(EquipSlot::Head, Some(stack(5, 1)));
        eq.0.set(EquipSlot::MainHand, Some(stack(1, 2)));
        assert_eq!(
            eq.to_wire_vec(),
            vec![
                (EquipmentSlot::MainHand, stack(1, 2)),
                (EquipmentSlot::Head, stack(5, 1))
            ],
            "to_wire_vec must convert slots via protocol_slot in wire order"
        );
    }

    #[test]
    fn publish_player_equipment_ignores_unreplicated_and_emits_for_replicated() {
        let mut world = bevy_ecs::world::World::new();
        world.init_resource::<TileEvents>();
        MessageRegistry::register_message::<PlayerEquipmentDelta>(&mut world);
        let unreplicated = world.spawn(Player).id();
        let replicated = world
            .spawn((
                Player,
                Replicated {
                    id: EntityId(7),
                    tile: TileKey::new(1, 0, -1),
                    game_mode: GameMode::Survival,
                    visibility: Visibility::Shown,
                },
            ))
            .id();

        let slots = vec![(EquipmentSlot::MainHand, stack(10, 1))];
        let mut deltas = world.resource_mut::<Messages<PlayerEquipmentDelta>>();
        deltas.write(PlayerEquipmentDelta::Slots {
            player: unreplicated,
            slots: slots.clone(),
        });
        deltas.write(PlayerEquipmentDelta::SwapHands {
            player: unreplicated,
        });
        deltas.write(PlayerEquipmentDelta::Slots {
            player: replicated,
            slots: slots.clone(),
        });
        deltas.write(PlayerEquipmentDelta::SwapHands { player: replicated });

        let mut sys = IntoSystem::into_system(publish_player_equipment);
        sys.initialize(&mut world);
        let _ = sys.run((), &mut world);

        let taken = world.resource_mut::<TileEvents>().take();
        assert_eq!(
            taken.tiles.len(),
            1,
            "only replicated player emits tile events: {:?}",
            taken.tiles
        );
        let events: Vec<_> = taken.tiles[0]
            .events
            .iter()
            .map(|ev| ev.event.clone())
            .collect();
        assert_eq!(
            events,
            vec![
                EntityEvent::Data(ReplicationData::Player(PlayerData::Equipment(slots))),
                EntityEvent::Data(ReplicationData::Player(PlayerData::SwapHands)),
            ],
            "replicated player deltas must emit Equipment then SwapHands"
        );
    }
}
