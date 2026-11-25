use std::mem::{align_of, size_of};

use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageWriter};
use bevy_ecs::observer::On;
use mcrs_minecraft_network::event::Received;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::Hand;
use mcrs_minecraft_protocol_game::serverbound::ServerboundUseItem;

#[derive(Message, Clone, Copy, Debug)]
pub struct UseItemRequest {
    pub entity: Entity,
    pub hand: Hand,
    pub sequence: VarInt,
    pub yaw: f32,
    pub pitch: f32,
}

const _: () = assert!(
    size_of::<UseItemRequest>() == 24,
    "UseItemRequest must pack into 24 bytes"
);
const _: () = assert!(
    align_of::<UseItemRequest>() == 8,
    "UseItemRequest must align to 8 bytes"
);

pub(crate) fn handle_use_item(
    on: On<Received<ServerboundUseItem>>,
    mut writer: MessageWriter<UseItemRequest>,
) {
    let Some(packet) = on.packet() else {
        return;
    };
    writer.write(UseItemRequest {
        entity: on.entity,
        hand: packet.hand,
        sequence: packet.sequence,
        yaw: packet.look.yaw,
        pitch: packet.look.pitch,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::message::{MessageRegistry, Messages};
    use bevy_ecs::world::World;
    use bytes::Bytes;
    use mcrs_minecraft_core::Look;
    use mcrs_minecraft_protocol::Encode;

    #[test]
    fn handle_use_item_emits_request_for_valid_packet_and_ignores_malformed() {
        let mut world = World::new();
        MessageRegistry::register_message::<UseItemRequest>(&mut world);
        world.add_observer(handle_use_item);
        let player = world.spawn_empty().id();

        let mut payload = Vec::new();
        ServerboundUseItem {
            hand: Hand::Off,
            sequence: VarInt(9),
            look: Look {
                yaw: 12.5,
                pitch: -30.0,
            },
        }
        .encode(&mut payload)
        .expect("encode ServerboundUseItem");
        world.trigger(Received::<ServerboundUseItem>::new(
            player,
            Bytes::from_static(b""),
        ));
        world.trigger(Received::<ServerboundUseItem>::new(
            player,
            Bytes::from(payload),
        ));
        world.flush();

        let requests: Vec<_> = world
            .resource_mut::<Messages<UseItemRequest>>()
            .drain()
            .collect();
        assert_eq!(
            requests.len(),
            1,
            "malformed packet ignored; valid emits one UseItemRequest"
        );
        assert_eq!(
            (
                requests[0].entity,
                requests[0].hand,
                requests[0].sequence,
                requests[0].yaw,
                requests[0].pitch
            ),
            (player, Hand::Off, VarInt(9), 12.5, -30.0),
            "UseItemRequest fields must match decoded ServerboundUseItem"
        );
    }
}
