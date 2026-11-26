use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::query::{Changed, With};
use bevy_ecs::system::Populated;
use mcrs_minecraft_damage::{DamageTags, Defence};
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;

use crate::env_state;

pub(crate) fn sync_fire_resistance_immunity(
    mut players: Populated<
        (&PlayerFileState, &mut Defence),
        (With<Player>, Changed<PlayerFileState>),
    >,
) {
    for (file_state, mut defence) in &mut players {
        let mut next = *defence;
        next.immune = if env_state::has_active_effect(file_state, MobEffect::FireResistance) {
            defence.immune.union(DamageTags::IS_FIRE)
        } else {
            defence.immune.difference(DamageTags::IS_FIRE)
        };
        defence.set_if_neq(next);
    }
}
