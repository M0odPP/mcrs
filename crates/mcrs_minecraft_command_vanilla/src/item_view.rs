use crate::mob_view::display_name;
use bevy_ecs::entity::Entity;
use bevy_math::DVec3;
use mcrs_minecraft_engine::command_handoff::SourceMob;
use mcrs_minecraft_entity::entity_type_keys::EntityType;
use mcrs_minecraft_entity_key::EntityId;
use mcrs_minecraft_physics::EntityState;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_text::Text;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ItemView<'a> {
    pub(crate) entity: Entity,
    pub(crate) id: EntityId,
    pub(crate) state: &'a EntityState,
}

impl ItemView<'_> {
    pub(crate) fn position(&self) -> DVec3 {
        self.state.body.position
    }

    pub(crate) fn source(&self) -> SourceMob {
        SourceMob {
            entity: self.entity,
            id: self.id,
        }
    }

    pub(crate) fn uuid(&self) -> Uuid {
        self.state.key.0
    }

    pub(crate) fn display_name(&self) -> Text {
        display_name(EntityType::Item, self.uuid(), None)
    }
}
