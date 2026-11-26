use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::schedule::common_conditions::{any_with_component, on_message};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemCondition, SystemSet};
use mcrs_minecraft_engine::block_simulation::BlockSimulationSet;
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::{BlockBreakSet, ItemUsePredictionSet, UsingItem};
use mcrs_minecraft_engine_damage::DamageSet;
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_vanilla_inventory::InventorySet;

use crate::ingest::{on_player_action, on_set_carried_item, on_use_item};
use crate::systems::{
    FinishUseItem, TickConsumeSound, finish_use_items, handle_use_item_inputs, tick_using_items,
};
use crate::{CarriedSlotChanged, ReleaseUseItem, StartUseItem};

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemUseSet {
    Input,
    Simulate,
    Finish,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VanillaItemUsePlugin;

impl Plugin for VanillaItemUsePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<StartUseItem>()
            .add_message::<ReleaseUseItem>()
            .add_message::<CarriedSlotChanged>()
            .add_message::<TickConsumeSound>()
            .add_message::<FinishUseItem>()
            .add_message::<OutboundPlayerPacket>()
            .add_observer(on_use_item)
            .add_observer(on_player_action)
            .add_observer(on_set_carried_item)
            .configure_sets(
                FixedUpdate,
                (
                    ItemUseSet::Input
                        .in_set(TickSet::ClientInput)
                        .after(InventorySet::Input)
                        .after(BlockBreakSet),
                    ItemUseSet::Simulate
                        .in_set(TickSet::Simulate)
                        .before(ItemUsePredictionSet),
                    ItemUseSet::Finish
                        .in_set(TickSet::WorldWrite)
                        .after(BlockSimulationSet)
                        .before(DamageSet::Collect),
                ),
            )
            .add_systems(
                FixedUpdate,
                (
                    handle_use_item_inputs
                        .run_if(
                            on_message::<StartUseItem>
                                .or_else(on_message::<ReleaseUseItem>)
                                .or_else(on_message::<CarriedSlotChanged>),
                        )
                        .in_set(ItemUseSet::Input),
                    tick_using_items
                        .run_if(any_with_component::<UsingItem>)
                        .in_set(ItemUseSet::Simulate),
                    finish_use_items
                        .run_if(on_message::<FinishUseItem>.or_else(on_message::<TickConsumeSound>))
                        .in_set(ItemUseSet::Finish),
                ),
            );
    }
}
