use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;

use crate::DurabilitySlot;

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurabilityExtraUse {
    pub target: Entity,
    pub slot: DurabilitySlot,
    pub amount: i32,
}
