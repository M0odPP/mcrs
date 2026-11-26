use crate::attribute_state::AttributeState;
use crate::delta_base::Pose;
use crate::player_entries::PlayerEntries;
use mcrs_engine_muxer::ViewerId;
use mcrs_minecraft_game_mode::GameMode;
use mcrs_minecraft_profile::Property;
use mcrs_minecraft_protocol_game::entity::EquipmentSlot;
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_registries::Wire;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerSpawn {
    pub viewer: ViewerId,
    pub uuid: Uuid,
    pub username: String,
    pub properties: Vec<Property>,
    pub pose: Pose,
    pub game_mode: GameMode,
    pub attributes: Vec<AttributeState>,
    pub entries: PlayerEntries,
    pub equipment: Vec<(EquipmentSlot, ProtoStack<Wire>)>,
}
