use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::query::With;
use bevy_ecs::system::Populated;
use mcrs_minecraft_engine_world::entity::player::Player;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerLivedTicks(pub u64);

pub(crate) fn advance_player_lived_ticks(
    mut players: Populated<&mut PlayerLivedTicks, With<Player>>,
) {
    for mut ticks in &mut players {
        ticks.set_if_neq(PlayerLivedTicks(ticks.0.saturating_add(1)));
    }
}
