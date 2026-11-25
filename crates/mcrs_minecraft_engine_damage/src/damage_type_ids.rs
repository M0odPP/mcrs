use bevy_ecs::resource::Resource;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageTypeIds {
    pub starve: Option<u32>,
    pub player_attack: Option<u32>,
}
