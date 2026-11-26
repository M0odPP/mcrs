use bevy_app::{App, FixedPostUpdate, FixedPreUpdate, Plugin};
use bevy_ecs::prelude::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use mcrs_minecraft_engine_world::voxel_update::VoxelUpdateSet;

use crate::own_equipment::{
    PlayerEquipmentDelta, PlayerEquipmentSyncSet, PublishPlayerTilesSet, publish_player_equipment,
};
use crate::player_signal::{PlayerSignal, publish_player_signals};
use crate::publish_player_tiles::{publish_player_despawn, publish_player_tiles};
use crate::swing::{PendingPlayerSwings, has_pending_player_swings, publish_player_swings};
use crate::tile_events::TileEvents;

pub struct PlayerTrackerPlugin;

impl Plugin for PlayerTrackerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<mcrs_minecraft_engine_world::session::DimPlayerIndex>();
        app.init_resource::<TileEvents>();
        app.init_resource::<PendingPlayerSwings>();
        app.add_message::<PlayerSignal>();
        app.add_message::<PlayerEquipmentDelta>();
        app.add_observer(publish_player_despawn);
        app.add_systems(
            FixedPreUpdate,
            crate::drain_player_despawn::drain_inbound_player_despawn
                .run_if(on_message::<crate::bus::InboundPlayerDespawn>)
                .after(crate::dim_tick::DimInboxDrain),
        );
        app.configure_sets(
            FixedPostUpdate,
            PlayerEquipmentSyncSet.before(PublishPlayerTilesSet),
        );
        app.add_systems(
            FixedPostUpdate,
            (
                publish_player_equipment
                    .run_if(on_message::<PlayerEquipmentDelta>)
                    .after(PlayerEquipmentSyncSet)
                    .before(PublishPlayerTilesSet),
                publish_player_tiles
                    .in_set(PublishPlayerTilesSet)
                    .after(VoxelUpdateSet::NetworkSync),
                publish_player_swings
                    .run_if(has_pending_player_swings)
                    .after(PublishPlayerTilesSet),
                publish_player_signals
                    .run_if(on_message::<PlayerSignal>)
                    .after(publish_player_swings),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::message::MessageWriter;
    use bevy_ecs::prelude::With;
    use bevy_ecs::schedule::{LogLevel, ScheduleBuildError, ScheduleBuildSettings, Schedules};
    use bevy_ecs::system::Query;
    use bevy_time::{TimePlugin, TimeUpdateStrategy};
    use mcrs_engine_muxer::EntityEvent;
    use mcrs_minecraft_engine_world::birth_clock::BirthClock;
    use mcrs_minecraft_engine_world::entity::physics::Transform;
    use mcrs_minecraft_engine_world::entity::player::Player;
    use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
    use mcrs_minecraft_engine_world::world::dimension::{
        DimensionPlugin, DimensionTypeConfig, InDimension,
    };
    use mcrs_minecraft_equipment::EquipSlot;
    use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
    use mcrs_minecraft_pose::Pose;
    use mcrs_minecraft_protocol_game::entity::EquipmentSlot;
    use mcrs_minecraft_protocol_game::item::ProtoStack;
    use mcrs_minecraft_registries::Wire;
    use mcrs_minecraft_replication::{PlayerData, ReplicationData};
    use mcrs_minecraft_resource::Id;

    use crate::bus::InboundPlayerDespawn;
    use crate::dim_player_plugin::DimPlayerPlugin;
    use crate::own_equipment::OwnEquipment;
    use crate::player_sprinting::PlayerSprinting;
    use crate::player_state::PlayerFileState;
    use crate::replicated::Replicated;
    use crate::shift_key::ShiftKey;
    use crate::swing::{PlayerSwingPlugin, SwingPlayer};

    fn test_stack(raw: u16, count: i32) -> ProtoStack<Wire> {
        let id = Id::checked(raw, 4096).expect("id").into();
        ProtoStack {
            count,
            id,
            ..Default::default()
        }
    }

    #[test]
    fn equipment_sync_tiles_and_swings_form_strict_fixed_post_update_chain() {
        let mut app = App::new();
        app.add_plugins((TimePlugin, PlayerSwingPlugin, PlayerTrackerPlugin))
            .add_message::<InboundPlayerDespawn>()
            .insert_resource(TimeUpdateStrategy::FixedTimesteps(1))
            .add_systems(
                FixedPostUpdate,
                (|| {})
                    .after(publish_player_swings)
                    .before(PlayerEquipmentSyncSet),
            );
        let built = app
            .world_mut()
            .schedule_scope(FixedPostUpdate, |world, schedule| {
                schedule.initialize(world).map(|_| ())
            });
        assert!(
            matches!(
                built,
                Err(ScheduleBuildError::CrossDependency(_) | ScheduleBuildError::DependencySort(_))
            ),
            "cycle check: {built:?}"
        );
    }

    #[test]
    fn player_tracker_and_dim_player_schedules_have_no_ambiguity() {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            DimensionPlugin,
            DimPlayerPlugin,
            PlayerTrackerPlugin,
        ));
        let world = app.world_mut();
        let labels: Vec<_> = world
            .resource::<Schedules>()
            .iter()
            .map(|(_, schedule)| schedule.label())
            .collect();
        for label in labels {
            let report = world
                .try_schedule_scope(label, |inner_world, schedule| {
                    let prev = schedule.get_build_settings();
                    schedule.set_build_settings(ScheduleBuildSettings {
                        ambiguity_detection: LogLevel::Error,
                        ..prev
                    });
                    schedule
                        .initialize(inner_world)
                        .err()
                        .map(|err| err.to_string(schedule.graph(), inner_world))
                })
                .expect("schedule exists");
            assert!(
                report.is_none(),
                "schedule {label:?} must build without ambiguity: {}",
                report.unwrap_or_default()
            );
        }
    }

    #[test]
    fn runtime_fixed_post_update_seeds_spawn_without_duplicate_and_orders_after_swings() {
        fn seed(
            mut query: Query<(bevy_ecs::entity::Entity, &mut OwnEquipment), With<Player>>,
            mut deltas: MessageWriter<PlayerEquipmentDelta>,
        ) {
            for (entity, mut own) in &mut query {
                if own.0.is_empty() {
                    own.0.set(EquipSlot::MainHand, Some(test_stack(11, 1)));
                    deltas.write(PlayerEquipmentDelta::Slots {
                        player: entity,
                        slots: vec![(EquipmentSlot::MainHand, test_stack(11, 1))],
                    });
                }
            }
        }
        let mut app = App::new();
        app.add_plugins((TimePlugin, PlayerSwingPlugin, PlayerTrackerPlugin))
            .add_message::<InboundPlayerDespawn>()
            .insert_resource(TimeUpdateStrategy::FixedTimesteps(1))
            .insert_resource(BirthClock(10))
            .add_systems(FixedPostUpdate, seed.in_set(PlayerEquipmentSyncSet));
        app.update();

        let dim = app
            .world_mut()
            .spawn(DimensionTypeConfig::new(-64, 384))
            .id();
        let player = app
            .world_mut()
            .spawn((
                Player,
                Owner(PlayerSession(5)),
                Transform::default(),
                InDimension(dim),
                Pose::default(),
                ShiftKey::default(),
                PlayerSprinting::default(),
                PlayerGameMode(GameMode::Survival),
                PlayerFileState::default(),
                OwnEquipment::default(),
            ))
            .id();

        app.world_mut()
            .write_message(SwingPlayer::default_main(player));
        app.update();
        assert!(
            app.world().get::<Replicated>(player).is_some(),
            "tick 1 attaches Replicated"
        );
        let taken1 = app.world_mut().resource_mut::<TileEvents>().take();
        let evs1: Vec<_> = taken1
            .tiles
            .iter()
            .flat_map(|tile| tile.events.iter().map(|entry| entry.event.clone()))
            .collect();
        assert!(
            matches!(evs1.as_slice(), [EntityEvent::Spawn(ReplicationData::Player(PlayerData::Spawn(spawn))), EntityEvent::Data(ReplicationData::Player(PlayerData::Swing { .. }))] if spawn.equipment == vec![(EquipmentSlot::MainHand, test_stack(11, 1))]),
            "tick 1 must emit Spawn(with MainHand) then Swing without duplicate Equipment: {evs1:?}"
        );

        let updated = vec![(EquipmentSlot::Head, test_stack(22, 1))];
        app.world_mut().resource_mut::<BirthClock>().0 = 20;
        app.world_mut().write_message(PlayerEquipmentDelta::Slots {
            player,
            slots: updated.clone(),
        });
        app.world_mut()
            .write_message(SwingPlayer::default_main(player));
        app.update();
        let taken2 = app.world_mut().resource_mut::<TileEvents>().take();
        let evs2: Vec<_> = taken2
            .tiles
            .iter()
            .flat_map(|tile| tile.events.iter().map(|entry| entry.event.clone()))
            .collect();
        assert!(
            matches!(evs2.as_slice(), [EntityEvent::Data(ReplicationData::Player(PlayerData::Equipment(slots))), EntityEvent::Data(ReplicationData::Player(PlayerData::Swing { .. }))] if *slots == updated),
            "tick 2 must emit Equipment then Swing: {evs2:?}"
        );
    }
}
