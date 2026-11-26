use bevy_app::{App, Plugin};
use mcrs_minecraft_engine_world::world::sub_app::AddDimensionPlugins;

pub struct VanillaEquipmentPlugin;

impl Plugin for VanillaEquipmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_dimension_plugin(mcrs_minecraft_vanilla_equipment::VanillaEquipmentPlugin);
    }
}
