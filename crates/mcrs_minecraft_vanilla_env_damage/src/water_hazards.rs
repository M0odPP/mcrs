use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use glam::DVec3;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_world::world::storage::view::DimensionView;
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
use mcrs_minecraft_entity_attribute::AttributeInstances;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_env_damage::{self as env_damage, BreathableMedium, PLAYER_MAX_AIR};
use mcrs_minecraft_game_mode::GameMode;
use mcrs_minecraft_physics as physics;
use mcrs_minecraft_replication::MobSignal;
use mcrs_voxel::BlockPos;

use crate::bake::{EnvBlockTable, EnvContactBlock, EnvFluidKind};
use crate::env_state;
use crate::fall_system;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EyeWaterState {
    Dry,
    BubbleColumn,
    Submerged,
}

pub(crate) struct DrownInputs<'a> {
    pub(crate) player: Entity,
    pub(crate) eye: DVec3,
    pub(crate) mode: GameMode,
    pub(crate) now: u64,
    pub(crate) drowning_damage_enabled: bool,
    pub(crate) attributes: Option<&'a AttributeInstances>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrownEvaluation {
    pub(crate) next_air: i16,
    pub(crate) damage: Option<f32>,
}

const _: () = {
    assert!(
        size_of::<EyeWaterState>() == 1,
        "EyeWaterState size must match single-byte enum"
    );
    assert!(
        align_of::<EyeWaterState>() == 1,
        "EyeWaterState alignment must match single-byte enum"
    );
    assert!(
        size_of::<DrownInputs<'static>>() == 56,
        "DrownInputs size must match entity, eye, timing, and attribute fields"
    );
    assert!(
        align_of::<DrownInputs<'static>>() == 8,
        "DrownInputs alignment must match pointer and f64 alignment"
    );
    assert!(
        size_of::<DrownEvaluation>() == 12,
        "DrownEvaluation size must match air supply and optional damage"
    );
    assert!(
        align_of::<DrownEvaluation>() == 4,
        "DrownEvaluation alignment must match f32 alignment"
    );
};

#[must_use]
pub(crate) fn fluid_top_y(
    dimension_view: Option<&DimensionView<'_>>,
    block_table: &EnvBlockTable,
    block_position: BlockPos,
    fluid: EnvFluidKind,
) -> f64 {
    let above_state = fall_system::block_state_at(
        dimension_view,
        BlockPos::new(block_position.x, block_position.y + 1, block_position.z),
    );
    let above_fluid = block_table
        .get(above_state)
        .map_or(EnvFluidKind::None, |entry| entry.fluid);
    let same_above = matches!(
        (fluid, above_fluid),
        (EnvFluidKind::Water(_), EnvFluidKind::Water(_))
            | (EnvFluidKind::Lava(_), EnvFluidKind::Lava(_))
    );
    let level = match fluid {
        EnvFluidKind::Water(level) | EnvFluidKind::Lava(level) => level,
        EnvFluidKind::None => return f64::from(block_position.y),
    };
    let height = if same_above {
        1.0_f64
    } else {
        f64::from(level) / 9.0_f64
    };
    f64::from(block_position.y) + height
}

#[must_use]
pub(crate) fn eye_water_state(
    dimension_view: Option<&DimensionView<'_>>,
    block_table: &EnvBlockTable,
    eye: DVec3,
) -> EyeWaterState {
    let check_y = eye.y - (1.0_f64 / 9.0_f64);
    let fluid_position = BlockPos::new(
        physics::floor_block(eye.x),
        physics::floor_block(check_y),
        physics::floor_block(eye.z),
    );
    let state = fall_system::block_state_at(dimension_view, fluid_position);
    let Some(entry) = block_table.get(state) else {
        return EyeWaterState::Dry;
    };
    if !matches!(entry.fluid, EnvFluidKind::Water(_)) {
        return EyeWaterState::Dry;
    }
    let top = fluid_top_y(dimension_view, block_table, fluid_position, entry.fluid);
    if top <= check_y {
        return EyeWaterState::Dry;
    }
    let eye_block_position = BlockPos::new(
        physics::floor_block(eye.x),
        physics::floor_block(eye.y),
        physics::floor_block(eye.z),
    );
    let eye_block_state = fall_system::block_state_at(dimension_view, eye_block_position);
    if block_table
        .get(eye_block_state)
        .is_some_and(|eye_entry| eye_entry.contact == EnvContactBlock::BubbleColumn)
    {
        EyeWaterState::BubbleColumn
    } else {
        EyeWaterState::Submerged
    }
}

fn keep_air_via_oxygen_bonus(
    tick: u64,
    player: Entity,
    current_air: i16,
    attributes: Option<&AttributeInstances>,
) -> bool {
    let bonus = attributes
        .and_then(|instances| instances.value(Attribute::OxygenBonus))
        .unwrap_or(0.0_f64);
    if bonus <= 0.0_f64 {
        return false;
    }
    let seed = tick.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ player.to_bits()
        ^ (current_air as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    let mixed = ((seed ^ (seed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9) >> 40) as f64;
    let unit = mixed / ((1_u64 << 24) as f64);
    unit * (bonus + 1.0_f64) < bonus
}

pub(crate) fn step_player_drowning(
    dimension_view: Option<&DimensionView<'_>>,
    block_table: &EnvBlockTable,
    inputs: &DrownInputs<'_>,
    file_state: &PlayerFileState,
    signals: &mut MessageWriter<PlayerSignal>,
) -> Option<DrownEvaluation> {
    let water_state = eye_water_state(dimension_view, block_table, inputs.eye);
    match water_state {
        EyeWaterState::Dry | EyeWaterState::BubbleColumn => {
            let outcome = env_damage::step_air(
                file_state.air,
                PLAYER_MAX_AIR,
                BreathableMedium::AirOrWaterBreathing,
            );
            Some(DrownEvaluation {
                next_air: outcome.next_air,
                damage: None,
            })
        }
        EyeWaterState::Submerged => {
            let has_water_breathing =
                env_state::has_active_effect(file_state, MobEffect::WaterBreathing)
                    || env_state::has_active_effect(file_state, MobEffect::ConduitPower);
            let has_nautilus =
                env_state::has_active_effect(file_state, MobEffect::BreathOfTheNautilus);
            if has_nautilus && !has_water_breathing {
                return None;
            }
            let invulnerable = matches!(inputs.mode, GameMode::Creative | GameMode::Spectator);
            if invulnerable || has_water_breathing {
                let outcome = env_damage::step_air(
                    file_state.air,
                    PLAYER_MAX_AIR,
                    BreathableMedium::AirOrWaterBreathing,
                );
                return Some(DrownEvaluation {
                    next_air: outcome.next_air,
                    damage: None,
                });
            }
            if keep_air_via_oxygen_bonus(
                inputs.now,
                inputs.player,
                file_state.air,
                inputs.attributes,
            ) {
                return None;
            }
            let outcome =
                env_damage::step_air(file_state.air, PLAYER_MAX_AIR, BreathableMedium::Underwater);
            if outcome.emit_drown_particles {
                signals.write(PlayerSignal::new(inputs.player, MobSignal::DrownBubbles));
            }
            let damage = if inputs.drowning_damage_enabled {
                outcome.damage
            } else {
                None
            };
            Some(DrownEvaluation {
                next_air: outcome.next_air,
                damage,
            })
        }
    }
}
