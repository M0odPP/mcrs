use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::schedule::IntoScheduleConfigs;
use mcrs_minecraft_engine_damage::VitalsTickSet;
use mcrs_minecraft_engine_world::world::sub_app::AddDimensionPlugins;
use mcrs_minecraft_vanilla_hunger::HungerSimulateSet;
use mcrs_minecraft_vanilla_item_use::ItemUseSet;
use mcrs_minecraft_vanilla_respawn::RespawnWriteSet;

pub struct VanillaItemUsePlugin;

impl Plugin for VanillaItemUsePlugin {
    fn build(&self, app: &mut App) {
        fn order_item_use(app: &mut App) {
            app.configure_sets(
                FixedUpdate,
                (
                    ItemUseSet::Simulate
                        .after(VitalsTickSet)
                        .before(HungerSimulateSet),
                    ItemUseSet::Finish.after(RespawnWriteSet),
                ),
            );
        }
        app.add_dimension_plugin(mcrs_minecraft_vanilla_item_use::VanillaItemUsePlugin)
            .add_dimension_plugin(order_item_use);
    }
}
