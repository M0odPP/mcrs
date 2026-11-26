use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::schedule::common_conditions::{
    on_message, resource_exists, resource_exists_and_changed,
};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_engine::block_simulation::BlockSimulationSet;
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::{OwnAttributesSet, PlayerMoved, PoseSystems};
use mcrs_minecraft_engine_damage::{DamageSet, Damaged, PendingHits};
use mcrs_minecraft_engine_mob_attributes::AttributeEdits;
use mcrs_minecraft_engine_physics::{CrowdHitSet, PhysicsTables};
use mcrs_minecraft_engine_world::block_update::BlockSetRequest;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_vanilla_attributes::AttributeSources;

use crate::bake::{self, BakedEnvDamage, EnvBlockTable};
use crate::env_state::{EnvDamageSet, EnvTickState, PendingEnvEffect};
use crate::registry_tables::EnvRegistryTables;
use crate::{fall_system, fire_resistance, hazard_system, lava_burn_sound, powder_snow_speed};

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
            .init_resource::<EnvRegistryTables>()
            .init_resource::<PendingHits>()
            .add_message::<PlayerMoved>()
            .add_message::<OutboundPlayerPacket>()
            .add_message::<PendingEnvEffect>()
            .add_message::<PlayerSignal>()
            .add_message::<Damaged>()
            .add_message::<BlockSetRequest>()
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
                    AttributeSources
                        .in_set(TickSet::Project)
                        .after(AttributeEdits)
                        .before(OwnAttributesSet),
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
                    fire_resistance::sync_fire_resistance_immunity.in_set(DamageSet::Filter),
                    (
                        lava_burn_sound::report_lava_burn_sounds.run_if(on_message::<Damaged>),
                        fall_system::emit_env_effects.run_if(on_message::<PendingEnvEffect>),
                    )
                        .chain()
                        .in_set(EnvDamageSet::Project),
                    powder_snow_speed::sync_powder_snow_speed
                        .in_set(AttributeSources)
                        .ambiguous_with(AttributeSources),
                ),
            );
    }
}
