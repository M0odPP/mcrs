use bevy_ecs::change_detection::{DetectChangesMut, Mut};
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::With;
use bevy_ecs::system::Query;
use mcrs_minecraft_engine::PlayerMoved;
use mcrs_minecraft_engine::player_sprinting::PlayerSprinting;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_world::block_drops::BlockBroken;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_hunger::{
    HARVEST_BREAK_EXHAUSTION, HungerState, SprintState, jump_exhaustion, sprint_move_exhaustion,
};

pub(crate) const fn is_invulnerable(mode: GameMode) -> bool {
    matches!(mode, GameMode::Creative | GameMode::Spectator)
}

pub(crate) fn hunger_state_from_file(file_state: &PlayerFileState) -> HungerState {
    HungerState::new(
        file_state.food_level,
        file_state.food_saturation_level,
        file_state.food_exhaustion_level,
        file_state.food_tick_timer,
    )
}

pub(crate) fn add_player_exhaustion(mode: GameMode, file_state: Mut<PlayerFileState>, amount: f32) {
    if is_invulnerable(mode) {
        return;
    }
    let mut hunger = hunger_state_from_file(&file_state);
    hunger.add_exhaustion(amount);
    if hunger.exhaustion != file_state.food_exhaustion_level {
        file_state
            .map_unchanged(|state| &mut state.food_exhaustion_level)
            .set_if_neq(hunger.exhaustion);
    }
}

pub(crate) fn collect_move_exhaustion(
    mut moves: MessageReader<PlayerMoved>,
    mut players: Query<(&PlayerGameMode, &PlayerSprinting, &mut PlayerFileState), With<Player>>,
) {
    for moved in moves.read() {
        let Ok((mode, sprinting, file_state)) = players.get_mut(moved.entity) else {
            continue;
        };
        let mut added = 0.0_f32;
        if moved.was_on_ground && !moved.now_on_ground && moved.delta.y > 0.0 {
            let sprint = if sprinting.0 {
                SprintState::Sprinting
            } else {
                SprintState::Idle
            };
            added += jump_exhaustion(sprint);
        }
        if moved.now_on_ground && sprinting.0 {
            let horizontal = moved.delta.x.hypot(moved.delta.z) as f32;
            added += sprint_move_exhaustion(horizontal);
        }
        if added > 0.0 {
            add_player_exhaustion(mode.0, file_state, added);
        }
    }
}

pub(crate) fn collect_block_break_exhaustion(
    mut broken: MessageReader<BlockBroken>,
    mut players: Query<(&PlayerGameMode, &mut PlayerFileState), With<Player>>,
) {
    for event in broken.read() {
        if !event.harvests {
            continue;
        }
        let Ok((mode, file_state)) = players.get_mut(event.breaker) else {
            continue;
        };
        add_player_exhaustion(mode.0, file_state, HARVEST_BREAK_EXHAUSTION);
    }
}
