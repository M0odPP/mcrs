use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::schedule::common_conditions::{on_message, resource_exists_and_changed};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};
use mcrs_minecraft_engine::PoseSystems;
use mcrs_minecraft_engine::bus::OutboundPlayerPacket;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine_damage::{DamageSet, Damaged, MeleeAttackHit};
use mcrs_minecraft_engine_item::ItemSet;
use mcrs_minecraft_engine_player_vitals::PlayerVitalsSet;
use mcrs_minecraft_engine_world::block_drops::BlockBroken;
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_registries::CurrentRegistries;

use crate::DurabilityExtraUse;
use crate::wear_systems;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DurabilityProjectSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DurabilityPlugin;

impl Plugin for DurabilityPlugin {
    fn build(&self, app: &mut App) {
        let block_wear = wear_systems::on_block_broken.run_if(on_message::<BlockBroken>);
        let melee_wear = wear_systems::on_melee_attack.run_if(on_message::<MeleeAttackHit>);
        let armour_wear = wear_systems::on_entity_damaged.run_if(on_message::<Damaged>);
        let extra_wear = wear_systems::on_extra_use.run_if(on_message::<DurabilityExtraUse>);
        let simulate = (block_wear, melee_wear).chain().in_set(TickSet::Simulate);
        let project = (armour_wear, extra_wear)
            .chain()
            .in_set(DurabilityProjectSet)
            .in_set(TickSet::Project)
            .after(DamageSet::Report)
            .before(PoseSystems::Publish)
            .before(PlayerVitalsSet::Health)
            .before(PlayerVitalsSet::SignalHits);
        app.init_resource::<wear_systems::UnbreakingEnchantment>()
            .init_resource::<wear_systems::PendingConsumes>()
            .add_message::<DurabilityExtraUse>()
            .add_message::<BlockBroken>()
            .add_message::<MeleeAttackHit>()
            .add_message::<Damaged>()
            .add_message::<OutboundPlayerPacket>()
            .add_message::<PlayerSignal>()
            .add_systems(
                FixedUpdate,
                wear_systems::find_unbreaking_enchantment
                    .run_if(resource_exists_and_changed::<CurrentRegistries>)
                    .in_set(TickSet::DomainInput),
            )
            .add_systems(FixedUpdate, simulate.after(ItemSet))
            .add_systems(FixedUpdate, project);
    }
}
