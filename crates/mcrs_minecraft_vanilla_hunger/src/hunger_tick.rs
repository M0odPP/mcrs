use bevy_ecs::{
    change_detection::DetectChangesMut,
    entity::Entity,
    query::With,
    system::{Populated, Res, ResMut, SystemParam},
};
use mcrs_minecraft_damage::{DamageSource, Hit, Vitals};
use mcrs_minecraft_engine::{
    PlayerBody, PlayerLivedTicks, ServerDifficulty, max_health, player_dimensions,
    player_state::PlayerFileState,
};
use mcrs_minecraft_engine_damage::{DamageRequest, DamageTypeIds, PendingHits};
use mcrs_minecraft_engine_world::{
    birth_clock::BirthClock,
    entity::{physics::Transform, player::Player},
    game_rules::GameRules,
    world::{dimension::InDimension, storage::column::LoadedColumns},
};
use mcrs_minecraft_entity_key::{EntityUuid, WriterKey};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_hunger::{HungerDifficulty, HungerTickInput, hunger_effect_exhaustion};
use mcrs_minecraft_physics::Aabb;
use mcrs_minecraft_pose::Pose;
use mcrs_minecraft_save::Difficulty;

use crate::movement_exhaustion::{hunger_state_from_file, is_invulnerable};

#[derive(SystemParam)]
pub(crate) struct HungerContext<'w, 's> {
    rules: Option<Res<'w, GameRules>>,
    difficulty: Option<Res<'w, ServerDifficulty>>,
    damage_types: Option<Res<'w, DamageTypeIds>>,
    clock: Option<Res<'w, BirthClock>>,
    columns: LoadedColumns<'w, 's>,
}

type PlayerHungerRow<'a> = (
    Entity,
    &'a EntityUuid,
    &'a PlayerGameMode,
    &'a mut Vitals,
    &'a mut PlayerFileState,
    Option<&'a PlayerLivedTicks>,
    Option<&'a InDimension>,
    Option<&'a Transform>,
    Option<PlayerBody>,
);

const fn to_hunger_difficulty(difficulty: Difficulty) -> HungerDifficulty {
    match difficulty {
        Difficulty::Peaceful => HungerDifficulty::Peaceful,
        Difficulty::Easy => HungerDifficulty::Easy,
        Difficulty::Normal => HungerDifficulty::Normal,
        Difficulty::Hard => HungerDifficulty::Hard,
    }
}

fn active_hunger_amplifier(file_state: &PlayerFileState) -> Option<u8> {
    file_state
        .active_effects
        .iter()
        .filter(|effect| {
            (effect.duration == -1 || effect.duration > 0)
                && matches!(effect.id.as_str(), "minecraft:hunger" | "hunger")
        })
        .map(|effect| effect.amplifier)
        .max()
}

pub(crate) fn hunger_tick(
    mut players: Populated<PlayerHungerRow, With<Player>>,
    context: HungerContext,
    mut pending: ResMut<PendingHits>,
) {
    let natural_regeneration = context
        .rules
        .as_deref()
        .is_none_or(|rules| rules.is_on(GameRule::NaturalHealthRegeneration));
    let difficulty = context
        .difficulty
        .as_deref()
        .map_or(HungerDifficulty::Normal, |difficulty| {
            to_hunger_difficulty(difficulty.0)
        });
    let starve_id = context
        .damage_types
        .as_deref()
        .and_then(|types| types.starve);
    let now = context.clock.as_deref().map_or(0, |clock| clock.0);

    for (entity, uuid, mode, vitals, mut file_state, lived_ticks, dim, transform, body) in
        &mut players
    {
        if mode.0 == GameMode::Spectator
            && let Some((dim, transform)) = dim.zip(transform)
        {
            let dims = body.as_ref().map_or_else(
                || player_dimensions(Pose::Standing, None),
                |b| b.dimensions(),
            );
            let bounds = Aabb::of_body(transform.translation, dims.width, dims.height);
            if context
                .columns
                .touches_unloaded_chunk(dim.0, bounds.min, bounds.max)
            {
                continue;
            }
        }

        let invulnerable = is_invulnerable(mode.0, &file_state);
        let initial = hunger_state_from_file(&file_state);
        let mut next = initial;
        if !invulnerable && let Some(amplifier) = active_hunger_amplifier(&file_state) {
            next.add_exhaustion(hunger_effect_exhaustion(amplifier));
        }

        let player_max_health = max_health(body.as_ref().and_then(|b| b.attributes));
        let output = next.tick(HungerTickInput::new(
            vitals.health,
            player_max_health,
            difficulty,
            natural_regeneration,
            invulnerable,
            lived_ticks.map_or(0, |ticks| ticks.0),
        ));

        if next != initial {
            file_state
                .reborrow()
                .map_unchanged(|state| &mut state.food_level)
                .set_if_neq(next.food_level);
            file_state
                .reborrow()
                .map_unchanged(|state| &mut state.food_tick_timer)
                .set_if_neq(next.tick_timer);
            file_state
                .reborrow()
                .map_unchanged(|state| &mut state.food_saturation_level)
                .set_if_neq(next.saturation);
            file_state
                .reborrow()
                .map_unchanged(|state| &mut state.food_exhaustion_level)
                .set_if_neq(next.exhaustion);
        }

        if let Some(next_health) = output.next_health
            && next_health != vitals.health
        {
            vitals
                .map_unchanged(|current| &mut current.health)
                .set_if_neq(next_health);
        }

        if output.starve
            && !invulnerable
            && let Some(starve_type) = starve_id
        {
            let sequence = u32::try_from(pending.len()).unwrap_or(u32::MAX);
            pending.push(DamageRequest::new(
                entity,
                Hit::new(1.0, DamageSource::of(starve_type)),
                WriterKey::Entity { key: *uuid },
                now,
                sequence,
            ));
        }
    }
}
