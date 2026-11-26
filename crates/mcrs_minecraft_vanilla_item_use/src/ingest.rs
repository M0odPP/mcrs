use std::mem::{align_of, size_of};

use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageWriter};
use bevy_ecs::observer::On;
use mcrs_minecraft_inventory_player::HOTBAR;
use mcrs_minecraft_network::event::Received;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::Hand;
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{
    ServerboundPlayerAction, ServerboundSetCarriedItem, ServerboundUseItem,
};

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartUseItem {
    pub player: Entity,
    pub hand: Hand,
    pub sequence: VarInt,
}

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseUseItem {
    pub player: Entity,
}

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarriedSlotChanged {
    pub player: Entity,
    pub slot: u8,
}

const _: () = {
    assert!(size_of::<StartUseItem>() == 16, "StartUseItem is 16 bytes");
    assert!(align_of::<StartUseItem>() == 8, "StartUseItem aligns to 8");
    assert!(
        size_of::<ReleaseUseItem>() == size_of::<Entity>(),
        "ReleaseUseItem matches Entity size"
    );
    assert!(
        align_of::<ReleaseUseItem>() == align_of::<Entity>(),
        "ReleaseUseItem matches Entity align"
    );
    assert!(
        size_of::<CarriedSlotChanged>() == 16,
        "CarriedSlotChanged is 16 bytes"
    );
    assert!(
        align_of::<CarriedSlotChanged>() == 8,
        "CarriedSlotChanged aligns to 8"
    );
};

pub(crate) fn on_use_item(
    received: On<Received<ServerboundUseItem>>,
    mut writer: MessageWriter<StartUseItem>,
) {
    if let Some(packet) = received.packet() {
        writer.write(StartUseItem {
            player: received.entity,
            hand: packet.hand,
            sequence: packet.sequence,
        });
    }
}

pub(crate) fn on_player_action(
    received: On<Received<ServerboundPlayerAction>>,
    mut writer: MessageWriter<ReleaseUseItem>,
) {
    if let Some(packet) = received.packet()
        && packet.action == PlayerAction::ReleaseUseItem
    {
        writer.write(ReleaseUseItem {
            player: received.entity,
        });
    }
}

pub(crate) fn on_set_carried_item(
    received: On<Received<ServerboundSetCarriedItem>>,
    mut writer: MessageWriter<CarriedSlotChanged>,
) {
    if let Some(packet) = received.packet()
        && HOTBAR.contains(&packet.slot)
        && let Ok(slot) = u8::try_from(packet.slot)
    {
        writer.write(CarriedSlotChanged {
            player: received.entity,
            slot,
        });
    }
}
