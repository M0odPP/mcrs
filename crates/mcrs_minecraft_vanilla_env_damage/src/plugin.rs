use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::schedule::common_conditions::{
    on_message, resource_exists, resource_exists_and_changed,
};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_engine::PlayerMoved;
use mcrs_minecraft_engine::PoseSystems;
use mcrs_minecraft_engine::block_simulation::BlockSimulationSet;
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_damage::{DamageSet, PendingHits};
use mcrs_minecraft_engine_physics::{CrowdHitSet, PhysicsTables};
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_registries::CurrentRegistries;

use crate::bake;
use crate::bake::{BakedEnvDamage, EnvBlockTable};
use crate::env_state::{EnvDamageSet, EnvTickState, PendingEnvEffect};
use crate::fall_system;
use crate::hazard_system;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VanillaEnvDamagePlugin;

impl Plugin for VanillaEnvDamagePlugin {
    fn build(&self, app: &mut App) {
        if let Err(error) = app.try_register_required_components::<Player, EnvTickState>() {
            tracing::error!("player entity lacks required EnvTickState: {error}");
        }
        if let Err(error) = app.try_register_required_components::<Player, PlayerFileState>() {
            tracing::error!("player entity lacks required PlayerFileState: {error}");
        }
        app.init_resource::<BakedEnvDamage>()
            .init_resource::<EnvBlockTable>()
            .init_resource::<PendingHits>()
            .add_message::<PlayerMoved>()
            .add_message::<OutboundPlayerPacket>()
            .add_message::<PendingEnvEffect>()
            .configure_sets(
                FixedUpdate,
                (
                    BlockSimulationSet.before(DamageSet::Collect),
                    EnvDamageSet::Collect
                        .in_set(DamageSet::Collect)
                        .before(CrowdHitSet),
                    EnvDamageSet::Project
                        .in_set(TickSet::Project)
                        .after(DamageSet::Report)
                        .before(PoseSystems::Publish),
                ),
            )
            .add_systems(
                FixedUpdate,
                (
                    bake::bake_env_damage
                        .run_if(
                            resource_exists::<CurrentRegistries>
                                .and_then(resource_exists::<Blocks>)
                                .and_then(
                                    resource_exists_and_changed::<CurrentRegistries>
                                        .or_else(resource_exists_and_changed::<Blocks>),
                                ),
                        )
                        .in_set(TickSet::DomainInput),
                    (
                        fall_system::track_horizontal_movement.run_if(on_message::<PlayerMoved>),
                        hazard_system::collect_env_hazards.run_if(resource_exists::<PhysicsTables>),
                        fall_system::collect_fall_damage.run_if(
                            on_message::<PlayerMoved>.and_then(resource_exists::<PhysicsTables>),
                        ),
                    )
                        .chain()
                        .in_set(EnvDamageSet::Collect),
                    fall_system::emit_env_effects
                        .run_if(on_message::<PendingEnvEffect>)
                        .in_set(EnvDamageSet::Project),
                ),
            );
    }
}
