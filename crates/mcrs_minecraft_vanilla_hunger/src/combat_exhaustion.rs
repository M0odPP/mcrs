use bevy_ecs::message::{MessageCursor, Messages};
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res, ResMut, SystemParam};
use mcrs_minecraft_damage::{DamageTypeRules, HitOutcome};
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_damage::{DamageTypeIds, Damaged};
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_entity_key::EntityUuid;
use mcrs_minecraft_game_mode::PlayerGameMode;
use mcrs_minecraft_hunger::ATTACK_EXHAUSTION;

use crate::movement_exhaustion::add_player_exhaustion;

#[derive(Resource, Default)]
pub(crate) struct DamagedCursor(MessageCursor<Damaged>);

pub(crate) fn has_unread_damage(
    messages: Res<Messages<Damaged>>,
    cursor: Res<DamagedCursor>,
) -> bool {
    !cursor.0.is_empty(&messages)
}

#[derive(SystemParam)]
pub(crate) struct DamageExhaustionRules<'w> {
    rules: Option<Res<'w, DamageTypeRules>>,
    ids: Option<Res<'w, DamageTypeIds>>,
}

pub(crate) fn collect_damaged_exhaustion(
    messages: Res<Messages<Damaged>>,
    mut cursor: ResMut<DamagedCursor>,
    damage: DamageExhaustionRules,
    mut players: Query<(&EntityUuid, &PlayerGameMode, &mut PlayerFileState), With<Player>>,
) {
    let player_attack_id = damage.ids.as_deref().and_then(|ids| ids.player_attack);
    for event in cursor.0.read(&messages) {
        if event.report.health > 0.0 {
            let exhaustion = damage
                .rules
                .as_deref()
                .map_or(0.0, |rules| rules.exhaustion(event.source.damage_type));
            if exhaustion > 0.0
                && let Ok((_, mode, file_state)) = players.get_mut(event.target)
            {
                add_player_exhaustion(mode.0, file_state, exhaustion);
            }
        }
        if event.report.outcome != HitOutcome::Rejected
            && player_attack_id == Some(event.source.damage_type)
            && let Some(direct) = event.source.cause.direct
        {
            for (uuid, mode, file_state) in &mut players {
                if *uuid == direct {
                    add_player_exhaustion(mode.0, file_state, ATTACK_EXHAUSTION);
                    break;
                }
            }
        }
    }
}
