use bevy_ecs::resource::Resource;
use mcrs_minecraft_save::Difficulty;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerDifficulty(pub Difficulty);
