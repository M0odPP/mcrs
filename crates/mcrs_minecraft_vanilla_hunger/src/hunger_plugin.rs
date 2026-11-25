use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::schedule::{IntoScheduleConfigs, common_conditions::on_message};
use mcrs_minecraft_engine::{PlayerMoved, ServerDifficulty};
use mcrs_minecraft_engine_damage::DamageSet;
use mcrs_minecraft_engine_physics::CrowdHitSet;
use mcrs_minecraft_engine_world::{block_drops::BlockBroken, tick::TickSet};

use crate::{
    combat_exhaustion::{DamagedCursor, collect_damaged_exhaustion, has_unread_damage},
    hunger_tick::hunger_tick,
    movement_exhaustion::{collect_block_break_exhaustion, collect_move_exhaustion},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct VanillaHungerPlugin;

impl Plugin for VanillaHungerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DamagedCursor>();
        app.init_resource::<ServerDifficulty>();
        app.add_systems(
            FixedUpdate,
            (
                (
                    collect_move_exhaustion.run_if(on_message::<PlayerMoved>),
                    collect_block_break_exhaustion.run_if(on_message::<BlockBroken>),
                )
                    .chain()
                    .in_set(TickSet::Simulate),
                collect_damaged_exhaustion
                    .run_if(has_unread_damage)
                    .in_set(DamageSet::PacketReport),
                hunger_tick.in_set(DamageSet::Collect).after(CrowdHitSet),
                collect_damaged_exhaustion
                    .run_if(has_unread_damage)
                    .in_set(DamageSet::Report),
            ),
        );
    }
}
