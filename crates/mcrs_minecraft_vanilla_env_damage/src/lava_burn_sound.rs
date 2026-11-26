use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::prelude::{Query, Res, With};
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_damage::Damaged;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;

use crate::bake::{self, BakedEnvDamage};
use crate::env_state::{self, PendingEnvEffect};

pub(crate) fn report_lava_burn_sounds(
    mut damaged: MessageReader<Damaged>,
    baked: Res<BakedEnvDamage>,
    clock: Option<Res<BirthClock>>,
    players: Query<(&Transform, &PlayerFileState), With<Player>>,
    mut effects: MessageWriter<PendingEnvEffect>,
) {
    let Some(lava_damage_type) = baked.lava else {
        return;
    };
    let Some(burn_sound) = baked.generic_burn else {
        return;
    };
    let now = clock.as_deref().map_or(0, |birth| birth.0);
    for hit in damaged.read() {
        if hit.source.damage_type != lava_damage_type {
            continue;
        }
        let Ok((transform, file_state)) = players.get(hit.target) else {
            continue;
        };
        if file_state.silent {
            continue;
        }
        let pitch = 2.0_f32 + env_state::unit_random(now, hit.target, 7) * 0.4_f32;
        effects.write(PendingEnvEffect {
            actor: hit.target,
            position: transform.translation,
            entity_sound: Some(bake::sound_spec_wire(burn_sound, 0.4_f32, pitch)),
            block_sound: None,
            honey_slide: false,
        });
    }
}
