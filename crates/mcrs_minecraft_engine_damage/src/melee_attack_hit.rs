use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeleeAttackHit {
    pub attacker: Entity,
    pub target: Entity,
}
